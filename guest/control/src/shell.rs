//! Window PTY: one vsock connection is one login shell for `snow`.

use std::path::Path;

/// argv0 for a login shell: a leading hyphen plus the basename of `pw_shell`.
pub fn login_argv0(shell: &str) -> String {
    let base = Path::new(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("bash");
    format!("-{base}")
}

#[cfg(target_os = "linux")]
mod linux {
    use super::login_argv0;
    use crate::frame::{self, HostFrame};
    use std::ffi::{CStr, CString};
    use std::fs::File;
    use std::io::{self, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::net::UnixStream;

    pub fn handle_socket(stream: UnixStream) -> io::Result<()> {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        ws.ws_row = 24;
        ws.ws_col = 80;
        let (master, slave) = openpty(&ws)?;

        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(io::Error::last_os_error());
        }
        if pid == 0 {
            drop(master);
            drop(stream);
            child_login(slave);
        }

        drop(slave);
        parent_copy(stream, master)?;
        let _ = unsafe { libc::kill(pid, libc::SIGHUP) };
        let mut status = 0;
        let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
        Ok(())
    }

    fn parent_copy(stream: UnixStream, mut master: File) -> io::Result<()> {
        let mut master_out = master.try_clone()?;
        let mut stream_in = stream.try_clone()?;
        let mut stream_out = stream;
        let up = std::thread::spawn(move || {
            let _ = frame::copy_stdin_frames(&mut stream_in, |f| match f {
                HostFrame::Stdin(b) => master.write_all(&b),
                HostFrame::Winsize { rows, cols } => set_winsize(&master, rows, cols),
            });
        });
        let _ = io::copy(&mut master_out, &mut stream_out);
        let _ = stream_out.shutdown(std::net::Shutdown::Both);
        let _ = up.join();
        Ok(())
    }

    fn child_login(slave: File) -> ! {
        let fd = slave.as_raw_fd();
        if unsafe { libc::login_tty(fd) } != 0 {
            let err = io::Error::last_os_error();
            drop(slave);
            eprintln!("snowbox-shell: login_tty: {err}");
            unsafe { libc::_exit(1) };
        }
        // login_tty dup2s onto 0/1/2 and closes fd when fd > 2.
        std::mem::forget(slave);
        if let Err(e) = exec_login_shell() {
            eprintln!("snowbox-shell: {e}");
        }
        unsafe { libc::_exit(1) };
    }

    fn exec_login_shell() -> io::Result<()> {
        let user_c = CString::new("snow").unwrap();
        let pw = unsafe { libc::getpwnam(user_c.as_ptr()) };
        if pw.is_null() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no such user snow"));
        }
        let pw = unsafe { &*pw };
        let uid = pw.pw_uid;
        let gid = pw.pw_gid;
        let home = unsafe { CStr::from_ptr(pw.pw_dir) }.to_owned();
        let shell = unsafe { CStr::from_ptr(pw.pw_shell) }.to_owned();
        let name = unsafe { CStr::from_ptr(pw.pw_name) }.to_owned();

        if unsafe { libc::initgroups(name.as_ptr(), gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::setresgid(gid, gid, gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::setresuid(uid, uid, uid) } != 0 {
            return Err(io::Error::last_os_error());
        }

        unsafe {
            libc::clearenv();
        }
        setenv("HOME", home.as_c_str())?;
        setenv("USER", name.as_c_str())?;
        setenv("LOGNAME", name.as_c_str())?;
        setenv("SHELL", shell.as_c_str())?;
        setenv_bytes(
            "PATH",
            b"/run/wrappers/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin",
        )?;
        setenv_bytes("TERM", b"xterm-256color")?;
        setenv_bytes("COLORTERM", b"truecolor")?;
        setenv_bytes("LANG", b"C.UTF-8")?;

        if unsafe { libc::chdir(home.as_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }

        let shell_str = shell
            .to_str()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "pw_shell"))?;
        let argv0 = CString::new(login_argv0(shell_str))?;
        let argv = [argv0.as_ptr(), std::ptr::null()];
        unsafe {
            libc::execve(shell.as_ptr(), argv.as_ptr(), environ());
        }
        Err(io::Error::last_os_error())
    }

    fn setenv(key: &str, val: &CStr) -> io::Result<()> {
        let k = CString::new(key)?;
        if unsafe { libc::setenv(k.as_ptr(), val.as_ptr(), 1) } != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn setenv_bytes(key: &str, val: &[u8]) -> io::Result<()> {
        let v = CString::new(val)?;
        setenv(key, v.as_c_str())
    }

    fn environ() -> *const *const libc::c_char {
        unsafe { environ_ptr() }
    }

    unsafe extern "C" {
        static environ: *const *const libc::c_char;
    }

    unsafe fn environ_ptr() -> *const *const libc::c_char {
        environ
    }

    fn set_winsize(master: &File, rows: u16, cols: u16) -> io::Result<()> {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        ws.ws_row = rows;
        ws.ws_col = cols;
        if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &ws) } != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
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
        let slave_fd = unsafe {
            libc::open(
                name.as_ptr() as *const libc::c_char,
                libc::O_RDWR | libc::O_NOCTTY,
            )
        };
        if slave_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let slave = unsafe { File::from(OwnedFd::from_raw_fd(slave_fd)) };
        let _ = unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, ws) };
        Ok((master, slave))
    }
}

#[cfg(target_os = "linux")]
pub use linux::handle_socket;

#[cfg(test)]
mod tests {
    use super::login_argv0;

    #[test]
    fn login_argv0_is_hyphen_bash() {
        assert_eq!(login_argv0("/run/current-system/sw/bin/bash"), "-bash");
        assert_eq!(login_argv0("/nix/store/eeee-bash-5.2/bin/bash"), "-bash");
    }
}
