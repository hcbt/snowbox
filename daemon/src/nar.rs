//! NAR dump and `nix-store --export` framing. Used to copy a closure into
//! the Cache and into a guest without nix-bindings copy_closure.

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const EXPORT_MAGIC: u64 = 0x4558_494e; // "NIXE"

pub fn dump_path(path: &Path) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    write_str(&mut out, "nix-archive-1")?;
    dump_node(path, &mut out)?;
    Ok(out)
}

/// One path in `nix-store --export` form (NAR + trailer). References first.
pub fn export_path(store_path: &str, nar: &[u8], references: &[String]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    write_u64(&mut out, 1)?;
    out.extend_from_slice(nar);
    write_u64(&mut out, EXPORT_MAGIC)?;
    write_str(&mut out, store_path)?;
    write_strs(&mut out, references)?;
    write_str(&mut out, "")?; // deriver
    write_u64(&mut out, 0)?; // no legacy signature
    Ok(out)
}

pub fn export_end() -> Vec<u8> {
    let mut out = Vec::new();
    let _ = write_u64(&mut out, 0);
    out
}

fn dump_node(path: &Path, out: &mut Vec<u8>) -> io::Result<()> {
    write_str(out, "(")?;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        write_str(out, "type")?;
        write_str(out, "symlink")?;
        write_str(out, "target")?;
        let target = fs::read_link(path)?;
        write_str(out, &target.to_string_lossy())?;
    } else if meta.is_dir() {
        write_str(out, "type")?;
        write_str(out, "directory")?;
        let mut entries: Vec<_> = fs::read_dir(path)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            write_str(out, "entry")?;
            write_str(out, "(")?;
            write_str(out, "name")?;
            write_str(out, &entry.file_name().to_string_lossy())?;
            write_str(out, "node")?;
            dump_node(&entry.path(), out)?;
            write_str(out, ")")?;
        }
    } else {
        write_str(out, "type")?;
        write_str(out, "regular")?;
        if meta.permissions().mode() & 0o111 != 0 {
            write_str(out, "executable")?;
            write_str(out, "")?;
        }
        write_str(out, "contents")?;
        let bytes = fs::read(path)?;
        write_bytes(out, &bytes)?;
    }
    write_str(out, ")")?;
    Ok(())
}

fn write_str(out: &mut Vec<u8>, s: &str) -> io::Result<()> {
    write_bytes(out, s.as_bytes())
}

fn write_strs(out: &mut Vec<u8>, ss: &[String]) -> io::Result<()> {
    write_u64(out, ss.len() as u64)?;
    for s in ss {
        write_str(out, s)?;
    }
    Ok(())
}

fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    write_u64(out, bytes.len() as u64)?;
    out.write_all(bytes)?;
    let pad = (8 - (bytes.len() % 8)) % 8;
    out.write_all(&[0u8; 8][..pad])?;
    Ok(())
}

fn write_u64(out: &mut Vec<u8>, n: u64) -> io::Result<()> {
    out.write_all(&n.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumps_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("hello");
        fs::write(&f, b"hi").unwrap();
        let nar = dump_path(&f).unwrap();
        let s = String::from_utf8_lossy(&nar);
        assert!(s.contains("nix-archive-1"));
        assert!(s.contains("regular"));
        assert!(s.contains("hi"));
    }

    #[test]
    fn export_frames_nar() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f"), b"x").unwrap();
        let nar = dump_path(dir.path()).unwrap();
        let framed =
            export_path("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x", &nar, &[]).unwrap();
        assert_eq!(&framed[..8], 1u64.to_le_bytes());
        let end = export_end();
        assert_eq!(end, 0u64.to_le_bytes());
        assert!(framed.len() > nar.len());
    }
}
