//! NAR dump and `nix-store --export` framing, written to files so the
//! helper does not hold a whole closure in one Vec.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const EXPORT_MAGIC: u64 = 0x4558_494e; // "NIXE"

pub fn dump_path_to(path: &Path, out: &mut impl Write) -> io::Result<()> {
    write_str(out, "nix-archive-1")?;
    dump_node(path, out)
}

pub fn write_export_path(
    out: &mut impl Write,
    store_path: &str,
    nar: &mut impl Read,
    references: &[String],
) -> io::Result<()> {
    write_u64(out, 1)?;
    io::copy(nar, out)?;
    write_u64(out, EXPORT_MAGIC)?;
    write_str(out, store_path)?;
    write_strs(out, references)?;
    write_str(out, "")?; // deriver
    write_u64(out, 0)?; // no legacy signature
    Ok(())
}

pub fn write_export_end(out: &mut impl Write) -> io::Result<()> {
    write_u64(out, 0)
}

fn dump_node(path: &Path, out: &mut impl Write) -> io::Result<()> {
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

fn write_str(out: &mut impl Write, s: &str) -> io::Result<()> {
    write_bytes(out, s.as_bytes())
}

fn write_strs(out: &mut impl Write, ss: &[String]) -> io::Result<()> {
    write_u64(out, ss.len() as u64)?;
    for s in ss {
        write_str(out, s)?;
    }
    Ok(())
}

fn write_bytes(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    write_u64(out, bytes.len() as u64)?;
    out.write_all(bytes)?;
    let pad = (8 - (bytes.len() % 8)) % 8;
    out.write_all(&[0u8; 8][..pad])?;
    Ok(())
}

fn write_u64(out: &mut impl Write, n: u64) -> io::Result<()> {
    out.write_all(&n.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn dumps_regular_file_to_writer() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("hello");
        fs::write(&f, b"hi").unwrap();
        let mut out = Vec::new();
        dump_path_to(&f, &mut out).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("nix-archive-1"));
        assert!(s.contains("regular"));
        assert!(s.contains("hi"));
    }

    #[test]
    fn export_frames_nar_from_reader() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f"), b"x").unwrap();
        let mut nar = Vec::new();
        dump_path_to(dir.path(), &mut nar).unwrap();
        let mut framed = Vec::new();
        write_export_path(
            &mut framed,
            "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x",
            &mut Cursor::new(&nar),
            &[],
        )
        .unwrap();
        assert_eq!(&framed[..8], 1u64.to_le_bytes());
        let mut end = Vec::new();
        write_export_end(&mut end).unwrap();
        assert_eq!(end, 0u64.to_le_bytes());
        assert!(framed.len() > nar.len());
    }
}
