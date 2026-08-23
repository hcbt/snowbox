//! Virtualization.framework, in-process, macOS only.

use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::runtime::Runtime;
use crate::sandbox::Limits;

pub const AGENT_PORT: u32 = 52;
pub const SHELL_PORT: u32 = 53;

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

#[cfg(not(target_os = "macos"))]
pub fn is_supported() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn pump_main_run_loop() {
    objc2_foundation::NSRunLoop::mainRunLoop().run();
}

#[cfg(not(target_os = "macos"))]
pub fn pump_main_run_loop() {
    std::thread::park();
}

pub struct Hypervisor {
    runtime: Runtime,
}

impl Hypervisor {
    pub fn new(runtime: Runtime) -> Self {
        Self { runtime }
    }

    pub fn start(&self, id: Uuid, sandbox_dir: &Path, limits: Limits) -> Result<(), String> {
        start_vm(&self.runtime, id, sandbox_dir, limits)
    }

    pub fn stop(&self, id: Uuid) -> Result<(), String> {
        stop_vm(id)
    }

    pub fn vsock(&self, id: Uuid, port: u32) -> Result<std::os::unix::net::UnixStream, String> {
        vsock_connect(id, port)
    }
}

pub fn prepare_root_disk(
    sandbox_dir: &Path,
    runtime_rootfs: &Path,
    disk: u64,
) -> Result<PathBuf, String> {
    let disk_dir = sandbox_dir.join("disk");
    std::fs::create_dir_all(&disk_dir).map_err(|e| format!("mkdir disk: {e}"))?;
    let root = disk_dir.join("root.raw");
    if !root.exists() {
        // Runtime lives on the Nix volume; Sandbox disks live on the data
        // volume. clonefile cannot cross volumes, so copy onto this volume
        // once and clone from there.
        let template = sandbox_dir
            .parent()
            .unwrap_or(sandbox_dir)
            .join(".runtime-root.raw");
        ensure_runtime_template(runtime_rootfs, &template)?;
        clone_or_copy(&template, &root)?;
        let mut perms = std::fs::metadata(&root)
            .map_err(|e| format!("stat rootfs: {e}"))?
            .permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&root, perms).map_err(|e| format!("chmod rootfs: {e}"))?;
    }
    let len = std::fs::metadata(&root)
        .map_err(|e| format!("stat disk: {e}"))?
        .len();
    if len > disk {
        return Err(format!(
            "disk image ({len} bytes) exceeds Limit ({disk} bytes)"
        ));
    }
    if len < disk {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&root)
            .map_err(|e| format!("open disk: {e}"))?;
        file.set_len(disk).map_err(|e| format!("grow disk: {e}"))?;
    }
    Ok(root)
}

fn ensure_runtime_template(src: &Path, template: &Path) -> Result<(), String> {
    let key = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
    let key_path = template.with_extension("src");
    let same = template.is_file()
        && std::fs::read_to_string(&key_path).ok().as_deref()
            == Some(key.to_string_lossy().as_ref());
    if same {
        return Ok(());
    }
    if let Some(parent) = template.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir template: {e}"))?;
    }
    let tmp = template.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    std::fs::copy(src, &tmp).map_err(|e| format!("copy rootfs template: {e}"))?;
    std::fs::rename(&tmp, template).map_err(|e| format!("install rootfs template: {e}"))?;
    std::fs::write(&key_path, key.to_string_lossy().as_bytes())
        .map_err(|e| format!("write rootfs template key: {e}"))?;
    Ok(())
}

fn clone_or_copy(src: &Path, dst: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if clonefile(src, dst) {
            return Ok(());
        }
    }
    std::fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| format!("copy rootfs: {e}"))
}

