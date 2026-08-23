//! Host side of the vsock control plane.

use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::vmm::{AGENT_PORT, Control};

pub fn wait_ready(vmm: &impl Control, id: Uuid, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut last = "not tried".to_string();
    while Instant::now() < deadline {
        match ping(vmm, id) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(format!("agent not ready: {last}"))
}

pub fn ping(vmm: &impl Control, id: Uuid) -> Result<(), String> {
    let mut stream = vmm.vsock(id, AGENT_PORT)?;
    stream
        .write_all(b"PING\n")
        .map_err(|e| format!("ping write: {e}"))?;
    let mut buf = [0u8; 16];
    let n = stream
        .read(&mut buf)
        .map_err(|e| format!("ping read: {e}"))?;
    let got = std::str::from_utf8(&buf[..n]).unwrap_or("");
    if got.contains("PONG") {
        Ok(())
    } else {
        Err(format!("unexpected ping reply: {got:?}"))
    }
}

pub fn tar_in(vmm: &impl Control, id: Uuid, dest: &str, from: &Path) -> Result<(), String> {
    if !from.exists() {
        return Ok(());
    }
    if from.is_dir()
        && std::fs::read_dir(from)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true)
    {
        return Ok(());
    }
    let mut tar = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar);
        builder
            .append_dir_all(".", from)
            .map_err(|e| format!("tar: {e}"))?;
        builder.finish().map_err(|e| format!("tar finish: {e}"))?;
    }
    let mut stream = vmm.vsock(id, AGENT_PORT)?;
    let header = format!("TAR_IN {dest}\n");
    stream
        .write_all(header.as_bytes())
        .map_err(|e| format!("tar_in header: {e}"))?;
    stream
        .write_all(&tar)
        .map_err(|e| format!("tar_in body: {e}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("tar_in shutdown: {e}"))?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .map_err(|e| format!("tar_in reply: {e}"))?;
    if reply.contains("OK") {
        Ok(())
    } else {
        Err(format!("tar_in: {reply}"))
    }
}

pub fn tar_out(vmm: &impl Control, id: Uuid, src: &str, to: &Path) -> Result<(), String> {
    let mut stream = vmm.vsock(id, AGENT_PORT)?;
    let header = format!("TAR_OUT {src}\n");
    stream
        .write_all(header.as_bytes())
        .map_err(|e| format!("tar_out header: {e}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("tar_out shutdown: {e}"))?;
    if to.exists() {
        std::fs::remove_dir_all(to).map_err(|e| format!("clear dest: {e}"))?;
    }
    std::fs::create_dir_all(to).map_err(|e| format!("mkdir dest: {e}"))?;
    let mut archive = tar::Archive::new(stream);
    archive.unpack(to).map_err(|e| format!("untar: {e}"))?;
    Ok(())
}

pub fn nar_in(vmm: &impl Control, id: Uuid, export: &[u8]) -> Result<(), String> {
    let mut stream = vmm.vsock(id, AGENT_PORT)?;
    stream
        .write_all(b"NAR_IN\n")
        .map_err(|e| format!("nar_in header: {e}"))?;
    stream
        .write_all(export)
        .map_err(|e| format!("nar_in body: {e}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("nar_in shutdown: {e}"))?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .map_err(|e| format!("nar_in reply: {e}"))?;
    if reply.contains("OK") {
        Ok(())
    } else {
        Err(format!("nar_in: {reply}"))
    }
}

pub fn profile(vmm: &impl Control, id: Uuid, store_path: &str) -> Result<(), String> {
    let mut stream = vmm.vsock(id, AGENT_PORT)?;
    let header = format!("PROFILE {store_path}\n");
    stream
        .write_all(header.as_bytes())
        .map_err(|e| format!("profile write: {e}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("profile shutdown: {e}"))?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .map_err(|e| format!("profile reply: {e}"))?;
    if reply.contains("OK") {
        Ok(())
    } else {
        Err(format!("profile: {reply}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::thread;

    struct Pair(UnixStream);

    impl Control for Pair {
        fn vsock(&self, _id: Uuid, _port: u32) -> Result<UnixStream, String> {
            self.0.try_clone().map_err(|e| e.to_string())
        }
    }

    #[test]
    fn ping_reads_pong_through_control() {
        let (a, b) = UnixStream::pair().unwrap();
        thread::spawn(move || {
            let mut b = b;
            let mut buf = [0u8; 16];
            let n = b.read(&mut buf).unwrap();
            assert!(std::str::from_utf8(&buf[..n]).unwrap().starts_with("PING"));
            b.write_all(b"PONG\n").unwrap();
        });
        ping(&Pair(a), Uuid::nil()).unwrap();
    }
}
