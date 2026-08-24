use std::fs;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

const ALLOWED_ROOTS: [&str; 2] = ["/workspace", "/home/snow"];

/// One vsock connection: one verb, then close.
pub fn handle_socket(mut stream: UnixStream) -> io::Result<()> {
    let header = read_line(&mut stream)?;
    let (cmd, arg) = split_cmd(&header);
    match cmd {
        "PING" => stream.write_all(b"PONG\n"),
        "TAR_IN" => {
            tar_in(&mut stream, arg)?;
            stream.write_all(b"OK\n")
        }
        "TAR_OUT" => tar_out(&mut stream, arg),
        "NAR_IN" => {
            nar_in(&mut stream)?;
            stream.write_all(b"OK\n")
        }
        "PROFILE" => profile(arg).and_then(|()| stream.write_all(b"OK\n")),
        "RESET" => reset_dir(arg).and_then(|()| stream.write_all(b"OK\n")),
        "STTY" => stty(arg).and_then(|()| stream.write_all(b"OK\n")),
        "CONNECT" => connect_duplex(stream, arg),
        _ => {
            let _ = stream.write_all(b"ERR unknown\n");
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown command {cmd}"),
            ))
        }
    }
}

fn read_line(stream: &mut impl Read) -> io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte)?;
        if n == 0 || byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
        if buf.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "header too long",
            ));
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn split_cmd(header: &str) -> (&str, &str) {
    let header = header.trim_end();
    match header.split_once(' ') {
        Some((cmd, arg)) => (cmd, arg.trim()),
        None => (header, ""),
    }
}

fn tar_in(stream: &mut impl Read, dest: &str) -> io::Result<()> {
    let dest = allow_guest_path(dest)?;
    unpack_tar_in(stream, &dest)?;
    let _ = Command::new("chown")
        .args(["-R", "snow:snow"])
        .arg(&dest)
        .status();
    Ok(())
}

fn unpack_tar_in(stream: &mut impl Read, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    let mut archive = tar::Archive::new(stream);
    archive.set_unpack_xattrs(false);
    archive.set_preserve_permissions(false);
    for entry in archive.entries()? {
        let mut entry = entry?;
        entry.set_unpack_xattrs(false);
        entry.set_preserve_permissions(false);
        if !entry.unpack_in(dest)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tar member escaped dest",
            ));
        }
    }
    Ok(())
}

fn tar_out(stream: &mut impl Write, src: &str) -> io::Result<()> {
    let src = allow_guest_path(src)?;
    let mut builder = tar::Builder::new(stream);
    builder.append_dir_all(".", &src)?;
    builder.finish()
}

fn nar_in(stream: &mut impl Read) -> io::Result<()> {
    let mut child = Command::new("nix-store")
        .arg("--import")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    io::copy(stream, &mut stdin)?;
    drop(stdin);
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "nix-store --import: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

fn profile(store_path: &str) -> io::Result<()> {
    if !is_nix_store_path(store_path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PROFILE path must be a nix store path",
        ));
    }
    let store_path = Path::new(store_path);
    fs::create_dir_all("/nix/var/nix/profiles")?;
    let dest = Path::new("/nix/var/nix/profiles/snowbox-environment");
    let _ = fs::remove_file(dest);
    std::os::unix::fs::symlink(profile_link_target(store_path), dest)?;
    let activate = store_path.join("activate");
    if activate.is_file() {
        let status = Command::new("runuser")
            .args(["-u", "snow", "--"])
            .arg(&activate)
            .status()?;
        if !status.success() {
            return Err(io::Error::other("home-manager activate failed"));
        }
    }
    Ok(())
}

/// HM activationPackage puts user binaries in `home-path/bin`, not `$out/bin`.
fn profile_link_target(store_path: &Path) -> PathBuf {
    let home_path = store_path.join("home-path");
    if home_path.is_dir() {
        home_path
    } else {
        store_path.to_path_buf()
    }
}

fn is_nix_store_path(path: &str) -> bool {
    let p = Path::new(path);
    if !p.is_absolute() {
        return false;
    }
    let mut comps = p.components();
    matches!(
        (
            comps.next(),
            comps.next(),
            comps.next(),
            comps.next(),
            comps.next()
        ),
        (
            Some(Component::RootDir),
            Some(Component::Normal(nix)),
            Some(Component::Normal(store)),
            Some(Component::Normal(hash_name)),
            None
        ) if nix == "nix" && store == "store" && !hash_name.is_empty()
    )
}

