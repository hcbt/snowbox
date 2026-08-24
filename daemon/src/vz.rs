//! Virtualization.framework, in-process, macOS only.

use std::path::Path;
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
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(target_os = "macos")]
pub fn is_supported() -> bool {
    use objc2_virtualization::VZVirtualMachine;
    unsafe { VZVirtualMachine::isSupported() }
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
        start_vm(runtime, id, sandbox_dir, limits, mac_id)
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
        restore_vm(runtime, id, sandbox_dir, limits, mac_id, save)
    }

    fn pause(&self, id: Uuid) -> Result<(), String> {
        pause_vm(id)
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

/// One NAT network per Sandbox. Shared-mode vmnet lets guests reach the
/// internet through the Host; a distinct network object means they cannot
/// reach each other.
#[cfg(target_os = "macos")]
fn attach_isolated_network(
    net: &objc2_virtualization::VZVirtioNetworkDeviceConfiguration,
    id: Uuid,
) -> Result<(), String> {
    use std::ffi::c_void;

    use objc2::AnyThread;
    use objc2::ClassType;
    use objc2::encode::{Encoding, RefEncode};
    use objc2::rc::Retained;
    use objc2_foundation::NSString;
    use objc2_virtualization::{VZMACAddress, VZVmnetNetworkDeviceAttachment};

    const VMNET_SHARED_MODE: u32 = 1001;

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

    unsafe {
        let mut map = NETWORKS.lock().expect("networks");
        let network = if let Some(&ptr) = map.get(&id) {
            ptr as *mut vmnet_network
        } else {
            let create_config: CreateConfig = vmnet_sym(c"vmnet_network_configuration_create")?;
            let create_network: CreateNetwork = vmnet_sym(c"vmnet_network_create")?;
            let mut status = 0u32;
            let config = create_config(VMNET_SHARED_MODE, &mut status);
            if config.is_null() {
                return Err(format!("vmnet configuration failed ({status})"));
            }
            let network = create_network(config, &mut status);
            CFRelease(config);
            if network.is_null() {
                return Err(format!("vmnet network failed ({status})"));
            }
            map.insert(id, network as usize);
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
            &NSString::from_str(&mac_address(id)),
        )
        .ok_or_else(|| format!("invalid MAC {}", mac_address(id)))?;
        net.setMACAddress(&mac);
        net.setAttachment(Some(attachment.as_super()));
    }
    Ok(())
}

fn wait_result(
    rx: std::sync::mpsc::Receiver<Result<(), String>>,
    timeout: std::time::Duration,
    what: &str,
) -> Result<(), String> {
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
    use std::os::fd::IntoRawFd;

    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_foundation::{NSArray, NSFileHandle, NSString};
    use objc2_virtualization::{
        VZFileHandleSerialPortAttachment, VZVirtioConsoleDeviceConfiguration,
        VZVirtioConsolePortConfiguration,
    };

    let log = sandbox_dir.join("console.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(|e| format!("console.log: {e}"))?;
    let fd = file.into_raw_fd();
    unsafe {
        let write = NSFileHandle::initWithFileDescriptor(NSFileHandle::alloc(), fd);
        let attach = VZFileHandleSerialPortAttachment::initWithFileHandleForReading_fileHandleForWriting(
            VZFileHandleSerialPortAttachment::alloc(),
            None,
            Some(&write),
        );
        let port = VZVirtioConsolePortConfiguration::new();
        port.setIsConsole(true);
        port.setName(Some(&NSString::from_str("org.snowbox.console")));
        let attach: Retained<objc2_virtualization::VZSerialPortAttachment> =
            Retained::into_super(attach);
        port.setAttachment(Some(&attach));
        let console = VZVirtioConsoleDeviceConfiguration::new();
        console.ports().setObject_atIndexedSubscript(Some(&port), 0);
        config.setConsoleDevices(&NSArray::from_retained_slice(&[Retained::into_super(
            console,
        )]));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn make_config(
    runtime: &Runtime,
    sandbox_dir: &Path,
    limits: Limits,
    mac_id: Uuid,
    console: bool,
) -> Result<objc2::rc::Retained<objc2_virtualization::VZVirtualMachineConfiguration>, String> {
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_foundation::{NSArray, NSData, NSString};
    use objc2_virtualization::*;

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
            VZLinuxBootLoader::initWithKernelURL(VZLinuxBootLoader::alloc(), &nsurl(kernel));
        boot_loader.setInitialRamdiskURL(Some(&nsurl(initrd)));
        boot_loader.setCommandLine(&NSString::from_str(&runtime.boot_cmdline()));

        let config = VZVirtualMachineConfiguration::new();
        config.setPlatform(&platform);
        config.setBootLoader(Some(&boot_loader));
        config.setCPUCount(usize::try_from(limits.cpu).expect("cpu fits usize"));
        config.setMemorySize(limits.ram);

        let disk_attach = VZDiskImageStorageDeviceAttachment::initWithURL_readOnly_cachingMode_synchronizationMode_error(
            VZDiskImageStorageDeviceAttachment::alloc(),
            &nsurl(root_str),
            false,
            VZDiskImageCachingMode::Automatic,
            VZDiskImageSynchronizationMode::Full,
        )
        .map_err(|e| format!("disk: {e}"))?;
        let disk = VZVirtioBlockDeviceConfiguration::initWithAttachment(
            VZVirtioBlockDeviceConfiguration::alloc(),
            &disk_attach,
        );
        config.setStorageDevices(&NSArray::from_retained_slice(&[Retained::into_super(disk)]));

        let net = VZVirtioNetworkDeviceConfiguration::new();
        attach_isolated_network(&net, mac_id)?;
        config.setNetworkDevices(&NSArray::from_retained_slice(&[Retained::into_super(net)]));

        config.setEntropyDevices(&NSArray::from_retained_slice(&[Retained::into_super(
            VZVirtioEntropyDeviceConfiguration::new(),
        )]));
        config.setMemoryBalloonDevices(&NSArray::from_retained_slice(&[Retained::into_super(
            VZVirtioTraditionalMemoryBalloonDeviceConfiguration::new(),
        )]));
        config.setSocketDevices(&NSArray::from_retained_slice(&[Retained::into_super(
            VZVirtioSocketDeviceConfiguration::new(),
        )]));

        // Virtio console (hvc0) only on cold boot. A new file handle each
        // Start makes restore fail with invalid argument.
        if console {
            attach_hvc0(&config, sandbox_dir)?;
        }

        config
            .validateWithError()
            .map_err(|e| format!("invalid vm: {e}"))?;
        if let Err(e) = config.validateSaveRestoreSupportWithError() {
            eprintln!("machine state unsupported ({e}); Stop will boot next Start");
        }
        Ok(config)
    }
}

#[cfg(target_os = "macos")]
fn start_vm(
    runtime: &Runtime,
    id: Uuid,
    sandbox_dir: &Path,
    limits: Limits,
    mac_id: Uuid,
) -> Result<(), String> {
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_foundation::NSError;
    use objc2_virtualization::VZVirtualMachine;

    if !is_supported() {
        return Err("virtualization is not supported on this Host".into());
    }
    let config = make_config(runtime, sandbox_dir, limits, mac_id, true)?;
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let config_ptr = Retained::into_raw(config) as usize;
    dispatch2::DispatchQueue::main().exec_async(move || {
        let config = unsafe {
            Retained::from_raw(
                config_ptr as *mut objc2_virtualization::VZVirtualMachineConfiguration,
            )
        }
        .expect("config pointer");
        let vm = unsafe {
            VZVirtualMachine::initWithConfiguration_queue(
                VZVirtualMachine::alloc(),
                &config,
                dispatch2::DispatchQueue::main(),
            )
        };
        let tx_clone = tx.clone();
        let handler = RcBlock::new(move |error: *mut NSError| {
            if error.is_null() {
                let _ = tx_clone.send(Ok(()));
            } else {
                let e = unsafe { &*error };
                let _ = tx_clone.send(Err(format!("{}", e.localizedDescription())));
            }
        });
        unsafe { vm.startWithCompletionHandler(&handler) };
        let vm_ptr = Retained::into_raw(vm) as usize;
        VMS.lock().expect("vms").insert(id, vm_ptr);
    });
    wait_result(rx, START_TIMEOUT, "start").inspect_err(|_| {
        let _ = stop_vm(id);
    })
}

#[cfg(target_os = "macos")]
fn restore_vm(
    runtime: &Runtime,
    id: Uuid,
    sandbox_dir: &Path,
    limits: Limits,
    mac_id: Uuid,
    save: &Path,
) -> Result<(), String> {
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_foundation::NSError;
    use objc2_virtualization::VZVirtualMachine;

    if !is_supported() {
        return Err("virtualization is not supported on this Host".into());
    }
    if !save.is_file() {
        return Err("no machine state".into());
    }
    let config = make_config(runtime, sandbox_dir, limits, mac_id, false)?;
    unsafe {
        config
            .validateSaveRestoreSupportWithError()
            .map_err(|e| format!("machine state unsupported: {e}"))?;
    }
    let save_str = path_str(save)?.to_string();
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let config_ptr = Retained::into_raw(config) as usize;
    dispatch2::DispatchQueue::main().exec_async(move || {
        let config = unsafe {
            Retained::from_raw(
                config_ptr as *mut objc2_virtualization::VZVirtualMachineConfiguration,
            )
        }
        .expect("config pointer");
        let vm = unsafe {
            VZVirtualMachine::initWithConfiguration_queue(
                VZVirtualMachine::alloc(),
                &config,
                dispatch2::DispatchQueue::main(),
            )
        };
        let vm_ptr = Retained::into_raw(vm) as usize;
        VMS.lock().expect("vms").insert(id, vm_ptr);
        let tx_restore = tx.clone();
        let handler = RcBlock::new(move |error: *mut NSError| {
            if !error.is_null() {
                let e = unsafe { &*error };
                let _ = tx_restore.send(Err(format!("{}", e.localizedDescription())));
                return;
            }
            let vm = unsafe { &*(vm_ptr as *const VZVirtualMachine) };
            if !unsafe { vm.canResume() } {
                let _ = tx_restore.send(Err("restored guest cannot resume".into()));
                return;
            }
            let tx_resume = tx_restore.clone();
            let resume = RcBlock::new(move |error: *mut NSError| {
                if error.is_null() {
                    let _ = tx_resume.send(Ok(()));
                } else {
                    let e = unsafe { &*error };
                    let _ = tx_resume.send(Err(format!("{}", e.localizedDescription())));
                }
            });
            unsafe { vm.resumeWithCompletionHandler(&resume) };
        });
        let vm = unsafe { &*(vm_ptr as *const VZVirtualMachine) };
        unsafe {
            vm.restoreMachineStateFromURL_completionHandler(&nsurl(&save_str), &handler);
        }
    });
    wait_result(rx, START_TIMEOUT, "restore").inspect_err(|_| {
        let _ = stop_vm(id);
    })
}

#[cfg(target_os = "macos")]
fn pause_vm(id: Uuid) -> Result<(), String> {
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2_foundation::NSError;
    use objc2_virtualization::VZVirtualMachine;

    let ptr = VMS
        .lock()
        .expect("vms")
        .get(&id)
        .copied()
        .ok_or_else(|| "sandbox is not running".to_string())?;
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    dispatch2::DispatchQueue::main().exec_async(move || {
        let vm = unsafe { &*(ptr as *const VZVirtualMachine) };
        if !unsafe { vm.canPause() } {
            let _ = tx.send(Err("guest cannot pause".into()));
            return;
        }
        let handler = RcBlock::new(move |error: *mut NSError| {
            if error.is_null() {
                let _ = tx.send(Ok(()));
            } else {
                let e = unsafe { &*error };
                let _ = tx.send(Err(format!("{}", e.localizedDescription())));
            }
        });
        unsafe { vm.pauseWithCompletionHandler(&handler) };
    });
    wait_result(rx, STOP_TIMEOUT, "pause")
}

#[cfg(target_os = "macos")]
fn save_vm(id: Uuid, save: &Path) -> Result<(), String> {
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2_foundation::NSError;
    use objc2_virtualization::VZVirtualMachine;

    if let Some(parent) = save.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir save: {e}"))?;
    }
    let _ = std::fs::remove_file(save);
    let save_str = path_str(save)?.to_string();
    let ptr = VMS
        .lock()
        .expect("vms")
        .get(&id)
        .copied()
        .ok_or_else(|| "sandbox is not running".to_string())?;
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    dispatch2::DispatchQueue::main().exec_async(move || {
        let vm = unsafe { &*(ptr as *const VZVirtualMachine) };
        let handler = RcBlock::new(move |error: *mut NSError| {
            if error.is_null() {
                let _ = tx.send(Ok(()));
            } else {
                let e = unsafe { &*error };
                let _ = tx.send(Err(format!("{}", e.localizedDescription())));
            }
        });
        unsafe { vm.saveMachineStateToURL_completionHandler(&nsurl(&save_str), &handler) };
    });
    wait_result(rx, std::time::Duration::from_secs(120), "save")
}

#[cfg(target_os = "macos")]
fn stop_vm(id: Uuid) -> Result<(), String> {
    use std::sync::mpsc;
    use std::time::Instant;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_foundation::NSError;
    use objc2_virtualization::VZVirtualMachine;

    let ptr = {
        let mut map = VMS.lock().expect("vms");
        map.remove(&id)
    };
    let Some(ptr) = ptr else {
        return Ok(());
    };

    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    dispatch2::DispatchQueue::main().exec_async(move || {
        let vm = unsafe { Retained::from_raw(ptr as *mut VZVirtualMachine) };
        let Some(vm) = vm else {
            let _ = tx.send(Ok(()));
            return;
        };
        if !unsafe { vm.canStop() } {
            let _ = tx.send(Ok(()));
            return;
        }
        let handler = RcBlock::new(move |error: *mut NSError| {
            if error.is_null() {
                let _ = tx.send(Ok(()));
            } else {
                let e = unsafe { &*error };
                let _ = tx.send(Err(format!("{}", e.localizedDescription())));
            }
        });
        unsafe { vm.stopWithCompletionHandler(&handler) };
    });

    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        match rx.try_recv() {
            Ok(r) => return r,
            Err(mpsc::TryRecvError::Empty) if Instant::now() < deadline => continue,
            Err(mpsc::TryRecvError::Empty) => return Err("stop timed out".into()),
            Err(e) => return Err(format!("channel: {e}")),
        }
    }
}

#[cfg(target_os = "macos")]
fn vsock_connect(id: Uuid, port: u32) -> Result<std::os::unix::net::UnixStream, String> {
    use std::os::fd::FromRawFd;
    use std::sync::mpsc;
    use std::time::Instant;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_foundation::NSError;
    use objc2_virtualization::{VZVirtioSocketConnection, VZVirtioSocketDevice, VZVirtualMachine};

    let ptr = VMS
        .lock()
        .expect("vms")
        .get(&id)
        .copied()
        .ok_or_else(|| "sandbox is not running".to_string())?;

    let (tx, rx) = mpsc::channel::<Result<i32, String>>();
    dispatch2::DispatchQueue::main().exec_async(move || {
        let vm = unsafe { &*(ptr as *const VZVirtualMachine) };
        let sockets = unsafe { vm.socketDevices() };
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
                let conn = unsafe { &*connection };
                let fd = unsafe { conn.fileDescriptor() };
                let duped = unsafe { libc::dup(fd) };
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

    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        match rx.try_recv() {
            Ok(Ok(fd)) => {
                let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
                return Ok(stream);
            }
            Ok(Err(e)) => return Err(e),
            Err(mpsc::TryRecvError::Empty) if Instant::now() < deadline => continue,
            Err(mpsc::TryRecvError::Empty) => return Err("vsock connect timed out".into()),
            Err(e) => return Err(format!("channel: {e}")),
        }
    }
}

#[cfg(target_os = "macos")]
fn nsurl(path: &str) -> objc2::rc::Retained<objc2_foundation::NSURL> {
    use objc2_foundation::{NSString, NSURL};
    NSURL::fileURLWithPath(&NSString::from_str(path))
}

#[cfg(target_os = "macos")]
fn path_str(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("non-utf8 path: {}", path.display()))
}

#[cfg(target_os = "macos")]
static VMS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<Uuid, usize>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[cfg(target_os = "macos")]
static NETWORKS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<Uuid, usize>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[cfg(test)]
mod tests {
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
}
