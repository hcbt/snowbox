//! Virtualization.framework, in-process, macOS only.

use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::runtime::Runtime;
use crate::sandbox::Limits;

/// Locally administered unicast MAC derived from the Sandbox id so
/// concurrent guests do not share an ethernet address.
pub fn mac_address(id: Uuid) -> String {
    let b = id.as_bytes();
    format!(
        "02:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        b[0], b[1], b[2], b[3], b[4]
    )
}

pub fn console_log_path(sandbox_dir: &Path) -> PathBuf {
    sandbox_dir.join("console.log")
}

pub fn vm_queue_label(id: Uuid) -> String {
    format!("snowbox.vm.{id}")
}

/// Device set used for both boot and restore. Automatic disk caching
/// corrupts ARM Linux; entropy devices are not save/restore compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevicePolicy {
    pub disk_cached: bool,
    pub disk_full_sync: bool,
    pub entropy: bool,
    pub serial_file_url: bool,
    pub serial_append: bool,
}

pub fn device_policy() -> DevicePolicy {
    DevicePolicy {
        disk_cached: true,
        disk_full_sync: true,
        entropy: false,
        serial_file_url: true,
        serial_append: true,
    }
}

const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(target_os = "macos")]
pub fn is_supported() -> bool {
    use objc2_virtualization::VZVirtualMachine;
    unsafe { VZVirtualMachine::isSupported() }
}

#[cfg(not(target_os = "macos"))]
pub fn is_supported() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn pump_main_run_loop() {
    objc2_foundation::NSRunLoop::mainRunLoop().run();
}

#[cfg(target_os = "macos")]
pub struct VzEngine;

#[cfg(target_os = "macos")]
impl crate::vmm::Engine for VzEngine {
    fn boot(
        &self,
        runtime: &Runtime,
        id: Uuid,
        sandbox_dir: &Path,
        limits: Limits,
        mac_id: Uuid,
    ) -> Result<(), String> {
        run_vm(runtime, id, sandbox_dir, limits, mac_id, None)
    }

    fn restore(
        &self,
        runtime: &Runtime,
        id: Uuid,
        sandbox_dir: &Path,
        limits: Limits,
        mac_id: Uuid,
        save: &Path,
    ) -> Result<(), String> {
        run_vm(runtime, id, sandbox_dir, limits, mac_id, Some(save))
    }

    fn pause(&self, id: Uuid) -> Result<(), String> {
        pause_vm(id)
    }

    fn resume(&self, id: Uuid) -> Result<(), String> {
        resume_vm(id)
    }

    fn save(&self, id: Uuid, save: &Path) -> Result<(), String> {
        save_vm(id, save)
    }

    fn stop(&self, id: Uuid) -> Result<(), String> {
        stop_vm(id)
    }

    fn vsock(&self, id: Uuid, port: u32) -> Result<std::os::unix::net::UnixStream, String> {
        vsock_connect(id, port)
    }
}

#[cfg(target_os = "macos")]
struct SendVm(objc2::rc::Retained<objc2_virtualization::VZVirtualMachine>);

#[cfg(target_os = "macos")]
unsafe impl Send for SendVm {}
#[cfg(target_os = "macos")]
unsafe impl Sync for SendVm {}

#[cfg(target_os = "macos")]
impl Clone for SendVm {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Owns the running VM until stop's completion handler (or Destroy).
#[cfg(target_os = "macos")]
struct LiveVm {
    queue: dispatch2::DispatchRetained<dispatch2::DispatchQueue>,
    vm: Option<SendVm>,
    #[allow(dead_code)]
    net: usize,
    save_restore_ok: bool,
}

#[cfg(target_os = "macos")]
impl Drop for LiveVm {
    fn drop(&mut self) {
        if let Some(vm) = self.vm.take() {
            let queue = self.queue.clone();
            queue.exec_async(move || drop(vm));
        }
    }
}

#[cfg(target_os = "macos")]
static VMS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<Uuid, LiveVm>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// CF-retained `vmnet_network`, keyed by Sandbox UUID, held until Destroy
/// or process exit. Same-process only; a new SHARED_MODE object after
/// Daemon restart is not restore-compatible.
#[cfg(target_os = "macos")]
static NETWORKS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<Uuid, usize>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// One NAT network per Sandbox. Shared-mode vmnet lets guests reach the
/// internet through the Host; a distinct network object means they cannot
/// reach each other.
#[cfg(target_os = "macos")]
fn attach_isolated_network(
    net: &objc2_virtualization::VZVirtioNetworkDeviceConfiguration,
    sandbox_id: Uuid,
    mac_id: Uuid,
) -> Result<(), String> {
    use std::ffi::c_void;

    use objc2::AnyThread;
    use objc2::ClassType;
    use objc2::encode::{Encoding, RefEncode};
    use objc2::rc::Retained;
    use objc2_foundation::NSString;
    use objc2_virtualization::{VZMACAddress, VZVmnetNetworkDeviceAttachment};

    const VMNET_SHARED_MODE: u32 = 1001;
    const VMNET_MEM_FAILURE: u32 = 1002;

    #[repr(C)]
    struct vmnet_network {
        _priv: [u8; 0],
    }

    unsafe impl RefEncode for vmnet_network {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("vmnet_network", &[]));
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFRetain(cf: *const c_void) -> *const c_void;
    }

