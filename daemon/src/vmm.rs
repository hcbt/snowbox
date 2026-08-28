//! Hypervisor seam. macOS VF and Linux KVM implement `Engine`; the
//! rest of the Daemon talks only to `Hypervisor` and `Control`.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use crate::disk;
use crate::runtime::Runtime;
use crate::sandbox::Limits;

pub const AGENT_PORT: u32 = 52;
pub const SHELL_PORT: u32 = 53;
pub const SAVE_NAME: &str = "machine.vzvmsave";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartKind {
    Cold,
    Restored,
}

/// Per-OS virtual machine. Disk prep and Start/Stop policy live on
/// `Hypervisor` so the backends stay small.
pub trait Engine: Send + Sync {
    fn boot(
        &self,
        runtime: &Runtime,
        id: Uuid,
        sandbox_dir: &Path,
        limits: Limits,
        mac_id: Uuid,
    ) -> Result<(), String>;

    fn restore(
        &self,
        runtime: &Runtime,
        id: Uuid,
        sandbox_dir: &Path,
        limits: Limits,
        mac_id: Uuid,
        save: &Path,
    ) -> Result<(), String>;

    fn pause(&self, id: Uuid) -> Result<(), String>;
    fn resume(&self, id: Uuid) -> Result<(), String>;
    fn save(&self, id: Uuid, save: &Path) -> Result<(), String>;
    fn stop(&self, id: Uuid) -> Result<(), String>;
    fn vsock(&self, id: Uuid, port: u32) -> Result<UnixStream, String>;
}

/// Guest control plane. Agent, Window PTY, and Publish talk through this
/// and never to a hypervisor backend.
pub trait Control: Send + Sync {
    fn vsock(&self, id: Uuid, port: u32) -> Result<UnixStream, String>;
}

pub struct Hypervisor {
    runtime: Runtime,
    engine: Arc<dyn Engine>,
}

impl Hypervisor {
    pub fn wrap(runtime: Runtime, engine: Arc<dyn Engine>) -> Self {
        Self { runtime, engine }
    }

    pub fn start(&self, id: Uuid, sandbox_dir: &Path, limits: Limits) -> Result<StartKind, String> {
        let own_save = sandbox_dir.join(SAVE_NAME);
        let has_disk = sandbox_dir.join("disk").join("root.raw").is_file();
        if !own_save.exists() && !has_disk {
            install_ready(sandbox_dir, &self.runtime);
        }
        let mac_id = disk::read_mac_id(sandbox_dir, id);
        if own_save.exists() {
            disk::prepare_root_disk_for_restore(sandbox_dir, &self.runtime.rootfs, limits.disk)?;
            match self
                .engine
                .restore(&self.runtime, id, sandbox_dir, limits, mac_id, &own_save)
            {
                Ok(()) => {
                    disk::write_mac_id(sandbox_dir, mac_id);
                    disk::grow_root_disk(sandbox_dir, limits.disk)?;
                    return Ok(StartKind::Restored);
                }
                Err(e) => {
                    eprintln!("sandbox {id}: restore failed ({e}); booting");
                    let _ = std::fs::remove_file(&own_save);
                    // Drop the snapshot identifier so a concurrent guest
                    // already using it does not collide on cold boot.
                    let _ = std::fs::remove_file(sandbox_dir.join("machine.ident"));
                    let _ = self.engine.stop(id);
                }
            }
        }
        disk::prepare_root_disk(sandbox_dir, &self.runtime.rootfs, limits.disk)?;
        disk::write_mac_id(sandbox_dir, id);
        self.engine
            .boot(&self.runtime, id, sandbox_dir, limits, id)?;
        Ok(StartKind::Cold)
    }

    pub fn start_cold(&self, id: Uuid, sandbox_dir: &Path, limits: Limits) -> Result<(), String> {
        disk::prepare_root_disk(sandbox_dir, &self.runtime.rootfs, limits.disk)?;
        disk::write_mac_id(sandbox_dir, id);
        self.engine.boot(&self.runtime, id, sandbox_dir, limits, id)
    }