fn reset_dir(dest: &str) -> io::Result<()> {
    let dest = allow_guest_path(dest)?;
    empty_dir(&dest)?;
    let _ = Command::new("chown")
        .args(["snow:snow"])
        .arg(&dest)
        .status();
    Ok(())
}

fn empty_dir(dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for ent in fs::read_dir(dest)? {
        let path = ent?.path();
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn stty(arg: &str) -> io::Result<()> {
    let arg = arg.trim();
    let (size, pts) = match arg.split_once(' ') {
        Some((size, pts)) => (size, pts.trim()),
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "STTY requires a pts path",
            ));
        }
    };
    if pts.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "STTY requires a pts path",
        ));
    }
    let pts_path = Path::new(pts);
    if !pts_path.starts_with("/dev/pts")
        || pts_path.file_name().is_some_and(|n| n == "ptmx")
        || pts_path
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "STTY pts path",
        ));
    }
    let (rows, cols) = parse_winsize(size)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "STTY rowsxcols"))?;
    let f = fs::File::options().write(true).open(pts_path)?;
    use std::os::fd::AsRawFd;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    ws.ws_row = rows;
    ws.ws_col = cols;
    if unsafe { libc::ioctl(f.as_raw_fd(), libc::TIOCSWINSZ, &ws) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn parse_winsize(arg: &str) -> Option<(u16, u16)> {
    let (rows, cols) = arg.split_once('x')?;
    Some((rows.parse().ok()?, cols.parse().ok()?))
}

fn connect_duplex(stream: UnixStream, port: &str) -> io::Result<()> {
    let mut tcp = TcpStream::connect(format!("127.0.0.1:{port}"))?;
    tcp.set_nodelay(true)?;
    let mut stream_r = stream.try_clone()?;
    let mut tcp_r = tcp.try_clone()?;
    let mut stream_w = stream;
    let up = std::thread::spawn(move || io::copy(&mut stream_r, &mut tcp_r));
    let _ = io::copy(&mut tcp, &mut stream_w);
    let _ = up.join();
    Ok(())
}

fn allow_guest_path(path: &str) -> io::Result<PathBuf> {
    allow_guest_path_in(path, &ALLOWED_ROOTS)
}

fn allow_guest_path_in(path: &str, roots: &[&str]) -> io::Result<PathBuf> {
    if path.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty path"));
    }
    let requested = Path::new(path);
    if !requested.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must be absolute",
        ));
    }
    let lexical = lexically_normalize(requested)?;
    let Some(root) = roots.iter().map(Path::new).find(|r| is_under(&lexical, r)) else {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path not allowed",
        ));
    };
    let root_canon = if root.exists() {
        root.canonicalize()?
    } else {
        root.to_path_buf()
    };
    let rel = lexical.strip_prefix(root).unwrap_or(Path::new(""));
    let mut acc = root_canon.clone();
    for c in rel.components() {
        acc.push(c);
        if acc.is_symlink() {
            let canon = acc
                .canonicalize()
                .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "path not allowed"))?;
            if !is_under(&canon, &root_canon) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "path not allowed",
                ));
            }
            acc = canon;
        }
    }
    Ok(acc)
}

fn lexically_normalize(path: &Path) -> io::Result<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() || out.as_os_str().is_empty() {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "path escapes"));
                }
            }
            Component::Normal(p) => out.push(p),
            Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "path not allowed",
                ));
            }
        }
    }
    Ok(out)
}

