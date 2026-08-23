//! AF_VSOCK listener. Accepted fds are `UnixStream` (SOCK_STREAM).

use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

use libc::{sockaddr, sockaddr_vm, socklen_t, AF_VSOCK, SOCK_STREAM, VMADDR_CID_ANY};

pub struct VsockListener {
    fd: OwnedFd,
}

impl VsockListener {
    pub fn bind(port: u32) -> io::Result<Self> {
        let fd = unsafe { libc::socket(AF_VSOCK, SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let yes: libc::c_int = 1;
        unsafe {
            libc::setsockopt(
                raw(&fd),
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &yes as *const _ as *const _,
                std::mem::size_of_val(&yes) as socklen_t,
            );
        }
        let mut addr: sockaddr_vm = unsafe { std::mem::zeroed() };
        addr.svm_family = AF_VSOCK as libc::sa_family_t;
        addr.svm_cid = VMADDR_CID_ANY;
        addr.svm_port = port;
        let rc = unsafe {
            libc::bind(
                raw(&fd),
                &addr as *const _ as *const sockaddr,
                std::mem::size_of::<sockaddr_vm>() as socklen_t,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::listen(raw(&fd), 128) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }

    pub fn accept(&self) -> io::Result<UnixStream> {
        let mut addr: sockaddr_vm = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<sockaddr_vm>() as socklen_t;
        let cfd = unsafe {
            libc::accept(
                raw(&self.fd),
                &mut addr as *mut _ as *mut sockaddr,
                &mut len,
            )
        };
        if cfd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { UnixStream::from_raw_fd(cfd) })
    }
}

fn raw(fd: &OwnedFd) -> RawFd {
    use std::os::fd::AsRawFd;
    fd.as_raw_fd()
}
