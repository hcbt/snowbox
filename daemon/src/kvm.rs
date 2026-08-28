//! Linux KVM (qemu). Implements `vmm::Engine`. Same guest disks and
//! vsock agent as macOS VF.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

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
        start(runtime, id, sandbox_dir, limits, None)
    }

    fn restore(
        &self,
        runtime: &Runtime,
        id: Uuid,
        sandbox_dir: &Path,
        limits: Limits,
        _mac_id: Uuid,
        save: &Path,
    ) -> Result<(), String> {
        if !save.is_file() {
            return Err("no machine state".into());
        }
        start(runtime, id, sandbox_dir, limits, Some(save))
    }

    fn pause(&self, id: Uuid) -> Result<(), String> {
        qmp_cmd(id, "stop", None).map(|_| ())
    }

    fn resume(&self, id: Uuid) -> Result<(), String> {
        qmp_cmd(id, "cont", None).map(|_| ())
    }

    fn save(&self, id: Uuid, save: &Path) -> Result<(), String> {
        if let Some(parent) = save.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir save: {e}"))?;
        }
        let _ = std::fs::remove_file(save);
        qmp_migrate(id, save)
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

/// Preferred vsock CID from the Sandbox id. Not unique on its own.
pub fn preferred_cid(id: Uuid) -> u32 {
    let b = id.as_bytes();
    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    3 + (n % 0x7fff_ff00)
}

pub fn cid(id: Uuid) -> u32 {
    preferred_cid(id)
}

/// Pick a guest CID that is not already in `used`. Prefers [`preferred_cid`].
pub fn next_unique_cid(id: Uuid, used: &HashSet<u32>) -> Result<u32, String> {
    let preferred = preferred_cid(id);
    let min = 3u32;
    let span = 0x7fff_ff00u32;
    for i in 0..=span {
        let cid = min + ((preferred - min).wrapping_add(i) % span);
        if cid >= 3 && !used.contains(&cid) {
            return Ok(cid);
        }
    }
    Err("vsock CID space exhausted".into())
}

pub fn qemu_bin() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "qemu-system-aarch64"
    } else {
        "qemu-system-x86_64"
    }
}

pub fn migrate_uri(save: &Path) -> String {
    format!("file:{}", save.display())
}

pub fn migrate_command(save: &Path) -> serde_json::Value {
    serde_json::json!({
        "execute": "migrate",
        "arguments": { "uri": migrate_uri(save) }
    })
}

pub fn qemu_args(
    runtime: &Runtime,
    cid: u32,
    root: &Path,
    console: &Path,
    limits: Limits,
    kvm: bool,
    incoming: Option<&Path>,
    qmp: Option<&Path>,
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
        format!("vhost-vsock-pci,guest-cid={cid}"),
        "-chardev".into(),
        format!("file,id=cons,path={cons}"),
        "-device".into(),
        "virtio-serial-pci".into(),
        "-device".into(),
        "virtconsole,chardev=cons".into(),
    ]);
    if let Some(qmp) = qmp {
        args.extend([
            "-qmp".into(),
            format!("unix:{},server,nowait", qmp.display()),
        ]);
    }
    if let Some(incoming) = incoming {
        args.extend(["-incoming".into(), migrate_uri(incoming)]);
    }
    Ok(args)
}

struct LiveKvm {
    child: Child,
    qmp: PathBuf,
    cid: u32,
}

static VMS: LazyLock<Mutex<HashMap<Uuid, LiveKvm>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static USED_CIDS: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn cid_path(sandbox_dir: &Path) -> PathBuf {
    sandbox_dir.join("vsock.cid")
}

fn load_persisted_cid(sandbox_dir: &Path) -> Option<u32> {
    std::fs::read_to_string(cid_path(sandbox_dir))
        .ok()?
        .trim()
        .parse()
        .ok()
        .filter(|c| *c >= 3)
}

fn persist_cid(sandbox_dir: &Path, cid: u32) {
    let _ = std::fs::write(cid_path(sandbox_dir), cid.to_string());
}

fn allocate_cid(id: Uuid, sandbox_dir: &Path) -> Result<u32, String> {
    let mut used = USED_CIDS.lock().expect("cids");
    if let Some(cid) = load_persisted_cid(sandbox_dir)
        && !used.contains(&cid)
    {
        used.insert(cid);
        return Ok(cid);
    }
    let cid = next_unique_cid(id, &used)?;
    used.insert(cid);
    persist_cid(sandbox_dir, cid);
    Ok(cid)
}

fn release_cid(cid: u32) {
    USED_CIDS.lock().expect("cids").remove(&cid);
}

fn qmp_path(sandbox_dir: &Path) -> PathBuf {
    sandbox_dir.join("qmp.sock")
}