    type CreateConfig = unsafe extern "C" fn(u32, *mut u32) -> *mut c_void;
    type CreateNetwork = unsafe extern "C" fn(*mut c_void, *mut u32) -> *mut vmnet_network;

    unsafe fn vmnet_sym<T>(name: &std::ffi::CStr) -> Result<T, String> {
        let handle = unsafe {
            libc::dlopen(
                c"/System/Library/Frameworks/vmnet.framework/vmnet".as_ptr(),
                libc::RTLD_LAZY | libc::RTLD_LOCAL,
            )
        };
        if handle.is_null() {
            return Err("dlopen vmnet failed".into());
        }
        let ptr = unsafe { libc::dlsym(handle, name.as_ptr()) };
        if ptr.is_null() {
            return Err(format!("vmnet missing {}", name.to_string_lossy()));
        }
        Ok(unsafe { std::mem::transmute_copy(&ptr) })
    }

    fn status_msg(what: &str, status: u32) -> String {
        if status == VMNET_MEM_FAILURE {
            format!("{what} failed ({status}; VMNET_MEM_FAILURE)")
        } else {
            format!("{what} failed ({status})")
        }
    }

    unsafe {
        let mut map = NETWORKS.lock().expect("networks");
        let network = if let Some(&ptr) = map.get(&sandbox_id) {
            ptr as *mut vmnet_network
        } else {
            let create_config: CreateConfig = vmnet_sym(c"vmnet_network_configuration_create")?;
            let create_network: CreateNetwork = vmnet_sym(c"vmnet_network_create")?;
            let mut status = 0u32;
            let config = create_config(VMNET_SHARED_MODE, &mut status);
            if config.is_null() {
                return Err(status_msg("vmnet configuration", status));
            }
            let network = create_network(config, &mut status);
            CFRelease(config);
            if network.is_null() {
                return Err(status_msg("vmnet network", status));
            }
            CFRetain(network as *const c_void);
            map.insert(sandbox_id, network as usize);
            network
        };

        let attachment: Option<Retained<VZVmnetNetworkDeviceAttachment>> = objc2::msg_send![
            VZVmnetNetworkDeviceAttachment::alloc(),
            initWithNetwork: network
        ];
        let Some(attachment) = attachment else {
            return Err("isolated network attachment failed".into());
        };
        let mac = VZMACAddress::initWithString(
            VZMACAddress::alloc(),
            &NSString::from_str(&mac_address(mac_id)),
        )
        .ok_or_else(|| format!("invalid MAC {}", mac_address(mac_id)))?;
        net.setMACAddress(&mac);
        net.setAttachment(Some(attachment.as_super()));
    }
    Ok(())
}

fn wait_result<T>(
    rx: std::sync::mpsc::Receiver<Result<T, String>>,
    timeout: std::time::Duration,
    what: &str,
) -> Result<T, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        match rx.try_recv() {
            Ok(r) => return r,
            Err(std::sync::mpsc::TryRecvError::Empty) if std::time::Instant::now() < deadline => {
                continue;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                return Err(format!("{what} timed out"));
            }
            Err(e) => return Err(format!("channel: {e}")),
        }
    }
}

