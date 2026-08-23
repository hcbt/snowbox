//! Virtualization.framework, in-process, macOS only.

use std::path::Path;
use uuid::Uuid;

use crate::runtime::Runtime;

pub const AGENT_PORT: u32 = 52;
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

    pub fn start(&self, id: Uuid, sandbox_dir: &Path) -> Result<(), String> {
        start_vm(&self.runtime, id, sandbox_dir)
    }

    pub fn stop(&self, id: Uuid) -> Result<(), String> {
        stop_vm(id)
    }

    pub fn vsock(&self, id: Uuid, port: u32) -> Result<std::os::unix::net::UnixStream, String> {
        vsock_connect(id, port)
    }
}

#[cfg(target_os = "macos")]
fn start_vm(runtime: &Runtime, id: Uuid, sandbox_dir: &Path) -> Result<(), String> {
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

    let disk_dir = sandbox_dir.join("disk");
    std::fs::create_dir_all(&disk_dir).map_err(|e| format!("mkdir disk: {e}"))?;
    let root = disk_dir.join("root.raw");
    if !root.exists() {
        std::fs::copy(&runtime.rootfs, &root).map_err(|e| format!("copy rootfs: {e}"))?;
        let mut perms = std::fs::metadata(&root)
            .map_err(|e| format!("stat rootfs: {e}"))?
            .permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&root, perms).map_err(|e| format!("chmod rootfs: {e}"))?;
    }

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
        config.setCPUCount(2);
        config.setMemorySize(2 * 1024 * 1024 * 1024);

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
        net.setAttachment(Some(&VZNATNetworkDeviceAttachment::new()));
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
fn start_vm(_runtime: &Runtime, _id: Uuid, _sandbox_dir: &Path) -> Result<(), String> {
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
}
