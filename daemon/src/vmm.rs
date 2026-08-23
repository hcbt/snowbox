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
        if !own_save.exists() {
            install_ready(sandbox_dir, &self.runtime.rootfs);
        }
        let mac_id = disk::read_mac_id(sandbox_dir, id);
        disk::prepare_root_disk(sandbox_dir, &self.runtime.rootfs, limits.disk)?;
        if own_save.exists() {
            match self
                .engine
                .restore(&self.runtime, id, sandbox_dir, limits, mac_id, &own_save)
            {
                Ok(()) => {
                    disk::write_mac_id(sandbox_dir, mac_id);
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
        bake_ready(save.parent().unwrap_or(Path::new("")), &self.runtime.rootfs);
        Ok(())
    }

    pub fn stop(&self, id: Uuid) -> Result<(), String> {
        self.engine.stop(id)
    }

    pub fn vsock(&self, id: Uuid, port: u32) -> Result<UnixStream, String> {
        self.engine.vsock(id, port)
    }

    pub fn ready_snapshot_exists(&self, sandboxes_root: &Path) -> bool {
        snapshot_complete(&ready_dir(&sandboxes_root.join("_"), &self.runtime.rootfs))
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

fn ready_key(runtime: &Path) -> String {
    let p = runtime
        .canonicalize()
        .unwrap_or_else(|_| runtime.to_path_buf());
    let mut h = 2166136261u64;
    for b in p.as_os_str().as_encoded_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(16777619);
    }
    format!("{h:016x}")
}

fn ready_dir(sandbox_dir: &Path, runtime: &Path) -> PathBuf {
    sandbox_dir
        .parent()
        .unwrap_or(sandbox_dir)
        .join(".ready")
        .join(ready_key(runtime))
}

fn snapshot_complete(dir: &Path) -> bool {
    dir.join(SAVE_NAME).is_file()
        && dir.join("root.raw").is_file()
        && dir.join("machine.ident").is_file()
}

fn install_ready(sandbox_dir: &Path, runtime: &Path) {
    let ready = ready_dir(sandbox_dir, runtime);
    if !snapshot_complete(&ready) {
        eprintln!("ready snapshot: none");
        return;
    }
    // Consume the snapshot so a second New Sandbox cannot restore the
    // same Apple machine identifier while the first is running.
    let taking = ready.with_extension("taking");
    let _ = std::fs::remove_dir_all(&taking);
    if std::fs::rename(&ready, &taking).is_err() {
        return;
    }
    let disk_dir = sandbox_dir.join("disk");
    let dst_disk = disk_dir.join("root.raw");
    if let Err(e) = std::fs::create_dir_all(&disk_dir) {
        eprintln!("ready snapshot: mkdir disk: {e}");
        let _ = std::fs::rename(&taking, &ready);
        return;
    }
    if !dst_disk.exists() {
        if let Err(e) = disk::clone_file(&taking.join("root.raw"), &dst_disk) {
            eprintln!("ready snapshot: clone disk: {e}");
            let _ = std::fs::rename(&taking, &ready);
            return;
        }
    }
    for name in [SAVE_NAME, "machine.ident", "mac.id", "environment.applied"] {
        let src = taking.join(name);
        if src.is_file() {
            let _ = std::fs::copy(&src, sandbox_dir.join(name));
        }
    }
    let _ = std::fs::write(sandbox_dir.join(HATCHED), b"");
    let _ = std::fs::remove_dir_all(&taking);
}

fn bake_ready(sandbox_dir: &Path, runtime: &Path) {
    let src_save = sandbox_dir.join(SAVE_NAME);
    let src_disk = sandbox_dir.join("disk").join("root.raw");
    let src_ident = sandbox_dir.join("machine.ident");
    if !(src_save.is_file() && src_disk.is_file() && src_ident.is_file()) {
        return;
    }
    let dest = ready_dir(sandbox_dir, runtime);
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
    for (src, name) in [
        (src_save.as_path(), SAVE_NAME),
        (src_ident.as_path(), "machine.ident"),
        (sandbox_dir.join("mac.id").as_path(), "mac.id"),
        (
            sandbox_dir.join("environment.applied").as_path(),
            "environment.applied",
        ),
    ] {
        if src.is_file() {
            let _ = std::fs::copy(src, tmp.join(name));
        }
    }
    let _ = std::fs::remove_dir_all(&dest);
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        eprintln!("ready snapshot: install: {e}");
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    }
    if let Some(parent) = dest.parent() {
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                if entry.path() != dest {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        boots: Mutex<Vec<Uuid>>,
        restores: Mutex<Vec<Uuid>>,
        pauses: Mutex<Vec<Uuid>>,
        saves: Mutex<Vec<Uuid>>,
        stops: Mutex<Vec<Uuid>>,
        restore_ok: bool,
        pause_ok: bool,
        save_ok: bool,
    }

    impl Fake {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                boots: Mutex::new(Vec::new()),
                restores: Mutex::new(Vec::new()),
                pauses: Mutex::new(Vec::new()),
                saves: Mutex::new(Vec::new()),
                stops: Mutex::new(Vec::new()),
                restore_ok: true,
                pause_ok: true,
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
            _sandbox_dir: &Path,
            _limits: Limits,
            _mac_id: Uuid,
            _save: &Path,
        ) -> Result<(), String> {
            self.restores.lock().expect("restores").push(id);
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

        fn save(&self, id: Uuid, _save: &Path) -> Result<(), String> {
            self.saves.lock().expect("saves").push(id);
            if self.save_ok {
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
            pauses: Mutex::new(Vec::new()),
            saves: Mutex::new(Vec::new()),
            stops: Mutex::new(Vec::new()),
            restore_ok: false,
            pause_ok: true,
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
            pauses: Mutex::new(Vec::new()),
            saves: Mutex::new(Vec::new()),
            stops: Mutex::new(Vec::new()),
            restore_ok: true,
            pause_ok: true,
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
    fn start_restores_a_new_sandbox_from_a_ready_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        let a = dir.path().join("a");
        crate::environment::write_default(&a).unwrap();
        std::fs::create_dir_all(a.join("disk")).unwrap();
        std::fs::write(a.join("disk").join("root.raw"), vec![0u8; 64]).unwrap();
        std::fs::write(a.join(SAVE_NAME), b"save").unwrap();
        std::fs::write(a.join("machine.ident"), b"ident").unwrap();
        std::fs::write(a.join("environment.applied"), b"stamp").unwrap();
        bake_ready(&a, &rt.rootfs);

        let b = dir.path().join("b");
        crate::environment::write_default(&b).unwrap();
        // Different Packages must not force a cold boot; the snapshot is
        // the guest runtime, and Environment is applied after restore.
        std::fs::write(b.join("environment/packages.json"), r#"["hello"]"#).unwrap();
        let id = Uuid::from_u128(6);
        assert_eq!(hv.start(id, &b, limits()).unwrap(), StartKind::Restored);
        assert_eq!(*fake.restores.lock().unwrap(), vec![id]);
        assert!(fake.boots.lock().unwrap().is_empty());
        assert!(b.join(SAVE_NAME).is_file());
        assert!(b.join("disk").join("root.raw").is_file());
        assert!(!ready_dir(&a, &rt.rootfs).exists());
    }

    #[test]
    fn save_and_stop_bakes_a_ready_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        let sb = dir.path().join("sb");
        crate::environment::write_default(&sb).unwrap();
        std::fs::create_dir_all(sb.join("disk")).unwrap();
        std::fs::write(sb.join("disk").join("root.raw"), vec![0u8; 64]).unwrap();
        std::fs::write(sb.join("machine.ident"), b"ident").unwrap();
        let save = sb.join(SAVE_NAME);
        std::fs::write(&save, b"save").unwrap();
        hv.save_and_stop(Uuid::from_u128(7), &save).unwrap();
        assert!(ready_dir(&sb, &rt.rootfs).join(SAVE_NAME).is_file());
    }

    #[test]
    fn ready_snapshot_is_consumed_so_a_second_hatch_boots() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake.clone());
        let a = dir.path().join("a");
        crate::environment::write_default(&a).unwrap();
        std::fs::create_dir_all(a.join("disk")).unwrap();
        std::fs::write(a.join("disk").join("root.raw"), vec![0u8; 64]).unwrap();
        std::fs::write(a.join(SAVE_NAME), b"save").unwrap();
        std::fs::write(a.join("machine.ident"), b"ident").unwrap();
        bake_ready(&a, &rt.rootfs);

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
            StartKind::Cold
        );
        assert_eq!(*fake.boots.lock().unwrap(), vec![Uuid::from_u128(10)]);
    }

    #[test]
    fn ready_snapshot_does_not_apply_across_runtimes() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let other = dir.path().join("other.raw");
        std::fs::write(&other, vec![0u8; 64]).unwrap();
        let a = dir.path().join("a");
        crate::environment::write_default(&a).unwrap();
        std::fs::create_dir_all(a.join("disk")).unwrap();
        std::fs::write(a.join("disk").join("root.raw"), vec![0u8; 64]).unwrap();
        std::fs::write(a.join(SAVE_NAME), b"save").unwrap();
        std::fs::write(a.join("machine.ident"), b"ident").unwrap();
        bake_ready(&a, &rt.rootfs);

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
    }

    #[test]
    fn bake_drops_snapshots_for_other_runtimes() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let stale = dir.path().join(".ready").join("deadbeefdeadbeef");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join(SAVE_NAME), b"old").unwrap();
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake);
        let sb = dir.path().join("sb");
        crate::environment::write_default(&sb).unwrap();
        std::fs::create_dir_all(sb.join("disk")).unwrap();
        std::fs::write(sb.join("disk").join("root.raw"), vec![0u8; 64]).unwrap();
        std::fs::write(sb.join("machine.ident"), b"ident").unwrap();
        std::fs::write(sb.join(SAVE_NAME), b"save").unwrap();
        hv.save_and_stop(Uuid::from_u128(12), &sb.join(SAVE_NAME))
            .unwrap();
        assert!(!stale.exists());
        assert!(hv.ready_snapshot_exists(dir.path()));
    }

    #[test]
    fn ready_snapshot_exists_after_bake() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime(dir.path());
        let fake = Fake::new();
        let hv = Hypervisor::wrap(rt.clone(), fake);
        let sb = dir.path().join("sb");
        crate::environment::write_default(&sb).unwrap();
        std::fs::create_dir_all(sb.join("disk")).unwrap();
        std::fs::write(sb.join("disk").join("root.raw"), vec![0u8; 64]).unwrap();
        std::fs::write(sb.join("machine.ident"), b"ident").unwrap();
        std::fs::write(sb.join(SAVE_NAME), b"save").unwrap();
        assert!(!hv.ready_snapshot_exists(dir.path()));
        hv.save_and_stop(Uuid::from_u128(11), &sb.join(SAVE_NAME))
            .unwrap();
        assert!(hv.ready_snapshot_exists(dir.path()));
    }
}