    pub fn save_and_stop(&self, id: Uuid, save: &Path) -> Result<(), String> {
        self.engine.pause(id)?;
        if let Err(e) = self.engine.save(id, save) {
            let _ = self.engine.stop(id);
            let _ = std::fs::remove_file(save);
            return Err(e);
        }
        self.engine.stop(id)?;
        Ok(())
    }

    pub fn stop(&self, id: Uuid) -> Result<(), String> {
        self.engine.stop(id)
    }

    /// Pause this Sandbox, save machine state, and clone disk + save +
    /// ident + MAC as the reusable ready image. Resume afterwards and
    /// drop the save next to this disk so later Stop writes a fresh one.
    /// Later New Sandboxes restore that clone instead of cold-booting.
    pub fn capture_ready(&self, id: Uuid, sandbox_dir: &Path) -> Result<(), String> {
        crate::ready::ensure(
            || snapshot_complete(&ready_dir(sandbox_dir, &self.runtime)),
            || self.capture_ready_once(id, sandbox_dir),
        );
        Ok(())
    }

    fn capture_ready_once(&self, id: Uuid, sandbox_dir: &Path) -> Result<(), String> {
        if snapshot_complete(&ready_dir(sandbox_dir, &self.runtime)) {
            return Ok(());
        }
        self.engine.pause(id)?;
        let save = sandbox_dir.join(SAVE_NAME);
        if let Err(e) = self.engine.save(id, &save) {
            let _ = std::fs::remove_file(&save);
            let _ = self.engine.resume(id);
            return Err(e);
        }
        bake_ready(sandbox_dir, &self.runtime);
        let _ = std::fs::remove_file(&save);
        self.engine.resume(id)?;
        if snapshot_complete(&ready_dir(sandbox_dir, &self.runtime)) {
            Ok(())
        } else {
            Err("ready snapshot: bake failed".into())
        }
    }

    pub fn vsock(&self, id: Uuid, port: u32) -> Result<UnixStream, String> {
        self.engine.vsock(id, port)
    }

    pub fn ready_snapshot_exists(&self, sandboxes_root: &Path) -> bool {
        snapshot_complete(&ready_dir(&sandboxes_root.join("_"), &self.runtime))
    }
}

impl Control for Hypervisor {
    fn vsock(&self, id: Uuid, port: u32) -> Result<UnixStream, String> {
        Hypervisor::vsock(self, id, port)
    }
}

impl<T: Control + ?Sized> Control for Arc<T> {
    fn vsock(&self, id: Uuid, port: u32) -> Result<UnixStream, String> {
        (**self).vsock(id, port)
    }
}

pub fn attach(runtime: Runtime, _data: PathBuf) -> Arc<Hypervisor> {
    #[cfg(target_os = "macos")]
    let engine: Arc<dyn Engine> = Arc::new(crate::vz::VzEngine);
    #[cfg(target_os = "linux")]
    let engine: Arc<dyn Engine> = Arc::new(crate::kvm::KvmEngine);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    compile_error!("snowbox Host is macOS or Linux");
    Arc::new(Hypervisor::wrap(runtime, engine))
}

