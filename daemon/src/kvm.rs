//! Linux KVM (qemu). Implements `vmm::Engine`. Same guest disks and
//! vsock agent as macOS VF.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{LazyLock, Mutex};

use uuid::Uuid;

use crate::runtime::Runtime;
use crate::sandbox::Limits;
use crate::vmm::Engine;

pub struct KvmEngine;

impl Engine for KvmEngine {
    fn boot(
        &self,
        runtime: &Runtime,
        id: Uuid,
        sandbox_dir: &Path,
        limits: Limits,
        _mac_id: Uuid,
    ) -> Result<(), String> {
        start(runtime, id, sandbox_dir, limits)
    }

    fn restore(
        &self,
        _runtime: &Runtime,
        _id: Uuid,
        _sandbox_dir: &Path,
        _limits: Limits,
        _mac_id: Uuid,
        _save: &Path,
    ) -> Result<(), String> {
        Err("KVM restore is not implemented".into())
    }

    fn pause(&self, _id: Uuid) -> Result<(), String> {
        Err("KVM pause is not implemented".into())
    }

    fn save(&self, _id: Uuid, _save: &Path) -> Result<(), String> {
        Err("KVM save is not implemented".into())
    }

    fn stop(&self, id: Uuid) -> Result<(), String> {
        stop(id)
    }

    fn vsock(&self, id: Uuid, port: u32) -> Result<std::os::unix::net::UnixStream, String> {
        vsock(id, port)
    }
}

pub fn is_supported() -> bool {
    which(qemu_bin()).is_some()
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let p = dir.join(bin);
            p.is_file().then_some(p)
        })
    })
}

pub fn cid(id: Uuid) -> u32 {
    let b = id.as_bytes();
    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    3 + (n % 0x7fff_ff00)
}

pub fn qemu_bin() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "qemu-system-aarch64"
    } else {
        "qemu-system-x86_64"
    }
}

pub fn qemu_args(
    runtime: &Runtime,
    id: Uuid,
    root: &Path,
    console: &Path,
    limits: Limits,
    kvm: bool,
) -> Result<Vec<String>, String> {
    let kernel = runtime.kernel.to_str().ok_or("kernel path")?.to_string();
    let initrd = runtime.initrd.to_str().ok_or("initrd path")?.to_string();
    let disk = root.to_str().ok_or("disk path")?.to_string();
    let cons = console.to_str().ok_or("console path")?.to_string();
    let accel = if kvm { "kvm" } else { "tcg" };
    let mut args = Vec::new();
    if cfg!(target_arch = "aarch64") {
        args.extend([
            "-machine".into(),
            format!("virt,gic-version=3,accel={accel}"),
        ]);
    } else {
        args.extend(["-machine".into(), format!("q35,accel={accel}")]);
    }
    if kvm {
        args.extend(["-cpu".into(), "host".into()]);
    } else {
        args.extend(["-cpu".into(), "max".into()]);
    }
    args.extend([
        "-smp".into(),
        limits.cpu.to_string(),
        "-m".into(),
        format!("{}", limits.ram / (1024 * 1024)),
        "-display".into(),
        "none".into(),
        "-no-reboot".into(),
        "-kernel".into(),
        kernel,
        "-initrd".into(),
        initrd,
        "-append".into(),
        runtime.boot_cmdline(),
        "-drive".into(),
        format!("file={disk},if=virtio,format=raw"),
        "-netdev".into(),
        "user,id=n0".into(),
        "-device".into(),
        "virtio-net-pci,netdev=n0".into(),
        "-device".into(),
        format!("vhost-vsock-pci,guest-cid={}", cid(id)),
        "-chardev".into(),
        format!("file,id=cons,path={cons}"),
        "-device".into(),
        "virtio-serial-pci".into(),
        "-device".into(),
        "virtconsole,chardev=cons".into(),
    ]);
    Ok(args)
}