fn start(
    runtime: &Runtime,
    id: Uuid,
    sandbox_dir: &Path,
    limits: Limits,
    incoming: Option<&Path>,
) -> Result<(), String> {
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
    let cid = allocate_cid(id, sandbox_dir)?;
    let qmp = qmp_path(sandbox_dir);
    let _ = std::fs::remove_file(&qmp);
    let args = qemu_args(runtime, cid, &root, &log, limits, kvm, incoming, Some(&qmp))?;
    let qemu_log_path = sandbox_dir.join("qemu.log");
    let mut qemu_log = File::create(&qemu_log_path).map_err(|e| format!("qemu log: {e}"))?;
    writeln!(qemu_log, "{} {}", qemu_bin(), args.join(" ")).ok();
    qemu_log.flush().ok();
    let stdout = qemu_log.try_clone().map_err(|e| format!("qemu log: {e}"))?;
    let child = match Command::new(qemu_bin())
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(qemu_log))
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            release_cid(cid);
            return Err(format!("qemu: {e}"));
        }
    };
    VMS.lock().expect("vms").insert(
        id,
        LiveKvm {
            child,
            qmp: qmp.clone(),
            cid,
        },
    );
    std::thread::sleep(Duration::from_millis(150));
    {
        let mut map = VMS.lock().expect("vms");
        if let Some(live) = map.get_mut(&id) {
            match live.child.try_wait() {
                Ok(Some(status)) => {
                    let cid = live.cid;
                    map.remove(&id);
                    release_cid(cid);
                    return Err(format!(
                        "qemu exited {status} (see {})",
                        qemu_log_path.display()
                    ));
                }
                Ok(None) => {}
                Err(e) => return Err(format!("qemu: {e}")),
            }
        }
    }
    if let Err(e) = wait_qmp_socket(&qmp, id) {
        let _ = stop(id);
        return Err(e);
    }
    if let Some(save) = incoming {
        if let Err(e) = qmp_wait_status(id, &["paused", "prelaunch"], Duration::from_secs(120)) {
            let _ = stop(id);
            return Err(e);
        }
        if let Err(e) = qmp_cmd(id, "cont", None) {
            let _ = stop(id);
            return Err(e);
        }
        let _ = std::fs::remove_file(save);
    }
    Ok(())
}

fn wait_qmp_socket(path: &Path, id: Uuid) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if path.exists() {
            return Ok(());
        }
        {
            let mut map = VMS.lock().expect("vms");
            if let Some(live) = map.get_mut(&id)
                && let Ok(Some(status)) = live.child.try_wait()
            {
                let cid = live.cid;
                map.remove(&id);
                release_cid(cid);
                return Err(format!("qemu exited {status} before qmp"));
            }
        }
        if Instant::now() >= deadline {
            return Err("qmp socket missing".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn stop(id: Uuid) -> Result<(), String> {
    let Some(mut live) = VMS.lock().expect("vms").remove(&id) else {
        return Ok(());
    };
    if let Ok(mut qmp) = Qmp::connect(&live.qmp) {
        let _ = qmp.exec("quit", None);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match live.child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = live.child.kill();
                let _ = live.child.wait();
                break;
            }
        }
    }
    let _ = std::fs::remove_file(&live.qmp);
    release_cid(live.cid);
    Ok(())
}

struct Qmp {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Qmp {
    fn connect(path: &Path) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("qmp: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(15))).ok();
        let writer = stream.try_clone().map_err(|e| format!("qmp: {e}"))?;
        let mut reader = BufReader::new(stream);
        let mut greeting = String::new();
        reader
            .read_line(&mut greeting)
            .map_err(|e| format!("qmp greeting: {e}"))?;
        let mut qmp = Self { reader, writer };
        qmp.exec("qmp_capabilities", None)?;
        Ok(qmp)
    }