#[cfg(target_os = "macos")]
fn attach_hvc0(
    config: &objc2_virtualization::VZVirtualMachineConfiguration,
    sandbox_dir: &Path,
) -> Result<(), String> {
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_foundation::NSArray;
    use objc2_virtualization::{
        VZFileSerialPortAttachment, VZVirtioConsoleDeviceSerialPortConfiguration,
    };

    let policy = device_policy();
    let log = console_log_path(sandbox_dir);
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("console.log: {e}"))?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(policy.serial_append)
        .open(&log)
        .map_err(|e| format!("console.log: {e}"))?;
    let log_str = path_str(&log)?;
    unsafe {
        let attach = VZFileSerialPortAttachment::initWithURL_append_error(
            VZFileSerialPortAttachment::alloc(),
            &nsurl(&log_str),
            policy.serial_append,
        )
        .map_err(|e| format!("console.log: {e}"))?;
        let port = VZVirtioConsoleDeviceSerialPortConfiguration::new();
        let attach: Retained<objc2_virtualization::VZSerialPortAttachment> =
            Retained::into_super(attach);
        port.setAttachment(Some(&attach));
        config.setSerialPorts(&NSArray::from_retained_slice(&[Retained::into_super(port)]));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
struct BuiltConfig {
    config: objc2::rc::Retained<objc2_virtualization::VZVirtualMachineConfiguration>,
    save_restore_ok: bool,
}

#[cfg(target_os = "macos")]
fn make_config(
    runtime: &Runtime,
    sandbox_id: Uuid,
    sandbox_dir: &Path,
    limits: Limits,
    mac_id: Uuid,
) -> Result<BuiltConfig, String> {
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_foundation::{NSArray, NSData, NSString};
    use objc2_virtualization::*;

    let policy = device_policy();
    let root = sandbox_dir.join("disk").join("root.raw");
    let kernel = path_str(&runtime.kernel)?;
    let initrd = path_str(&runtime.initrd)?;
    let root_str = path_str(&root)?;

    unsafe {
        let platform =
            VZGenericPlatformConfiguration::init(VZGenericPlatformConfiguration::alloc());
        let ident_path = sandbox_dir.join("machine.ident");
        let ident = if ident_path.is_file() {
            let bytes = std::fs::read(&ident_path).map_err(|e| format!("read machine id: {e}"))?;
            let data = NSData::with_bytes(&bytes);
            VZGenericMachineIdentifier::initWithDataRepresentation(
                VZGenericMachineIdentifier::alloc(),
                &data,
            )
            .ok_or_else(|| "invalid machine identifier".to_string())?
        } else {
            let ident = VZGenericMachineIdentifier::init(VZGenericMachineIdentifier::alloc());
            let data = ident.dataRepresentation();
            std::fs::write(&ident_path, data.as_bytes_unchecked())
                .map_err(|e| format!("write machine id: {e}"))?;
            ident
        };
        platform.setMachineIdentifier(&ident);
        let boot_loader =
            VZLinuxBootLoader::initWithKernelURL(VZLinuxBootLoader::alloc(), &nsurl(&kernel));
        boot_loader.setInitialRamdiskURL(Some(&nsurl(&initrd)));
        boot_loader.setCommandLine(&NSString::from_str(&runtime.boot_cmdline()));

        let config = VZVirtualMachineConfiguration::new();
        config.setPlatform(&platform);
        config.setBootLoader(Some(&boot_loader));
        config.setCPUCount(usize::try_from(limits.cpu).expect("cpu fits usize"));
        config.setMemorySize(limits.ram);

        let caching = if policy.disk_cached {
            VZDiskImageCachingMode::Cached
        } else {
            VZDiskImageCachingMode::Uncached
        };
        let sync = if policy.disk_full_sync {
            VZDiskImageSynchronizationMode::Full
        } else {
            VZDiskImageSynchronizationMode::Fsync
        };
        let disk_attach = VZDiskImageStorageDeviceAttachment::initWithURL_readOnly_cachingMode_synchronizationMode_error(
            VZDiskImageStorageDeviceAttachment::alloc(),
            &nsurl(&root_str),
            false,
            caching,
            sync,
        )
        .map_err(|e| format!("disk: {e}"))?;
        let disk = VZVirtioBlockDeviceConfiguration::initWithAttachment(
            VZVirtioBlockDeviceConfiguration::alloc(),
            &disk_attach,
        );
        config.setStorageDevices(&NSArray::from_retained_slice(&[Retained::into_super(disk)]));

        let net = VZVirtioNetworkDeviceConfiguration::new();
        attach_isolated_network(&net, sandbox_id, mac_id)?;
        config.setNetworkDevices(&NSArray::from_retained_slice(&[Retained::into_super(net)]));

        if policy.entropy {
            config.setEntropyDevices(&NSArray::from_retained_slice(&[Retained::into_super(
                VZVirtioEntropyDeviceConfiguration::new(),
            )]));
        }

        config.setMemoryBalloonDevices(&NSArray::from_retained_slice(&[Retained::into_super(
            VZVirtioTraditionalMemoryBalloonDeviceConfiguration::new(),
        )]));
        config.setSocketDevices(&NSArray::from_retained_slice(&[Retained::into_super(
            VZVirtioSocketDeviceConfiguration::new(),
        )]));

        if policy.serial_file_url {
            attach_hvc0(&config, sandbox_dir)?;
        }

        config
            .validateWithError()
            .map_err(|e| format!("invalid vm: {e}"))?;
        let save_restore_ok = match config.validateSaveRestoreSupportWithError() {
            Ok(()) => true,
            Err(e) => {
                eprintln!("machine state unsupported ({e}); Stop will boot next Start");
                false
            }
        };
        Ok(BuiltConfig {
            config,
            save_restore_ok,
        })
    }
}

#[cfg(target_os = "macos")]
fn run_vm(
    runtime: &Runtime,
    id: Uuid,
    sandbox_dir: &Path,
    limits: Limits,
    mac_id: Uuid,
    save: Option<&Path>,
) -> Result<(), String> {
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2::AnyThread;
    use objc2_foundation::NSError;
    use objc2_virtualization::VZVirtualMachine;

    if !is_supported() {
        return Err("virtualization is not supported on this Host".into());
    }
    if let Some(save) = save
        && !save.is_file()
    {
        return Err("no machine state".into());
    }

    let queue =
        dispatch2::DispatchQueue::new(&vm_queue_label(id), dispatch2::DispatchQueueAttr::SERIAL);
    let queue_vm = queue.clone();
    let runtime = runtime.clone();
    let sandbox_dir = sandbox_dir.to_path_buf();
    let save_path = save.map(|p| p.to_path_buf());
    let restoring = save_path.is_some();
    let save = save_path.clone();
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    queue.exec_async(move || {
        let built = match make_config(&runtime, id, &sandbox_dir, limits, mac_id) {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        if restoring && !built.save_restore_ok {
            let _ = tx.send(Err("machine state unsupported".into()));
            return;
        }
        let vm = unsafe {
            VZVirtualMachine::initWithConfiguration_queue(
                VZVirtualMachine::alloc(),
                &built.config,
                &queue_vm,
            )
        };
        let send_vm = SendVm(vm);
        let net = NETWORKS
            .lock()
            .expect("networks")
            .get(&id)
            .copied()
            .unwrap_or(0);
        VMS.lock().expect("vms").insert(
            id,
            LiveVm {
                queue: queue_vm,
                vm: Some(send_vm.clone()),
                net,
                save_restore_ok: built.save_restore_ok,
            },
        );

        if let Some(save) = save {
            let save_str = match path_str(&save) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            let vm = send_vm.clone();
            let handler = RcBlock::new(move |error: *mut NSError| {
                if !error.is_null() {
                    let e = unsafe { &*error };
                    let _ = tx.send(Err(format!("{}", e.localizedDescription())));
                    return;
                }
                if !unsafe { vm.0.canResume() } {
                    let _ = tx.send(Err("restored sandbox cannot resume".into()));
                    return;
                }
                let resume = error_handler(tx.clone());
                unsafe { vm.0.resumeWithCompletionHandler(&resume) };
            });
            unsafe {
                send_vm
                    .0
                    .restoreMachineStateFromURL_completionHandler(&nsurl(&save_str), &handler);
            }
        } else {
            let completion = error_handler(tx);
            unsafe { send_vm.0.startWithCompletionHandler(&completion) };
        }
    });
    wait_result(
        rx,
        START_TIMEOUT,
        if restoring { "restore" } else { "start" },
    )
    .inspect_err(|_| {
        let _ = stop_vm(id);
    })?;
    if let Some(save) = save_path {
        let _ = std::fs::remove_file(save);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn error_handler(
    tx: std::sync::mpsc::Sender<Result<(), String>>,
) -> block2::RcBlock<dyn Fn(*mut objc2_foundation::NSError)> {
    use objc2_foundation::NSError;
    block2::RcBlock::new(move |error: *mut NSError| {
        if error.is_null() {
            let _ = tx.send(Ok(()));
        } else {
            let e = unsafe { &*error };
            let _ = tx.send(Err(format!("{}", e.localizedDescription())));
        }
    })
}

#[cfg(target_os = "macos")]
fn live_queue(id: Uuid) -> Result<dispatch2::DispatchRetained<dispatch2::DispatchQueue>, String> {
    VMS.lock()
        .expect("vms")
        .get(&id)
        .map(|l| l.queue.clone())
        .ok_or_else(|| "sandbox is not running".to_string())
}

#[cfg(target_os = "macos")]
fn pause_vm(id: Uuid) -> Result<(), String> {
    use std::sync::mpsc;

    let queue = live_queue(id)?;
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    queue.exec_async(move || {
        let vm = match live_send_vm(id) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        if !unsafe { vm.0.canPause() } {
            let _ = tx.send(Err("sandbox cannot pause".into()));
            return;
        }
        let handler = error_handler(tx);
        unsafe { vm.0.pauseWithCompletionHandler(&handler) };
    });
    wait_result(rx, STOP_TIMEOUT, "pause")
}

#[cfg(target_os = "macos")]
fn resume_vm(id: Uuid) -> Result<(), String> {
    use std::sync::mpsc;

    let queue = live_queue(id)?;
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    queue.exec_async(move || {
        let vm = match live_send_vm(id) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        if !unsafe { vm.0.canResume() } {
            let _ = tx.send(Err("sandbox cannot resume".into()));
            return;
        }
        let handler = error_handler(tx);
        unsafe { vm.0.resumeWithCompletionHandler(&handler) };
    });
    wait_result(rx, STOP_TIMEOUT, "resume")
}

#[cfg(target_os = "macos")]
fn live_send_vm(id: Uuid) -> Result<SendVm, String> {
    VMS.lock()
        .expect("vms")
        .get(&id)
        .and_then(|l| l.vm.clone())
        .ok_or_else(|| "sandbox is not running".to_string())
}

#[cfg(target_os = "macos")]
fn save_vm(id: Uuid, save: &Path) -> Result<(), String> {
    use std::sync::mpsc;

    let save_restore_ok = VMS
        .lock()
        .expect("vms")
        .get(&id)
        .map(|l| l.save_restore_ok)
        .ok_or_else(|| "sandbox is not running".to_string())?;
    if !save_restore_ok {
        return Err("machine state unsupported".into());
    }
    if let Some(parent) = save.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir save: {e}"))?;
    }
    let _ = std::fs::remove_file(save);
    let save_str = path_str(save)?;
    let queue = live_queue(id)?;
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    queue.exec_async(move || {
        let vm = match live_send_vm(id) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        let handler = error_handler(tx);
        unsafe {
            vm.0.saveMachineStateToURL_completionHandler(&nsurl(&save_str), &handler);
        }
    });
    wait_result(rx, std::time::Duration::from_secs(120), "save")
}

#[cfg(target_os = "macos")]
fn stop_vm(id: Uuid) -> Result<(), String> {
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2_foundation::NSError;
    use objc2_virtualization::VZVirtualMachineState;

    let Ok(queue) = live_queue(id) else {
        return Ok(());
    };
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    queue.exec_async(move || {
        let mut map = VMS.lock().expect("vms");
        let Some(live) = map.get(&id) else {
            let _ = tx.send(Ok(()));
            return;
        };
        let Some(vm) = live.vm.clone() else {
            drop(map.remove(&id));
            let _ = tx.send(Ok(()));
            return;
        };
        let can_stop = unsafe { vm.0.canStop() };
        let stopping = unsafe { vm.0.state() } == VZVirtualMachineState::Stopping;
        if !can_stop {
            if !stopping {
                drop(map.remove(&id));
            }
            let _ = tx.send(Ok(()));
            return;
        }
        drop(map);
        let vm_stop = vm.clone();
        let handler = RcBlock::new(move |error: *mut NSError| {
            let _keep = &vm;
            drop(VMS.lock().expect("vms").remove(&id));
            if error.is_null() {
                let _ = tx.send(Ok(()));
            } else {
                let e = unsafe { &*error };
                let _ = tx.send(Err(format!("{}", e.localizedDescription())));
            }
        });
        unsafe { vm_stop.0.stopWithCompletionHandler(&handler) };
    });
    wait_result(rx, STOP_TIMEOUT, "stop")
}

#[cfg(target_os = "macos")]
fn vsock_connect(id: Uuid, port: u32) -> Result<std::os::unix::net::UnixStream, String> {
    use std::os::fd::FromRawFd;
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_foundation::NSError;
    use objc2_virtualization::{VZVirtioSocketConnection, VZVirtioSocketDevice};

    let queue = live_queue(id)?;
    let (tx, rx) = mpsc::channel::<Result<i32, String>>();
    queue.exec_async(move || {
        let vm = match live_send_vm(id) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        let sockets = unsafe { vm.0.socketDevices() };
        if sockets.is_empty() {
            let _ = tx.send(Err("no vsock device".into()));
            return;
        }
        let device: Retained<VZVirtioSocketDevice> =
            unsafe { Retained::cast_unchecked(sockets.objectAtIndex(0)) };
        let handler = RcBlock::new(
            move |connection: *mut VZVirtioSocketConnection, error: *mut NSError| {
                if !error.is_null() {
                    let e = unsafe { &*error };
                    let _ = tx.send(Err(format!("{}", e.localizedDescription())));
                    return;
                }
                if connection.is_null() {
                    let _ = tx.send(Err("vsock connect returned null".into()));
                    return;
                }
                let Some(conn) = (unsafe { Retained::retain(connection) }) else {
                    let _ = tx.send(Err("vsock retain failed".into()));
                    return;
                };
                let fd = unsafe { conn.fileDescriptor() };
                if fd < 0 {
                    let _ = tx.send(Err("vsock fd closed".into()));
                    return;
                }
                let duped = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
                drop(conn);
                if duped < 0 {
                    let _ = tx.send(Err("dup vsock fd failed".into()));
                } else {
                    let _ = tx.send(Ok(duped));
                }
            },
        );
        let dyn_handler: &block2::DynBlock<dyn Fn(*mut VZVirtioSocketConnection, *mut NSError)> =
            &handler;
        unsafe { device.connectToPort_completionHandler(port, dyn_handler) };
    });

    match wait_result(rx, std::time::Duration::from_secs(10), "vsock connect") {
        Ok(fd) => Ok(unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) }),
        Err(e) => Err(e),
    }
}

#[cfg(target_os = "macos")]
fn nsurl(path: &str) -> objc2::rc::Retained<objc2_foundation::NSURL> {
    use objc2_foundation::{NSString, NSURL};
    NSURL::fileURLWithPath(&NSString::from_str(path))
}

fn path_str(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("non-utf8 path: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn reports_support_without_creating_a_vm() {
        let _ = super::is_supported();
    }

    #[test]
    fn mac_address_is_stable_locally_administered_unicast() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let mac = super::mac_address(id);
        assert_eq!(mac, super::mac_address(id));
        let other = super::mac_address(uuid::Uuid::nil());
        assert_ne!(mac, other);
        let first = u8::from_str_radix(&mac[..2], 16).unwrap();
        assert_eq!(first & 0x01, 0, "unicast");
        assert_eq!(first & 0x02, 0x02, "locally administered");
        assert_eq!(mac.chars().filter(|c| *c == ':').count(), 5);
    }

    #[test]
    fn mac_addresses_are_unique_across_ids() {
        let mut seen = HashSet::new();
        for _ in 0..1_000 {
            let mac = super::mac_address(Uuid::new_v4());
            assert!(seen.insert(mac));
        }
    }

    #[test]
    fn console_log_path_is_sandbox_console_log() {
        let dir = Path::new("/tmp/sb");
        assert_eq!(
            super::console_log_path(dir),
            Path::new("/tmp/sb/console.log")
        );
    }

    #[test]
    fn vm_queue_is_per_sandbox() {
        let id = Uuid::nil();
        assert_eq!(super::vm_queue_label(id), format!("snowbox.vm.{id}"));
        assert_ne!(
            super::vm_queue_label(id),
            super::vm_queue_label(Uuid::from_u128(1))
        );
    }

    #[test]
    fn device_policy_is_cached_full_no_entropy_file_serial() {
        let p = super::device_policy();
        assert!(p.disk_cached);
        assert!(p.disk_full_sync);
        assert!(!p.entropy);
        assert!(p.serial_file_url);
        assert!(p.serial_append);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn console_log_file_url_contains_path() {
        let url = super::nsurl("/tmp/snowbox-console.log");
        let path = url.path().expect("path");
        assert!(path.to_string().contains("snowbox-console.log"));
    }
}
