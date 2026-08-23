//! Host-side Canvas Layout. Closing the browser does not forget it.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::sandbox::ActionError;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Window {
    pub id: Uuid,
    pub sandbox: Uuid,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub z: u32,
    #[serde(default)]
    pub iconified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IconManager {
    pub x: i32,
    pub y: i32,
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn default_true() -> bool {
    true
}

impl Default for IconManager {
    fn default() -> Self {
        Self {
            x: 8,
            y: 8,
            visible: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Layout {
    #[serde(default)]
    pub windows: Vec<Window>,
    #[serde(default)]
    pub icon_manager: IconManager,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            windows: Vec::new(),
            icon_manager: IconManager::default(),
        }
    }
}

pub struct LayoutStore {
    path: PathBuf,
    inner: Mutex<Layout>,
}

impl LayoutStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ActionError> {
        let path = path.into();
        let layout = if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|_| ActionError::Internal)?;
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|_| ActionError::Internal)?;
            }
            Layout::default()
        };
        Ok(Self {
            path,
            inner: Mutex::new(layout),
        })
    }

    pub fn get(&self) -> Layout {
        self.inner.lock().expect("layout").clone()
    }

    pub fn put(&self, layout: Layout) -> Result<Layout, ActionError> {
        {
            let mut inner = self.inner.lock().expect("layout");
            *inner = layout;
        }
        self.persist()?;
        Ok(self.get())
    }

    pub fn open_window(&self, sandbox: Uuid, title: String) -> Result<Window, ActionError> {
        let mut inner = self.inner.lock().expect("layout");
        let n = inner.windows.len() as i32;
        let z = inner.windows.iter().map(|w| w.z).max().unwrap_or(0) + 1;
        let win = Window {
            id: Uuid::new_v4(),
            sandbox,
            title,
            x: 200 + n * 24,
            y: 48 + n * 24,
            w: 640,
            h: 400,
            z,
            iconified: false,
        };
        inner.windows.push(win.clone());
        drop(inner);
        self.persist()?;
        Ok(win)
    }

    pub fn window(&self, id: Uuid) -> Result<Window, ActionError> {
        self.inner
            .lock()
            .expect("layout")
            .windows
            .iter()
            .find(|w| w.id == id)
            .cloned()
            .ok_or(ActionError::NotFound)
    }

    pub fn close_window(&self, id: Uuid) -> Result<(), ActionError> {
        {
            let mut inner = self.inner.lock().expect("layout");
            let before = inner.windows.len();
            inner.windows.retain(|w| w.id != id);
            if inner.windows.len() == before {
                return Err(ActionError::NotFound);
            }
        }
        self.persist()
    }

    fn persist(&self) -> Result<(), ActionError> {
        let inner = self.inner.lock().expect("layout");
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|_| ActionError::Internal)?;
        }
        let raw = serde_json::to_string_pretty(&*inner).map_err(|_| ActionError::Internal)?;
        fs::write(&self.path, raw).map_err(|_| ActionError::Internal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_window_persists_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        let sandbox = Uuid::new_v4();
        let store = LayoutStore::open(&path).unwrap();
        let win = store.open_window(sandbox, "xterm".into()).unwrap();
        assert_eq!(win.title, "xterm");
        assert_eq!(win.sandbox, sandbox);
        drop(store);

        let store = LayoutStore::open(&path).unwrap();
        let got = store.window(win.id).unwrap();
        assert_eq!(got, win);
    }

    #[test]
    fn close_window_is_gone_after_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        let store = LayoutStore::open(&path).unwrap();
        let win = store.open_window(Uuid::new_v4(), "xterm".into()).unwrap();
        store.close_window(win.id).unwrap();
        drop(store);
        let store = LayoutStore::open(&path).unwrap();
        assert!(matches!(
            store.window(win.id).unwrap_err(),
            ActionError::NotFound
        ));
        assert!(store.get().windows.is_empty());
    }
}