#[cfg(target_os = "macos")]
fn clonefile(src: &Path, dst: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(src_c) = std::ffi::CString::new(src.as_os_str().as_bytes()) else {
        return false;
    };
    let Ok(dst_c) = std::ffi::CString::new(dst.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) == 0 }
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

        let attachment: Option<Retained<VZVmnetNetworkDeviceAttachment>> = objc2::msg_send![
            VZVmnetNetworkDeviceAttachment::alloc(),
            initWithNetwork: network
        ];
        let Some(attachment) = attachment else {
            CFRelease(network.cast());
            return Err("isolated network attachment failed".into());
        };
        let mac = VZMACAddress::initWithString(
            VZMACAddress::alloc(),
            &NSString::from_str(&mac_address(id)),
        )
        .ok_or_else(|| format!("invalid MAC {}", mac_address(id)))?;
        net.setMACAddress(&mac);
        net.setAttachment(Some(attachment.as_super()));
        CFRelease(network.cast());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn start_vm(runtime: &Runtime, id: Uuid, sandbox_dir: &Path, limits: Limits) -> Result<(), String> {
    use std::os::fd::IntoRawFd;
    use std::sync::mpsc;
    use std::time::Instant;

    use block2::RcBlock;
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_foundation::{NSArray, NSError, NSFileHandle, NSString};
    use objc2_virtualization::*;

    if !is_supported() {
        return Err("virtualization is not supported on this Host".into());
    }

    let root = prepare_root_disk(sandbox_dir, &runtime.rootfs, limits.disk)?;

    let kernel = path_str(&runtime.kernel)?;
    let initrd = path_str(&runtime.initrd)?;
    let root_str = path_str(&root)?;

    unsafe {
        let platform =
            VZGenericPlatformConfiguration::init(VZGenericPlatformConfiguration::alloc());

        let boot_loader =
            VZLinuxBootLoader::initWithKernelURL(VZLinuxBootLoader::alloc(), &nsurl(kernel));
        boot_loader.setInitialRamdiskURL(Some(&nsurl(initrd)));
        boot_loader.setCommandLine(&NSString::from_str(&runtime.cmdline));

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
        attach_isolated_network(&net, id)?;
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

        let console_log = sandbox_dir.join("console.log");
        let console_file =
            std::fs::File::create(&console_log).map_err(|e| format!("console log: {e}"))?;
        let write_handle =
            NSFileHandle::initWithFileDescriptor(NSFileHandle::alloc(), console_file.into_raw_fd());
        let read_handle = {
            let devnull = std::fs::File::open("/dev/null").map_err(|e| e.to_string())?;
            NSFileHandle::initWithFileDescriptor(NSFileHandle::alloc(), devnull.into_raw_fd())
        };
        let serial = VZVirtioConsoleDeviceSerialPortConfiguration::new();
        let attachment =
            VZFileHandleSerialPortAttachment::initWithFileHandleForReading_fileHandleForWriting(
                VZFileHandleSerialPortAttachment::alloc(),
                Some(&read_handle),
                Some(&write_handle),
            );
        serial.setAttachment(Some(&attachment));
        config.setSerialPorts(&NSArray::from_retained_slice(&[Retained::into_super(
            serial,
        )]));

        config
            .validateWithError()
            .map_err(|e| format!("invalid vm: {e}"))?;

        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        let config_ptr = Retained::into_raw(config) as usize;
        let id_owned = id;

        dispatch2::DispatchQueue::main().exec_async(move || {
            let config = Retained::from_raw(config_ptr as *mut VZVirtualMachineConfiguration)
                .expect("config pointer");
            let vm = VZVirtualMachine::initWithConfiguration_queue(
                VZVirtualMachine::alloc(),
                &config,
                dispatch2::DispatchQueue::main(),
            );
            let tx_clone = tx.clone();
            let handler = RcBlock::new(move |error: *mut NSError| {
                if error.is_null() {
                    let _ = tx_clone.send(Ok(()));
                } else {
                    let e = &*error;
                    let _ = tx_clone.send(Err(format!("{}", e.localizedDescription())));
                }
            });
            vm.startWithCompletionHandler(&handler);
            let vm_ptr = Retained::into_raw(vm) as usize;
            VMS.lock().expect("vms").insert(id_owned, vm_ptr);
        });

        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
            match rx.try_recv() {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(e)) => {
                    let _ = stop_vm(id);
                    return Err(e);
                }
                Err(mpsc::TryRecvError::Empty) if Instant::now() < deadline => continue,
                Err(mpsc::TryRecvError::Empty) => {
                    let _ = stop_vm(id);
                    return Err("start timed out".into());
                }
                Err(e) => return Err(format!("channel: {e}")),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn start_vm(
    _runtime: &Runtime,
    _id: Uuid,
    _sandbox_dir: &Path,
    _limits: Limits,
) -> Result<(), String> {
    Err("Virtualization.framework is macOS-only".into())
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

#[cfg(not(target_os = "macos"))]
fn stop_vm(_id: Uuid) -> Result<(), String> {
    Ok(())
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

#[cfg(not(target_os = "macos"))]
fn vsock_connect(_id: Uuid, _port: u32) -> Result<std::os::unix::net::UnixStream, String> {
    Err("Virtualization.framework is macOS-only".into())
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

    #[test]
    fn prepare_root_disk_copies_and_grows() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("runtime.raw");
        std::fs::write(&src, vec![0u8; 64]).unwrap();
        let sandbox = dir.path().join("sb");
        let root = super::prepare_root_disk(&sandbox, &src, 256).unwrap();
        assert_eq!(std::fs::metadata(&root).unwrap().len(), 256);
        assert_eq!(&std::fs::read(&root).unwrap()[..64], &[0u8; 64]);

        std::fs::write(&src, vec![1u8; 64]).unwrap();
        super::prepare_root_disk(&sandbox, &src, 256).unwrap();
        assert_eq!(&std::fs::read(&root).unwrap()[..64], &[0u8; 64]);
    }

    #[test]
    fn prepare_root_disk_clones_from_a_same_volume_template() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("runtime.raw");
        std::fs::write(&src, b"rootfs").unwrap();
        let a = super::prepare_root_disk(&dir.path().join("a"), &src, 64).unwrap();
        let b = super::prepare_root_disk(&dir.path().join("b"), &src, 64).unwrap();
        assert_eq!(std::fs::read(&a).unwrap()[..6], *b"rootfs");
        assert_eq!(std::fs::read(&b).unwrap()[..6], *b"rootfs");
        assert!(dir.path().join(".runtime-root.raw").is_file());
    }

    #[test]
    fn prepare_root_disk_refuses_when_image_exceeds_limit() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("runtime.raw");
        std::fs::write(&src, vec![0u8; 200]).unwrap();
        let err = super::prepare_root_disk(&dir.path().join("sb"), &src, 100).unwrap_err();
        assert!(err.contains("exceeds"));
    }
}