pub fn is_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::vz::is_supported()
    }
    #[cfg(target_os = "linux")]
    {
        crate::kvm::is_supported()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

pub fn pump_main_run_loop() {
    #[cfg(target_os = "macos")]
    crate::vz::pump_main_run_loop();
    #[cfg(not(target_os = "macos"))]
    std::thread::park();
}

pub(crate) const HATCHED: &str = "hatched.ready";

fn ready_key(runtime: &Runtime) -> String {
    runtime.content_id()
}

fn ready_dir(sandbox_dir: &Path, runtime: &Runtime) -> PathBuf {
    sandbox_dir
        .parent()
        .unwrap_or(sandbox_dir)
        .join(".ready")
        .join(ready_key(runtime))
}

fn snapshot_complete(dir: &Path) -> bool {
    dir.join("root.raw").is_file()
        && dir.join("runtime.src").is_file()
        && dir.join(SAVE_NAME).is_file()
        && dir.join("machine.ident").is_file()
        && dir.join("mac.id").is_file()
}

fn snapshot_matches_runtime(dir: &Path, runtime: &Runtime) -> bool {
    let stamped = std::fs::read_to_string(dir.join("runtime.src")).unwrap_or_default();
    let stamped = stamped.trim();
    if stamped.is_empty() {
        return false;
    }
    if stamped == ready_key(runtime) {
        return true;
    }
    let current = runtime
        .rootfs
        .canonicalize()
        .unwrap_or_else(|_| runtime.rootfs.clone());
    stamped == current.to_string_lossy()
}

fn install_ready(sandbox_dir: &Path, runtime: &Runtime) {
    let ready = ready_dir(sandbox_dir, runtime);
    if !ready.join("root.raw").is_file() {
        eprintln!("ready snapshot: none");
        return;
    }
    if !snapshot_matches_runtime(&ready, runtime) {
        eprintln!("ready snapshot: stale userspace; dropping");
        let _ = std::fs::remove_dir_all(&ready);
        return;
    }
    let disk_dir = sandbox_dir.join("disk");
    let dst_disk = disk_dir.join("root.raw");
    if let Err(e) = std::fs::create_dir_all(&disk_dir) {
        eprintln!("ready snapshot: mkdir disk: {e}");
        return;
    }
    if !dst_disk.exists() {
        if let Err(e) = disk::clone_file(&ready.join("root.raw"), &dst_disk) {
            eprintln!("ready snapshot: clone disk: {e}");
            return;
        }
    }
    for name in [SAVE_NAME, "machine.ident", "mac.id", "runtime.src"] {
        let src = ready.join(name);
        if src.is_file() {
            let _ = std::fs::copy(&src, sandbox_dir.join(name));
        }
    }
    let _ = std::fs::write(sandbox_dir.join(HATCHED), b"");
}

fn bake_ready(sandbox_dir: &Path, runtime: &Runtime) {
    let dest = ready_dir(sandbox_dir, runtime);
    if snapshot_complete(&dest) {
        return;
    }
    let src_disk = sandbox_dir.join("disk").join("root.raw");
    if !src_disk.is_file() {
        return;
    }
    let current = runtime
        .rootfs
        .canonicalize()
        .unwrap_or_else(|_| runtime.rootfs.clone());
    let stamped = std::fs::read_to_string(sandbox_dir.join("runtime.src")).unwrap_or_default();
    if stamped.trim() != current.to_string_lossy() {
        eprintln!("ready snapshot: skip bake (disk is a different runtime)");
        return;
    }
    let tmp = dest.with_extension("tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(e) = std::fs::create_dir_all(&tmp) {
        eprintln!("ready snapshot: mkdir: {e}");
        return;
    }
    if let Err(e) = disk::clone_file(&src_disk, &tmp.join("root.raw")) {
        eprintln!("ready snapshot: clone disk: {e}");
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    }
    for name in [SAVE_NAME, "machine.ident", "mac.id", "runtime.src"] {
        let src = sandbox_dir.join(name);
        if src.is_file() {
            let _ = std::fs::copy(&src, tmp.join(name));
        }
    }
    if !snapshot_complete(&tmp) {
        eprintln!("ready snapshot: skip bake (missing save, ident, or MAC)");
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    }
    let _ = std::fs::remove_dir_all(&dest);
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        eprintln!("ready snapshot: install: {e}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        boots: Mutex<Vec<Uuid>>,
        restores: Mutex<Vec<Uuid>>,
        restore_disk_lens: Mutex<Vec<u64>>,
        restore_macs: Mutex<Vec<Uuid>>,
        pauses: Mutex<Vec<Uuid>>,
        resumes: Mutex<Vec<Uuid>>,
        saves: Mutex<Vec<Uuid>>,
        stops: Mutex<Vec<Uuid>>,
        restore_ok: bool,
        pause_ok: bool,
        resume_ok: bool,
        save_ok: bool,
    }

    impl Fake {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                boots: Mutex::new(Vec::new()),
                restores: Mutex::new(Vec::new()),
                restore_disk_lens: Mutex::new(Vec::new()),
                restore_macs: Mutex::new(Vec::new()),
                pauses: Mutex::new(Vec::new()),
                resumes: Mutex::new(Vec::new()),
                saves: Mutex::new(Vec::new()),
                stops: Mutex::new(Vec::new()),
                restore_ok: true,
                pause_ok: true,
                resume_ok: true,
                save_ok: true,
            })
        }
    }

    impl Engine for Fake {
        fn boot(
            &self,
            _runtime: &Runtime,
            id: Uuid,
            _sandbox_dir: &Path,
            _limits: Limits,
            _mac_id: Uuid,
        ) -> Result<(), String> {
            self.boots.lock().expect("boots").push(id);
            Ok(())
        }

        fn restore(
            &self,
            _runtime: &Runtime,
            id: Uuid,
            sandbox_dir: &Path,
            _limits: Limits,
            mac_id: Uuid,
            _save: &Path,
        ) -> Result<(), String> {
            self.restores.lock().expect("restores").push(id);
            self.restore_macs.lock().expect("restore macs").push(mac_id);
            let len = std::fs::metadata(sandbox_dir.join("disk").join("root.raw"))
                .map(|m| m.len())
                .unwrap_or(0);
            self.restore_disk_lens
                .lock()
                .expect("restore disk")
                .push(len);
            if self.restore_ok {
                Ok(())
            } else {
                Err("nope".into())
            }
        }

        fn pause(&self, id: Uuid) -> Result<(), String> {
            self.pauses.lock().expect("pauses").push(id);
            if self.pause_ok {
                Ok(())
            } else {
                Err("nope".into())
            }
        }

        fn resume(&self, id: Uuid) -> Result<(), String> {
            self.resumes.lock().expect("resumes").push(id);
            if self.resume_ok {
                Ok(())
            } else {
                Err("nope".into())
            }
        }

        fn save(&self, id: Uuid, save: &Path) -> Result<(), String> {
            self.saves.lock().expect("saves").push(id);
            if self.save_ok {
                if let Some(parent) = save.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(save, b"save").map_err(|e| e.to_string())?;
                Ok(())
            } else {
                Err("nope".into())
            }
        }

        fn stop(&self, id: Uuid) -> Result<(), String> {
            self.stops.lock().expect("stops").push(id);
            Ok(())
        }

        fn vsock(&self, _id: Uuid, _port: u32) -> Result<UnixStream, String> {
            Err("fake vsock".into())
        }
    }

    fn runtime(dir: &Path) -> Runtime {
        let rootfs = dir.join("runtime.raw");
        std::fs::write(&rootfs, vec![0u8; 64]).unwrap();
        Runtime {
            kernel: dir.join("k"),
            initrd: dir.join("i"),
            rootfs,
            cmdline: "console=hvc0".into(),
        }
    }

    fn limits() -> Limits {
        Limits {
            cpu: 1,
            ram: 512 * 1024 * 1024,
            disk: 256,
        }
    }

    fn stamp_runtime(dir: &Path, runtime: &Path) {
        let key = runtime
            .canonicalize()
            .unwrap_or_else(|_| runtime.to_path_buf());
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("runtime.src"), key.to_string_lossy().as_bytes()).unwrap();
    }

    fn primed_booted(root: &Path, rt: &Runtime) -> PathBuf {
        let src = root.join("src");
        crate::environment::write_default(&src).unwrap();
        std::fs::create_dir_all(src.join("disk")).unwrap();
        std::fs::write(src.join("disk").join("root.raw"), b"booted!!").unwrap();
        std::fs::write(src.join(SAVE_NAME), b"save").unwrap();
        std::fs::write(src.join("machine.ident"), b"ident").unwrap();
        disk::write_mac_id(&src, Uuid::from_u128(99));
        stamp_runtime(&src, &rt.rootfs);
        bake_ready(&src, rt);
        src
    }

    #[test]
    fn start_boots_when_there_is_no_save() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt, fake.clone());
        let sb = dir.path().join("sb");
        let id = Uuid::from_u128(1);
        assert_eq!(hv.start(id, &sb, limits()).unwrap(), StartKind::Cold);
        assert_eq!(*fake.boots.lock().unwrap(), vec![id]);
        assert!(fake.restores.lock().unwrap().is_empty());
    }

    #[test]
    fn start_restores_when_save_exists() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt, fake.clone());
        let sb = dir.path().join("sb");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join(SAVE_NAME), b"save").unwrap();
        let id = Uuid::from_u128(2);
        assert_eq!(hv.start(id, &sb, limits()).unwrap(), StartKind::Restored);
        assert_eq!(*fake.restores.lock().unwrap(), vec![id]);
        assert!(fake.boots.lock().unwrap().is_empty());
        assert!(sb.join(SAVE_NAME).is_file());
    }

    #[test]
    fn start_boots_when_restore_fails() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Arc::new(Fake {
            boots: Mutex::new(Vec::new()),
            restores: Mutex::new(Vec::new()),
            restore_disk_lens: Mutex::new(Vec::new()),
            restore_macs: Mutex::new(Vec::new()),
            pauses: Mutex::new(Vec::new()),
            resumes: Mutex::new(Vec::new()),
            saves: Mutex::new(Vec::new()),
            stops: Mutex::new(Vec::new()),
            restore_ok: false,
            pause_ok: true,
            resume_ok: true,
            save_ok: true,
        });
        let hv = Hypervisor::wrap(rt, fake.clone());
        let sb = dir.path().join("sb");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join(SAVE_NAME), b"save").unwrap();
        let id = Uuid::from_u128(3);
        assert_eq!(hv.start(id, &sb, limits()).unwrap(), StartKind::Cold);
        assert_eq!(*fake.restores.lock().unwrap(), vec![id]);
        assert_eq!(*fake.boots.lock().unwrap(), vec![id]);
        assert!(!sb.join(SAVE_NAME).exists());
    }

    #[test]
    fn save_and_stop_pauses_saves_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt, fake.clone());
        let id = Uuid::from_u128(4);
        let save = dir.path().join("machine.save");
        hv.save_and_stop(id, &save).unwrap();
        assert_eq!(*fake.pauses.lock().unwrap(), vec![id]);
        assert_eq!(*fake.saves.lock().unwrap(), vec![id]);
        assert_eq!(*fake.stops.lock().unwrap(), vec![id]);
    }

    #[test]
    fn save_and_stop_powers_off_when_save_fails() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Arc::new(Fake {
            boots: Mutex::new(Vec::new()),
            restores: Mutex::new(Vec::new()),
            restore_disk_lens: Mutex::new(Vec::new()),
            restore_macs: Mutex::new(Vec::new()),
            pauses: Mutex::new(Vec::new()),
            resumes: Mutex::new(Vec::new()),
            saves: Mutex::new(Vec::new()),
            stops: Mutex::new(Vec::new()),
            restore_ok: true,
            pause_ok: true,
            resume_ok: true,
            save_ok: false,
        });
        let hv = Hypervisor::wrap(rt, fake.clone());
        let id = Uuid::from_u128(5);
        let save = dir.path().join("machine.save");
        std::fs::write(&save, b"stale").unwrap();
        assert!(hv.save_and_stop(id, &save).is_err());
        assert_eq!(*fake.stops.lock().unwrap(), vec![id]);
        assert!(!save.exists());
    }

    #[test]
    fn start_restores_a_clone_of_the_first_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        let src = primed_booted(dir.path(), &rt);

        let b = dir.path().join("b");
        crate::environment::write_default(&b).unwrap();
        std::fs::write(
            b.join("environment/config.json"),
            r#"{"programs":{"claude-code":{"enable":true}}}"#,
        )
        .unwrap();
        let id = Uuid::from_u128(6);
        assert_eq!(hv.start(id, &b, limits()).unwrap(), StartKind::Restored);
        assert_eq!(*fake.restores.lock().unwrap(), vec![id]);
        assert_eq!(
            *fake.restore_macs.lock().unwrap(),
            vec![Uuid::from_u128(99)]
        );
        assert!(fake.boots.lock().unwrap().is_empty());
        assert!(b.join(SAVE_NAME).is_file());
        assert_eq!(std::fs::read(b.join("machine.ident")).unwrap(), b"ident");
        assert_eq!(
            &std::fs::read(b.join("disk").join("root.raw")).unwrap()[..8],
            b"booted!!"
        );
        assert!(b.join(HATCHED).is_file());
        assert!(!b.join("environment.applied").exists());
        assert!(ready_dir(&src, &rt).join(SAVE_NAME).is_file());
    }

    #[test]
    fn save_and_stop_of_a_user_sandbox_does_not_bake_ready() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake);
        let sb = dir.path().join("sb");
        crate::environment::write_default(&sb).unwrap();
        std::fs::create_dir_all(sb.join("disk")).unwrap();
        std::fs::write(sb.join("disk").join("root.raw"), vec![0u8; 64]).unwrap();
        std::fs::write(sb.join("machine.ident"), b"ident").unwrap();
        stamp_runtime(&sb, &rt.rootfs);
        let ready = ready_dir(&sb, &rt);
        std::fs::create_dir_all(&ready).unwrap();
        std::fs::write(ready.join("marker"), b"keep").unwrap();
        let save = sb.join(SAVE_NAME);
        std::fs::write(&save, b"save").unwrap();
        hv.save_and_stop(Uuid::from_u128(7), &save).unwrap();
        assert_eq!(std::fs::read(ready.join("marker")).unwrap(), b"keep");
        assert!(!ready.join("root.raw").exists());
        assert!(!hv.ready_snapshot_exists(dir.path()));
    }

    #[test]
    fn capture_ready_saves_and_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        let sb = dir.path().join("sb");
        crate::environment::write_default(&sb).unwrap();
        let id = Uuid::from_u128(11);
        hv.start(id, &sb, limits()).unwrap();
        std::fs::write(sb.join("machine.ident"), b"ident").unwrap();
        hv.capture_ready(id, &sb).unwrap();
        assert_eq!(*fake.pauses.lock().unwrap(), vec![id]);
        assert_eq!(*fake.saves.lock().unwrap(), vec![id]);
        assert_eq!(*fake.resumes.lock().unwrap(), vec![id]);
        assert!(ready_dir(&sb, &rt).join("root.raw").is_file());
        assert!(ready_dir(&sb, &rt).join(SAVE_NAME).is_file());
        assert_eq!(
            std::fs::read(ready_dir(&sb, &rt).join("machine.ident")).unwrap(),
            b"ident"
        );
        assert!(!sb.join(SAVE_NAME).exists());
        assert!(hv.ready_snapshot_exists(dir.path()));
    }

    #[test]
    fn second_hatch_restores_the_same_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        primed_booted(dir.path(), &rt);

        let b = dir.path().join("b");
        crate::environment::write_default(&b).unwrap();
        assert_eq!(
            hv.start(Uuid::from_u128(9), &b, limits()).unwrap(),
            StartKind::Restored
        );

        let c = dir.path().join("c");
        crate::environment::write_default(&c).unwrap();
        assert_eq!(
            hv.start(Uuid::from_u128(10), &c, limits()).unwrap(),
            StartKind::Restored
        );
        assert_eq!(
            *fake.restores.lock().unwrap(),
            vec![Uuid::from_u128(9), Uuid::from_u128(10)]
        );
        assert!(fake.boots.lock().unwrap().is_empty());
        assert!(b.join(HATCHED).is_file());
        assert!(c.join(HATCHED).is_file());
        assert!(hv.ready_snapshot_exists(dir.path()));
    }

    #[test]
    fn capture_ready_is_a_noop_when_the_disk_exists() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        primed_booted(dir.path(), &rt);
        hv.capture_ready(Uuid::from_u128(12), &dir.path().join("x"))
            .unwrap();
        assert!(fake.pauses.lock().unwrap().is_empty());
        assert!(fake.resumes.lock().unwrap().is_empty());
    }

    #[test]
    fn ready_snapshot_does_not_apply_across_runtimes() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let other = dir.path().join("other.raw");
        std::fs::write(&other, vec![1u8; 64]).unwrap();
        primed_booted(dir.path(), &rt);

        let fake = Fake::new();
        let hv = Hypervisor::wrap(
            Runtime {
                kernel: rt.kernel.clone(),
                initrd: rt.initrd.clone(),
                rootfs: other,
                cmdline: rt.cmdline.clone(),
            },
            fake.clone(),
        );
        let b = dir.path().join("b");
        crate::environment::write_default(&b).unwrap();
        let id = Uuid::from_u128(8);
        assert_eq!(hv.start(id, &b, limits()).unwrap(), StartKind::Cold);
        assert!(fake.restores.lock().unwrap().is_empty());
        assert_eq!(*fake.boots.lock().unwrap(), vec![id]);
        assert!(!b.join(HATCHED).exists());
    }

    #[test]
    fn ready_snapshot_without_runtime_stamp_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        let src = primed_booted(dir.path(), &rt);
        let ready = ready_dir(&src, &rt);
        std::fs::remove_file(ready.join("runtime.src")).unwrap();

        let b = dir.path().join("b");
        crate::environment::write_default(&b).unwrap();
        let id = Uuid::from_u128(14);
        assert_eq!(hv.start(id, &b, limits()).unwrap(), StartKind::Cold);
        assert!(fake.restores.lock().unwrap().is_empty());
        assert_eq!(*fake.boots.lock().unwrap(), vec![id]);
        assert!(!ready.exists());
    }

    #[test]
    fn existing_disk_does_not_steal_the_ready_image() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        let src = primed_booted(dir.path(), &rt);

        let sb = dir.path().join("sb");
        crate::environment::write_default(&sb).unwrap();
        std::fs::create_dir_all(sb.join("disk")).unwrap();
        std::fs::write(sb.join("disk").join("root.raw"), vec![1u8; 64]).unwrap();
        let id = Uuid::from_u128(13);
        assert_eq!(hv.start(id, &sb, limits()).unwrap(), StartKind::Cold);
        assert!(fake.restores.lock().unwrap().is_empty());
        assert!(ready_dir(&src, &rt).join("root.raw").is_file());
        assert!(!sb.join(HATCHED).exists());
    }

    #[test]
    fn start_does_not_grow_disk_before_restore() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        let b = dir.path().join("b");
        crate::environment::write_default(&b).unwrap();
        std::fs::create_dir_all(b.join("disk")).unwrap();
        std::fs::write(b.join("disk").join("root.raw"), vec![0u8; 64]).unwrap();
        std::fs::write(b.join(SAVE_NAME), b"save").unwrap();
        let id = Uuid::from_u128(15);
        assert_eq!(hv.start(id, &b, limits()).unwrap(), StartKind::Restored);
        assert_eq!(*fake.restore_disk_lens.lock().unwrap(), vec![64]);
        assert_eq!(
            std::fs::metadata(b.join("disk").join("root.raw"))
                .unwrap()
                .len(),
            256
        );
    }
}