    fn exec(
        &mut self,
        cmd: &str,
        args: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let mut obj = serde_json::json!({"execute": cmd});
        if let Some(args) = args {
            obj["arguments"] = args;
        }
        writeln!(self.writer, "{obj}").map_err(|e| format!("qmp write: {e}"))?;
        loop {
            let mut line = String::new();
            self.reader
                .read_line(&mut line)
                .map_err(|e| format!("qmp read: {e}"))?;
            if line.is_empty() {
                return Err("qmp eof".into());
            }
            let v: serde_json::Value =
                serde_json::from_str(&line).map_err(|e| format!("qmp json: {e}"))?;
            if v.get("event").is_some() {
                continue;
            }
            if let Some(err) = v.get("error") {
                return Err(format!("qmp {cmd}: {err}"));
            }
            return Ok(v.get("return").cloned().unwrap_or(v));
        }
    }
}

fn qmp_for(id: Uuid) -> Result<Qmp, String> {
    let path = VMS
        .lock()
        .expect("vms")
        .get(&id)
        .map(|l| l.qmp.clone())
        .ok_or_else(|| "sandbox is not running".to_string())?;
    Qmp::connect(&path)
}

fn qmp_cmd(
    id: Uuid,
    cmd: &str,
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    qmp_for(id)?.exec(cmd, args)
}

fn qmp_wait_status(id: Uuid, want: &[&str], timeout: Duration) -> Result<String, String> {
    let mut qmp = qmp_for(id)?;
    let deadline = Instant::now() + timeout;
    loop {
        let st = qmp.exec("query-status", None)?;
        let status = st.get("status").and_then(|s| s.as_str()).unwrap_or("");
        if want.iter().any(|w| *w == status) {
            return Ok(status.to_string());
        }
        if Instant::now() >= deadline {
            return Err(format!("qmp status {status}"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn qmp_migrate(id: Uuid, save: &Path) -> Result<(), String> {
    let mut qmp = qmp_for(id)?;
    let uri = migrate_uri(save);
    qmp.exec("migrate", Some(serde_json::json!({"uri": uri})))?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let st = qmp.exec("query-migrate", None)?;
        let status = st
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");
        match status {
            "completed" => return Ok(()),
            "failed" | "cancelled" => {
                return Err(format!("migrate {status}"));
            }
            _ if Instant::now() >= deadline => {
                return Err("migrate timed out".into());
            }
            _ => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[cfg(target_os = "linux")]
fn vsock(id: Uuid, port: u32) -> Result<std::os::unix::net::UnixStream, String> {
    let cid = VMS
        .lock()
        .expect("vms")
        .get(&id)
        .map(|l| l.cid)
        .unwrap_or_else(|| cid(id));
    vsock_connect(cid, port)
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

    fn args(incoming: Option<&Path>, qmp: Option<&Path>, kvm: bool) -> Vec<String> {
        qemu_args(
            &runtime(),
            cid(Uuid::nil()),
            Path::new("/disk.raw"),
            Path::new("/console.log"),
            Limits::default(),
            kvm,
            incoming,
            qmp,
        )
        .unwrap()
    }

    #[test]
    fn cid_avoids_reserved() {
        let c = cid(Uuid::nil());
        assert!(c >= 3);
    }

    #[test]
    fn cids_are_unique_for_many_uuids() {
        let mut used = HashSet::new();
        let mut seen = HashSet::new();
        for _ in 0..10_000 {
            let id = Uuid::new_v4();
            let c = next_unique_cid(id, &used).unwrap();
            used.insert(c);
            assert!(seen.insert(c), "duplicate CID {c}");
            assert!(c >= 3);
        }
    }

    #[test]
    fn colliding_preferred_cids_are_disambiguated() {
        let a = Uuid::from_bytes([1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let b = Uuid::from_bytes([1, 2, 3, 4, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(preferred_cid(a), preferred_cid(b));
        let mut used = HashSet::new();
        let ca = next_unique_cid(a, &used).unwrap();
        used.insert(ca);
        let cb = next_unique_cid(b, &used).unwrap();
        assert_ne!(ca, cb);
    }

    #[test]
    fn is_supported_does_not_panic() {
        let _ = is_supported();
    }

    #[test]
    fn qemu_args_enable_kvm_and_vsock() {
        let args = args(None, None, true);
        assert!(args.iter().any(|a| a.contains("accel=kvm")));
        assert!(args.iter().any(|a| a.contains("vhost-vsock")));
        assert!(args.iter().any(|a| a.contains("/disk.raw")));
        assert!(args.iter().any(|a| a.contains("virtconsole")));
        assert!(args.iter().any(|a| a == "-no-reboot"));
    }

    #[test]
    fn qemu_args_tcg_when_kvm_is_missing() {
        let args = args(None, None, false);
        assert!(args.iter().any(|a| a.contains("accel=tcg")));
        assert!(args.iter().any(|a| a == "max"));
        assert!(!args.iter().any(|a| a.contains("accel=kvm")));
    }

    #[test]
    fn restore_args_include_the_save_path() {
        let save = Path::new("/var/sb/machine.vzvmsave");
        let qmp = Path::new("/var/sb/qmp.sock");
        let args = args(Some(save), Some(qmp), true);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-incoming" && w[1].contains("machine.vzvmsave"))
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-qmp" && w[1].contains("qmp.sock"))
        );
    }

    #[test]
    fn save_builds_a_migrate_command_with_the_save_path() {
        let save = Path::new("/var/sb/machine.vzvmsave");
        let cmd = migrate_command(save);
        assert_eq!(cmd["execute"], "migrate");
        assert!(
            cmd["arguments"]["uri"]
                .as_str()
                .unwrap()
                .contains("machine.vzvmsave")
        );
        assert!(migrate_uri(save).starts_with("file:"));
    }

    #[test]
    fn engine_save_restore_are_implemented() {
        let e = KvmEngine;
        let rt = runtime();
        let dir = Path::new("/tmp");
        let id = Uuid::nil();
        let save_err = e.save(id, dir).unwrap_err();
        assert!(
            !save_err.to_lowercase().contains("not implemented"),
            "{save_err}"
        );
        let restore_err = e
            .restore(&rt, id, dir, Limits::default(), id, dir)
            .unwrap_err();
        assert!(
            !restore_err.to_lowercase().contains("not implemented"),
            "{restore_err}"
        );
        let pause_err = e.pause(id).unwrap_err();
        assert!(!pause_err.to_lowercase().contains("not implemented"));
    }
}
