//! Locate the Nix-built sandbox runtime (kernel, initrd, root disk).

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
    }
}
