//! Publish maps a Sandbox port onto 127.0.0.1 on the Host.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

use crate::sandbox::ActionError;
use crate::vz::{self, Hypervisor};

#[derive(Clone, Debug, Serialize)]
pub struct Mapping {
    pub port: u16,
    pub host_port: u16,
    pub url: String,
}

struct Live {
    mapping: Mapping,
    abort: tokio::sync::watch::Sender<bool>,
}

#[derive(Clone, Default)]
pub struct Publisher {
    inner: Arc<Mutex<HashMap<(Uuid, u16), Live>>>,
}

impl Publisher {
    pub fn list(&self, sandbox: Uuid) -> Vec<Mapping> {
        let map = self.inner.lock().expect("publish");
        let mut v: Vec<_> = map
            .iter()
            .filter(|((id, _), _)| *id == sandbox)
            .map(|(_, l)| l.mapping.clone())
            .collect();
        v.sort_by_key(|m| m.port);
        v
    }

    pub fn drop_sandbox(&self, sandbox: Uuid) {
        let mut map = self.inner.lock().expect("publish");
        let keys: Vec<_> = map
            .keys()
            .filter(|(id, _)| *id == sandbox)
            .cloned()
            .collect();
        for k in keys {
            if let Some(live) = map.remove(&k) {
                let _ = live.abort.send(true);
            }
        }
    }

    pub async fn publish(
        &self,
        vmm: Arc<Hypervisor>,
        sandbox: Uuid,
        port: u16,
        host_port: Option<u16>,
    ) -> Result<Mapping, ActionError> {
        if port == 0 {
            return Err(ActionError::BadRequest("port must be at least 1"));
        }
        {
            let map = self.inner.lock().expect("publish");
            if map.contains_key(&(sandbox, port)) {
                return Err(ActionError::Conflict("already published"));
            }
        }
        let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), host_port.unwrap_or(port));
        let listener = TcpListener::bind(bind)
            .await
            .map_err(|e| ActionError::Failed(format!("bind 127.0.0.1: {e}")))?;
        let bound = listener
            .local_addr()
            .map_err(|e| ActionError::Failed(e.to_string()))?;
        if !bound.ip().is_loopback() {
            return Err(ActionError::Failed("refused non-loopback bind".into()));
        }
        let mapping = Mapping {
            port,
            host_port: bound.port(),
            url: format!("http://127.0.0.1:{}", bound.port()),
        };
        let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let vmm_loop = vmm;
        let mut stop = abort_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.changed() => break,
                    accepted = listener.accept() => {
                        let Ok((incoming, _)) = accepted else { break };
                        let vmm = vmm_loop.clone();
                        tokio::task::spawn_blocking(move || proxy(vmm, sandbox, port, incoming));
                    }
                }
            }
        });
        let mut map = self.inner.lock().expect("publish");
        map.insert(
            (sandbox, port),
            Live {
                mapping: mapping.clone(),
                abort: abort_tx,
            },
        );
        Ok(mapping)
    }

    pub fn unpublish(&self, sandbox: Uuid, port: u16) -> Result<Mapping, ActionError> {
        let mut map = self.inner.lock().expect("publish");
        let live = map.remove(&(sandbox, port)).ok_or(ActionError::NotFound)?;
        let _ = live.abort.send(true);
        Ok(live.mapping)
    }
}

fn proxy(vmm: Arc<Hypervisor>, sandbox: Uuid, port: u16, incoming: TcpStream) {
    let Ok(mut host) = incoming.into_std() else {
        return;
    };
    let _ = host.set_nonblocking(false);
    let Ok(mut guest) = vmm.vsock(sandbox, vz::AGENT_PORT) else {
        return;
    };
    if guest
        .write_all(format!("CONNECT {port}\n").as_bytes())
        .is_err()
    {
        return;
    }
    let Ok(mut host2) = host.try_clone() else {
        return;
    };
    let Ok(mut guest2) = guest.try_clone() else {
        return;
    };
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut host2, &mut guest2);
        let _ = guest2.shutdown(std::net::Shutdown::Both);
    });
    let _ = std::io::copy(&mut guest, &mut host);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_url_is_loopback() {
        let m = Mapping {
            port: 3000,
            host_port: 3000,
            url: "http://127.0.0.1:3000".into(),
        };
        assert!(m.url.starts_with("http://127.0.0.1:"));
    }
}