fn is_under(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn ping_pong() {
        let (mut a, b) = UnixStream::pair().unwrap();
        thread::spawn(move || handle_socket(b).unwrap());
        a.write_all(b"PING\n").unwrap();
        let mut buf = String::new();
        a.read_to_string(&mut buf).unwrap();
        assert!(buf.contains("PONG"));
    }

    #[test]
    fn split_header() {
        assert_eq!(split_cmd("RESET /workspace"), ("RESET", "/workspace"));
        assert_eq!(split_cmd("PING"), ("PING", ""));
        assert_eq!(parse_winsize("24x80"), Some((24, 80)));
    }

    #[test]
    fn allowlist_accepts_workspace_and_home() {
        for p in [
            "/workspace",
            "/workspace/src",
            "/home/snow",
            "/home/snow/.gitconfig",
        ] {
            allow_guest_path(p).unwrap_or_else(|e| panic!("{p}: {e}"));
        }
    }

    #[test]
    fn allowlist_rejects_outside() {
        for p in [
            "/",
            "/etc",
            "/tmp/escape",
            "/workspace/../etc",
            "/home/snow/../../etc",
        ] {
            assert!(allow_guest_path(p).is_err(), "{p} should be rejected");
        }
        assert!(allow_guest_path("workspace").is_err());
        assert!(allow_guest_path("").is_err());
    }

    #[test]
    fn allowlist_rejects_symlink_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("workspace");
        fs::create_dir(&root).unwrap();
        let outside = tmp.path().join("secret");
        fs::write(&outside, "x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let roots = [root.to_str().unwrap()];
        let escaped = root.join("link");
        assert!(allow_guest_path_in(escaped.to_str().unwrap(), &roots).is_err());
        assert!(allow_guest_path_in(root.to_str().unwrap(), &roots).is_ok());
    }

    #[test]
    fn unpack_in_does_not_write_outside_dest() {
        let dest = tempfile::tempdir().unwrap();
        let outside = dest.path().parent().unwrap().join("pwned");
        let _ = fs::remove_file(&outside);
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.as_gnu_mut().unwrap().name[..8].copy_from_slice(b"../pwned");
            header.set_size(3);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append(&header, &b"yes"[..]).unwrap();
            builder.finish().unwrap();
        }
        let err = unpack_tar_in(&mut tar_bytes.as_slice(), dest.path()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(!outside.exists(), "escaped tar member was written");
    }

    #[test]
    fn empty_dir_clears_contents() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("f"), "x").unwrap();
        fs::write(dir.path().join(".hidden"), "y").unwrap();
        empty_dir(dir.path()).unwrap();
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn tar_roundtrip_stays_in_dest() {
        let src = tempfile::tempdir().unwrap();
        fs::write(src.path().join("hi"), "hello").unwrap();
        let dest = tempfile::tempdir().unwrap();
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            builder.append_dir_all(".", src.path()).unwrap();
            builder.finish().unwrap();
        }
        unpack_tar_in(&mut tar_bytes.as_slice(), dest.path()).unwrap();
        assert_eq!(fs::read_to_string(dest.path().join("hi")).unwrap(), "hello");
    }

    #[test]
    fn tar_in_rejects_etc() {
        let (mut a, b) = UnixStream::pair().unwrap();
        let t = thread::spawn(move || handle_socket(b));
        a.write_all(b"TAR_IN /etc\n").unwrap();
        let _ = a.shutdown(std::net::Shutdown::Write);
        let mut reply = String::new();
        let _ = a.read_to_string(&mut reply);
        assert!(!reply.contains("OK"), "{reply}");
        let _ = t.join();
    }

    #[test]
    fn reset_rejects_root() {
        let (mut a, b) = UnixStream::pair().unwrap();
        let t = thread::spawn(move || handle_socket(b));
        a.write_all(b"RESET /\n").unwrap();
        let _ = a.shutdown(std::net::Shutdown::Write);
        let mut reply = String::new();
        let _ = a.read_to_string(&mut reply);
        assert!(!reply.contains("OK"), "{reply}");
        let _ = t.join();
    }

    #[test]
    fn stty_without_pts_errors() {
        let err = stty("24x80").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn profile_requires_store_path() {
        assert!(is_nix_store_path(
            "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-home-manager-generation"
        ));
        assert!(!is_nix_store_path("/etc"));
        assert!(!is_nix_store_path("/"));
        assert!(!is_nix_store_path("/nix/store"));
        assert!(!is_nix_store_path(
            "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-pkg/../../../etc"
        ));
        assert!(profile("/etc").is_err());
        assert!(profile("/nix/store/foo/../../../etc").is_err());
    }

    #[test]
    fn profile_prefers_home_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("gen");
        fs::create_dir_all(store.join("home-path/bin")).unwrap();
        fs::write(store.join("activate"), "true").unwrap();
        assert_eq!(profile_link_target(&store), store.join("home-path"));
        let plain = dir.path().join("plain");
        fs::create_dir(&plain).unwrap();
        assert_eq!(profile_link_target(&plain), plain);
    }
}
