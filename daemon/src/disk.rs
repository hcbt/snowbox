//! Guest root disk on the Host. Shared by every hypervisor.

use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn prepare_root_disk(
    sandbox_dir: &Path,
    runtime_rootfs: &Path,
    disk: u64,
) -> Result<PathBuf, String> {
    prepare_disk(sandbox_dir, runtime_rootfs, None, disk)
}

pub(crate) fn prepare_disk(
    sandbox_dir: &Path,
    runtime_rootfs: &Path,
    booted_template: Option<&Path>,
    disk: u64,
) -> Result<PathBuf, String> {
    let disk_dir = sandbox_dir.join("disk");
    std::fs::create_dir_all(&disk_dir).map_err(|e| format!("mkdir disk: {e}"))?;
    let root = disk_dir.join("root.raw");
    if !root.exists() {
        // Runtime lives on the Nix volume; Sandbox disks live on the data
        // volume. clonefile cannot cross volumes, so copy onto this volume
        // once and clone from there.
        if let Some(booted) = booted_template.filter(|p| p.is_file()) {
            clone_or_copy(booted, &root)?;
        } else {
            let template = sandbox_dir
                .parent()
                .unwrap_or(sandbox_dir)
                .join(".runtime-root.raw");
            ensure_runtime_template(runtime_rootfs, &template)?;
            clone_or_copy(&template, &root)?;
        }
        let key = runtime_rootfs
            .canonicalize()
            .unwrap_or_else(|_| runtime_rootfs.to_path_buf());
        let _ = std::fs::write(
            sandbox_dir.join("runtime.src"),
            key.to_string_lossy().as_bytes(),
        );
    }
    let mut perms = std::fs::metadata(&root)
        .map_err(|e| format!("stat rootfs: {e}"))?
        .permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&root, perms).map_err(|e| format!("chmod rootfs: {e}"))?;
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
    // Snapshots baked from a previous runtime's disk must not be restored
    // under this one (kernel cmdline init= would miss the store path).
    if let Some(parent) = template.parent() {
        let _ = std::fs::remove_dir_all(parent.join(".ready"));
    }
    Ok(())
}

pub(crate) fn clone_file(src: &Path, dst: &Path) -> Result<(), String> {
    clone_or_copy(src, dst)
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

pub fn read_mac_id(dir: &Path, fallback: Uuid) -> Uuid {
    std::fs::read_to_string(dir.join("mac.id"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(fallback)
}

pub fn write_mac_id(dir: &Path, id: Uuid) {
    let _ = std::fs::write(dir.join("mac.id"), id.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_root_disk_copies_and_grows() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("runtime.raw");
        std::fs::write(&src, vec![0u8; 64]).unwrap();
        let sandbox = dir.path().join("sb");
        let root = prepare_root_disk(&sandbox, &src, 256).unwrap();
        assert_eq!(std::fs::metadata(&root).unwrap().len(), 256);
        assert_eq!(&std::fs::read(&root).unwrap()[..64], &[0u8; 64]);

        std::fs::write(&src, vec![1u8; 64]).unwrap();
        prepare_root_disk(&sandbox, &src, 256).unwrap();
        assert_eq!(&std::fs::read(&root).unwrap()[..64], &[0u8; 64]);
    }

    #[test]
    fn prepare_root_disk_clones_from_a_same_volume_template() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("runtime.raw");
        std::fs::write(&src, b"rootfs").unwrap();
        let a = prepare_root_disk(&dir.path().join("a"), &src, 64).unwrap();
        let b = prepare_root_disk(&dir.path().join("b"), &src, 64).unwrap();
        assert_eq!(std::fs::read(&a).unwrap()[..6], *b"rootfs");
        assert_eq!(std::fs::read(&b).unwrap()[..6], *b"rootfs");
        assert!(dir.path().join(".runtime-root.raw").is_file());
    }

    #[test]
    fn prepare_root_disk_refuses_when_image_exceeds_limit() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("runtime.raw");
        std::fs::write(&src, vec![0u8; 200]).unwrap();
        let err = prepare_root_disk(&dir.path().join("sb"), &src, 100).unwrap_err();
        assert!(err.contains("exceeds"));
    }

    #[test]
    fn mac_id_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::from_u128(1);
        write_mac_id(dir.path(), id);
        assert_eq!(read_mac_id(dir.path(), uuid::Uuid::nil()), id);
    }

    #[test]
    fn prepare_disk_clones_a_booted_template() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("runtime.raw");
        let booted = dir.path().join("booted.raw");
        std::fs::write(&src, b"unbooted").unwrap();
        std::fs::write(&booted, b"booted!!").unwrap();
        let root = prepare_disk(&dir.path().join("sb"), &src, Some(&booted), 64).unwrap();
        assert_eq!(&std::fs::read(&root).unwrap()[..8], b"booted!!");
    }
}