static VMS: LazyLock<Mutex<HashMap<Uuid, Child>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn start(runtime: &Runtime, id: Uuid, sandbox_dir: &Path, limits: Limits) -> Result<(), String> {
    if which(qemu_bin()).is_none() {
        return Err(format!("{} not on PATH", qemu_bin()));
    }
    let root = sandbox_dir.join("disk").join("root.raw");
    if !root.is_file() {
        return Err("guest disk missing".into());
    }
    if !Path::new("/dev/vhost-vsock").exists() {
        return Err("vhost-vsock missing".into());
    }
    let log = sandbox_dir.join("console.log");
    let kvm = Path::new("/dev/kvm").exists();
    let args = qemu_args(runtime, id, &root, &log, limits, kvm)?;
    let qemu_log_path = sandbox_dir.join("qemu.log");
    let mut qemu_log = File::create(&qemu_log_path).map_err(|e| format!("qemu log: {e}"))?;
    writeln!(qemu_log, "{} {}", qemu_bin(), args.join(" ")).ok();
    qemu_log.flush().ok();
    let stdout = qemu_log.try_clone().map_err(|e| format!("qemu log: {e}"))?;
    let child = Command::new(qemu_bin())
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(qemu_log))
        .spawn()
        .map_err(|e| format!("qemu: {e}"))?;
    VMS.lock().expect("vms").insert(id, child);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let mut map = VMS.lock().expect("vms");
    if let Some(child) = map.get_mut(&id) {
        match child.try_wait() {
            Ok(Some(status)) => {
                map.remove(&id);
                return Err(format!(
                    "qemu exited {status} (see {})",
                    qemu_log_path.display()
                ));
            }
            Ok(None) => {}
            Err(e) => return Err(format!("qemu: {e}")),
        }
    }
    Ok(())
}

fn stop(id: Uuid) -> Result<(), String> {
    let Some(mut child) = VMS.lock().expect("vms").remove(&id) else {
        return Ok(());
    };
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

#[cfg(target_os = "linux")]
fn vsock(id: Uuid, port: u32) -> Result<std::os::unix::net::UnixStream, String> {
    vsock_connect(cid(id), port)
}

#[cfg(not(target_os = "linux"))]
fn vsock(_id: Uuid, _port: u32) -> Result<std::os::unix::net::UnixStream, String> {
    Err("KVM vsock is Linux-only".into())
}

#[cfg(target_os = "linux")]
fn vsock_connect(cid: u32, port: u32) -> Result<std::os::unix::net::UnixStream, String> {
    use std::os::unix::io::FromRawFd;
    let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err("vsock socket".into());
    }
    let addr = libc::sockaddr_vm {
        svm_family: libc::AF_VSOCK as libc::sa_family_t,
        svm_reserved1: 0,
        svm_port: port,
        svm_cid: cid,
        svm_zero: [0; 4],
    };
    let rc = unsafe {
        libc::connect(
            fd,
            std::ptr::addr_of!(addr).cast(),
            std::mem::size_of::<libc::sockaddr_vm>() as u32,
        )
    };
    if rc != 0 {
        unsafe { libc::close(fd) };
        return Err("vsock connect".into());
    }
    Ok(unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> Runtime {
        Runtime {
            kernel: PathBuf::from("/k"),
            initrd: PathBuf::from("/i"),
            rootfs: PathBuf::from("/r"),
            cmdline: "console=hvc0".into(),
        }
    }

    #[test]
    fn cid_avoids_reserved() {
        let c = cid(Uuid::nil());
        assert!(c >= 3);
    }

    #[test]
    fn is_supported_does_not_panic() {
        let _ = is_supported();
    }

    #[test]
    fn qemu_args_enable_kvm_and_vsock() {
        let args = qemu_args(
            &runtime(),
            Uuid::nil(),
            Path::new("/disk.raw"),
            Path::new("/console.log"),
            Limits::default(),
            true,
        )
        .unwrap();
        assert!(args.iter().any(|a| a.contains("accel=kvm")));
        assert!(args.iter().any(|a| a.contains("vhost-vsock")));
        assert!(args.iter().any(|a| a.contains("/disk.raw")));
        assert!(args.iter().any(|a| a.contains("virtconsole")));
        assert!(args.iter().any(|a| a == "-no-reboot"));
    }

    #[test]
    fn engine_save_restore_are_not_implemented() {
        let e = KvmEngine;
        let rt = runtime();
        let dir = Path::new("/tmp");
        let id = Uuid::nil();
        assert!(e.restore(&rt, id, dir, Limits::default(), id, dir).is_err());
        assert!(e.pause(id).is_err());
        assert!(e.save(id, dir).is_err());
    }

    #[test]
    fn qemu_args_tcg_when_kvm_is_missing() {
        let args = qemu_args(
            &runtime(),
            Uuid::nil(),
            Path::new("/disk.raw"),
            Path::new("/console.log"),
            Limits::default(),
            false,
        )
        .unwrap();
        assert!(args.iter().any(|a| a.contains("accel=tcg")));
        assert!(args.iter().any(|a| a == "max"));
        assert!(!args.iter().any(|a| a.contains("accel=kvm")));
    }
}
