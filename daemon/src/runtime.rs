//! Locate the Nix-built sandbox runtime (kernel, initrd, root disk).

use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Runtime {
    pub kernel: PathBuf,
    pub initrd: PathBuf,
    pub rootfs: PathBuf,
    pub cmdline: String,
}

impl Runtime {
    pub fn discover() -> Option<Self> {
        for dir in candidate_dirs() {
            if let Some(rt) = load(&dir) {
                return Some(rt);
            }
        }
        None
    }

    /// Kernel command line from the runtime dir. Re-read so a rebuilt
    /// `guest/result` symlink does not keep a stale `init=` in memory
    /// while kernel/initrd already follow the link.
    pub fn boot_cmdline(&self) -> String {
        self.kernel
            .parent()
            .map(|dir| dir.join("cmdline"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.cmdline.clone())
    }

    /// Content identity of kernel + initrd + rootfs. Nix store paths
    /// already hash content; anything else mixes file bytes so an
    /// in-place copy cannot collide with the previous runtime.
    pub fn content_id(&self) -> String {
        let mut h = 2166136261u64;
        for p in [&self.kernel, &self.initrd, &self.rootfs] {
            mix_identity_path(&mut h, p);
        }
        if let Some(dir) = self.kernel.parent() {
            if let Ok(bytes) = std::fs::read(dir.join("runtime.src")) {
                mix_bytes(&mut h, &bytes);
            }
        }
        format!("{h:016x}")
    }
}

fn mix_bytes(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(16777619);
    }
}

fn mix_identity_path(h: &mut u64, p: &Path) {
    let resolved = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    mix_bytes(h, resolved.as_os_str().as_encoded_bytes());
    if resolved.starts_with("/nix/store") {
        return;
    }
    let Ok(mut file) = std::fs::File::open(p) else {
        return;
    };
    let mut buf = [0u8; 8192];
    loop {
        match file.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => mix_bytes(h, &buf[..n]),
        }
    }
}

fn load(dir: &Path) -> Option<Runtime> {
    let kernel = dir.join("kernel");
    let initrd = dir.join("initrd");
    let rootfs = dir.join("root.raw");
    let cmdline = dir.join("cmdline");
    if !(kernel.is_file() && initrd.is_file() && rootfs.is_file() && cmdline.is_file()) {
        return None;
    }
    let cmdline = std::fs::read_to_string(&cmdline).ok()?.trim().to_string();
    if cmdline.is_empty() {
        return None;
    }
    Some(Runtime {
        kernel,
        initrd,
        rootfs,
        cmdline,
    })
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(p) = std::env::var("SNOWBOX_RUNTIME") {
        dirs.push(PathBuf::from(p));
    }
    if let Some(data) = dirs::data_dir() {
        dirs.push(data.join("snowbox").join("runtime"));
    }
    if let Ok(exe) = std::env::current_exe() {
        // cargo run: target/debug/snowbox → ../../guest/result
        if let Some(debug) = exe.parent() {
            if let Some(target) = debug.parent() {
                if let Some(root) = target.parent() {
                    dirs.push(root.join("guest").join("result"));
                }
            }
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_requires_all_boot_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_none());
        std::fs::write(dir.path().join("kernel"), b"k").unwrap();
        std::fs::write(dir.path().join("initrd"), b"i").unwrap();
        std::fs::write(dir.path().join("root.raw"), b"r").unwrap();
        std::fs::write(dir.path().join("cmdline"), "console=hvc0\n").unwrap();
        let rt = load(dir.path()).unwrap();
        assert_eq!(rt.cmdline, "console=hvc0");
        std::fs::write(dir.path().join("cmdline"), "console=hvc0 init=/new\n").unwrap();
        assert_eq!(rt.boot_cmdline(), "console=hvc0 init=/new");
    }

    #[test]
    fn content_id_changes_when_rootfs_bytes_change_in_place() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("kernel"), b"k").unwrap();
        std::fs::write(dir.path().join("initrd"), b"i").unwrap();
        std::fs::write(dir.path().join("root.raw"), b"r").unwrap();
        std::fs::write(dir.path().join("cmdline"), "console=hvc0\n").unwrap();
        let rt = load(dir.path()).unwrap();
        let a = rt.content_id();
        std::fs::write(dir.path().join("root.raw"), b"R").unwrap();
        let b = rt.content_id();
        assert_ne!(a, b);
    }

    #[test]
    fn content_id_mixes_runtime_src_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("kernel"), b"k").unwrap();
        std::fs::write(dir.path().join("initrd"), b"i").unwrap();
        std::fs::write(dir.path().join("root.raw"), b"r").unwrap();
        std::fs::write(dir.path().join("cmdline"), "console=hvc0\n").unwrap();
        let rt = load(dir.path()).unwrap();
        let a = rt.content_id();
        std::fs::write(dir.path().join("runtime.src"), b"stamp").unwrap();
        let b = rt.content_id();
        assert_ne!(a, b);
    }
}
