use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Stopped,
    Running,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Sandbox {
    pub id: Uuid,
    pub name: String,
    pub state: State,
}

#[derive(Debug)]
pub enum ActionError {
    NotFound,
    Conflict(&'static str),
}

pub struct Store {
    inner: Mutex<HashMap<Uuid, Sandbox>>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn list(&self) -> Vec<Sandbox> {
        let map = self.inner.lock().expect("sandbox store");
        let mut v: Vec<_> = map.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        v
    }

    pub fn create(&self, name: Option<String>) -> Sandbox {
        let id = Uuid::new_v4();
        let name = match name {
            Some(n) if !n.trim().is_empty() => n,
            _ => format!("sandbox-{}", &id.to_string()[..8]),
        };
        let sandbox = Sandbox {
            id,
            name,
            state: State::Stopped,
        };
        self.inner
            .lock()
            .expect("sandbox store")
            .insert(id, sandbox.clone());
        sandbox
    }

    pub fn get(&self, id: Uuid) -> Result<Sandbox, ActionError> {
        self.inner
            .lock()
            .expect("sandbox store")
            .get(&id)
            .cloned()
            .ok_or(ActionError::NotFound)
    }

    pub fn start(&self, id: Uuid) -> Result<Sandbox, ActionError> {
        self.set_state(id, State::Stopped, State::Running, "already running")
    }

    pub fn stop(&self, id: Uuid) -> Result<Sandbox, ActionError> {
        self.set_state(id, State::Running, State::Stopped, "already stopped")
    }

    pub fn reset(&self, id: Uuid) -> Result<Sandbox, ActionError> {
        self.get(id)
    }

    pub fn destroy(&self, id: Uuid) -> Result<(), ActionError> {
        let mut map = self.inner.lock().expect("sandbox store");
        map.remove(&id).map(|_| ()).ok_or(ActionError::NotFound)
    }

    fn set_state(
        &self,
        id: Uuid,
        from: State,
        to: State,
        conflict: &'static str,
    ) -> Result<Sandbox, ActionError> {
        let mut map = self.inner.lock().expect("sandbox store");
        let sandbox = map.get_mut(&id).ok_or(ActionError::NotFound)?;
        if sandbox.state != from {
            return Err(ActionError::Conflict(conflict));
        }
        sandbox.state = to;
        Ok(sandbox.clone())
    }
}
