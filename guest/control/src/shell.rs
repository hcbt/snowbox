//! Window PTY: one vsock connection is one login shell for `snow`.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

pub fn handle_socket(stream: UnixStream) -> io::Result<()> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    ws.ws_row = 24;
    ws.ws_col = 80;
    let (master, slave) = openpty(&ws)?;

    // util-linux: `runuser -u user` requires a command and cannot take --login.
    // `runuser -l user` is the su-compatible login form: shell from passwd.
    let mut child = unsafe {
        Command::new("runuser")
            .args(["-l", "snow"])
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave))
            .pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            })
            .spawn()?
    };

    let mut master_r = master.try_clone()?;
    let mut stream_r = stream.try_clone()?;
    let mut stream_w = stream;
    let up = std::thread::spawn(move || io::copy(&mut stream_r, &mut master_r));
    {
        let mut master_w = master;
        let _ = io::copy(&mut master_w, &mut stream_w);
    }
    let _ = up.join();
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

fn openpty(ws: &libc::winsize) -> io::Result<(File, File)> {
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if master < 0 {
        return Err(io::Error::last_os_error());
    }
    let master = unsafe { File::from(OwnedFd::from_raw_fd(master)) };
    if unsafe { libc::grantpt(master.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::unlockpt(master.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut name = [0u8; 64];
    if unsafe {
        libc::ptsname_r(
            master.as_raw_fd(),
            name.as_mut_ptr() as *mut libc::c_char,
            name.len(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    let slave_fd = unsafe { libc::open(name.as_ptr() as *const libc::c_char, libc::O_RDWR | libc::O_NOCTTY) };
    if slave_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let slave = unsafe { File::from(OwnedFd::from_raw_fd(slave_fd)) };
    let _ = unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, ws) };
    Ok((master, slave))
}
