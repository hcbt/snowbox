//! LAN presence of other Daemons. The Canvas intersects this list with its
//! roster. Advertising never Attaches.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use uuid::Uuid;

use crate::api::LISTEN_PORT;

const SERVICE: &str = "_snowbox._tcp.local.";

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FoundHost {
    pub id: Uuid,
    pub addresses: Vec<String>,
    pub port: u16,
}

#[derive(Clone)]
pub struct Discovery {
    pub id: Uuid,
    inner: Arc<Mutex<HashMap<Uuid, FoundHost>>>,
}

impl Discovery {
    pub fn start(id: Uuid) -> Self {
        let inner = Arc::new(Mutex::new(HashMap::new()));
        spawn_mdns(id, inner.clone());
        Self { id, inner }
    }

    pub fn empty(id: Uuid) -> Self {
        Self {
            id,
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn seed(&self, found: FoundHost) {
        self.inner
            .lock()
            .expect("discovery")
            .insert(found.id, found);
    }

    pub fn list(&self) -> Vec<FoundHost> {
        self.inner
            .lock()
            .expect("discovery")
            .values()
            .filter(|h| h.id != self.id)
            .cloned()
            .collect()
    }
}

fn spawn_mdns(id: Uuid, inner: Arc<Mutex<HashMap<Uuid, FoundHost>>>) {
    std::thread::Builder::new()
        .name("snowbox-discovery".into())
        .spawn(move || {
            if let Err(e) = run_mdns(id, inner) {
                eprintln!("discovery {e}");
            }
        })
        .ok();
}

fn run_mdns(id: Uuid, inner: Arc<Mutex<HashMap<Uuid, FoundHost>>>) -> Result<(), String> {
    let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
    let hostname = format!("{id}.local.");
    let props = [("id", id.to_string())];
    let info = mdns_sd::ServiceInfo::new(
        SERVICE,
        &id.to_string(),
        &hostname,
        "",
        LISTEN_PORT,
        &props[..],
    )
    .map_err(|e| e.to_string())?;
    mdns.register(info).map_err(|e| e.to_string())?;
    let rx = mdns.browse(SERVICE).map_err(|e| e.to_string())?;
    while let Ok(event) = rx.recv() {
        match event {
            mdns_sd::ServiceEvent::ServiceResolved(info) => {
                let Some(found) = found_from(&info) else {
                    continue;
                };
                if found.id == id {
                    continue;
                }
                inner.lock().expect("discovery").insert(found.id, found);
            }
            mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                if let Some(found) = parse_instance(&fullname) {
                    inner.lock().expect("discovery").remove(&found);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn found_from(info: &mdns_sd::ServiceInfo) -> Option<FoundHost> {
    let id = info
        .get_property_val_str("id")
        .and_then(|s| Uuid::parse_str(s).ok())
        .or_else(|| Uuid::parse_str(info.get_hostname().trim_end_matches(".local.")).ok())?;
    let mut addresses: Vec<String> = info.get_addresses().iter().map(|a| a.to_string()).collect();
    addresses.sort();
    Some(FoundHost {
        id,
        addresses,
        port: info.get_port(),
    })
}

fn parse_instance(fullname: &str) -> Option<Uuid> {
    let inst = fullname.split('.').next()?;
    Uuid::parse_str(inst).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_omits_self() {
        let id = Uuid::nil();
        let d = Discovery::empty(id);
        d.seed(FoundHost {
            id,
            addresses: vec!["127.0.0.1".into()],
            port: 5418,
        });
        let other = Uuid::from_u128(1);
        d.seed(FoundHost {
            id: other,
            addresses: vec!["10.0.0.2".into()],
            port: 5418,
        });
        let list = d.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, other);
    }
}
