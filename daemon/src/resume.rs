//! Which Sandboxes were running when the Daemon last quit, so Quit can
//! write machine state for those guests. The Daemon does not auto-start
//! this set. User Start restores that Sandbox's saved machine state.
//! User Stop drops the id from the set.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use uuid::Uuid;

pub struct Resume {
    path: PathBuf,
    inner: Mutex<HashSet<Uuid>>,
}

impl Resume {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let ids = read(&path);
        Self {
            path,
            inner: Mutex::new(ids),
        }
    }

    pub fn ids(&self) -> Vec<Uuid> {
        let mut v: Vec<_> = self.inner.lock().expect("resume").iter().copied().collect();
        v.sort();
        v
    }

    pub fn mark(&self, id: Uuid) {
        self.inner.lock().expect("resume").insert(id);
        self.persist();
    }

    pub fn unmark(&self, id: Uuid) {
        self.inner.lock().expect("resume").remove(&id);
        self.persist();
    }

    /// Drop IDs whose Sandboxes no longer exist. Persist if anything changed.
    pub fn prune(&self, live: impl IntoIterator<Item = Uuid>) {
        let live: HashSet<Uuid> = live.into_iter().collect();
        let mut ids = self.inner.lock().expect("resume");
        let before = ids.len();
        ids.retain(|id| live.contains(id));
        if ids.len() == before {
            return;
        }
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let raw = serde_json::to_string_pretty(&*ids).unwrap_or_else(|_| "[]".into());
        let _ = fs::write(&self.path, raw);
    }

    fn persist(&self) {
        let ids = self.inner.lock().expect("resume");
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let raw = serde_json::to_string_pretty(&*ids).unwrap_or_else(|_| "[]".into());
        let _ = fs::write(&self.path, raw);
    }
}

fn read(path: &std::path::Path) -> HashSet<Uuid> {
    let Ok(raw) = fs::read_to_string(path) else {
        return HashSet::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_survive_a_new_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("running.json");
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let resume = Resume::open(&path);
        resume.mark(a);
        resume.mark(b);
        resume.unmark(a);
        drop(resume);

        let resume = Resume::open(&path);
        assert_eq!(resume.ids(), vec![b]);
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let resume = Resume::open(dir.path().join("running.json"));
        assert!(resume.ids().is_empty());
    }

    #[test]
    fn prune_drops_ids_that_are_gone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("running.json");
        let keep = Uuid::from_u128(1);
        let gone = Uuid::from_u128(2);
        let resume = Resume::open(&path);
        resume.mark(keep);
        resume.mark(gone);
        resume.prune([keep]);
        drop(resume);

        let resume = Resume::open(&path);
        assert_eq!(resume.ids(), vec![keep]);
    }
}
