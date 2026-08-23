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
        self.engine.stop(id)
    }

    pub fn stop(&self, id: Uuid) -> Result<(), String> {
        self.engine.stop(id)
    }

    pub fn vsock(&self, id: Uuid, port: u32) -> Result<UnixStream, String> {
        self.engine.vsock(id, port)
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
}
