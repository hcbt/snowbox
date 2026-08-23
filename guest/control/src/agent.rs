use std::fs;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};

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
    if dest.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "TAR_IN path"));
    }
    fs::create_dir_all(dest)?;
    let mut archive = tar::Archive::new(stream);
    archive.set_unpack_xattrs(false);
    archive.set_preserve_permissions(false);
    archive.unpack(dest)?;
    let _ = Command::new("chown")
        .args(["-R", "snow:snow", dest])
        .status();
    Ok(())
}

fn tar_out(stream: &mut impl Write, src: &str) -> io::Result<()> {
    if src.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "TAR_OUT path"));
    }
    let mut builder = tar::Builder::new(stream);
    builder.append_dir_all(".", src)?;
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
    if store_path.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "PROFILE path"));
    }
    fs::create_dir_all("/nix/var/nix/profiles")?;
    let dest = Path::new("/nix/var/nix/profiles/snowbox-environment");
    let _ = fs::remove_file(dest);
    std::os::unix::fs::symlink(store_path, dest)?;
    let activate = Path::new(store_path).join("activate");
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

fn reset_dir(dest: &str) -> io::Result<()> {
    if dest.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "RESET path"));
    }
    fs::create_dir_all(dest)?;
    for ent in fs::read_dir(dest)? {
        let path = ent?.path();
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    let _ = Command::new("chown").args(["snow:snow", dest]).status();
    Ok(())
}

fn stty(arg: &str) -> io::Result<()> {
    let (rows, cols) = parse_winsize(arg).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "STTY rowsxcols")
    })?;
    let Ok(dir) = fs::read_dir("/dev/pts") else {
        return Ok(());
    };
    for ent in dir {
        let Ok(ent) = ent else { continue };
        let path = ent.path();
        if path.file_name().is_some_and(|n| n == "ptmx") {
            continue;
        }
        let Ok(f) = fs::File::options().write(true).open(&path) else {
            continue;
        };
        use std::os::fd::AsRawFd;
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        ws.ws_row = rows;
        ws.ws_col = cols;
        let _ = unsafe { libc::ioctl(f.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
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
    fn reset_empties_dir() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("f"), "x").unwrap();
        fs::write(dir.path().join(".hidden"), "y").unwrap();
        reset_dir(dir.path().to_str().unwrap()).unwrap();
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn tar_roundtrip() {
        let src = tempfile::tempdir().unwrap();
        fs::write(src.path().join("hi"), "hello").unwrap();
        let dest = tempfile::tempdir().unwrap();
        let (mut a, b) = UnixStream::pair().unwrap();
        let dest_s = dest.path().to_string_lossy().into_owned();
        thread::spawn(move || {
            handle_socket(b).unwrap();
        });
        a.write_all(format!("TAR_IN {dest_s}\n").as_bytes())
            .unwrap();
        {
            let mut builder = tar::Builder::new(&mut a);
            builder.append_dir_all(".", src.path()).unwrap();
            builder.finish().unwrap();
        }
        a.shutdown(std::net::Shutdown::Write).unwrap();
        let mut reply = String::new();
        a.read_to_string(&mut reply).unwrap();
        assert!(reply.contains("OK"), "{reply}");
        assert_eq!(fs::read_to_string(dest.path().join("hi")).unwrap(), "hello");
    }
}
