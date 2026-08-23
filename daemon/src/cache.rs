//! Snowbox Cache: a Host store the Daemon owns. Guests copy from it.

use std::path::{Path, PathBuf};

use crate::sandbox::ActionError;

#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ActionError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|_| ActionError::Internal)?;
        // file:// binary-cache layout. Realization writes narinfo/nar here.
        std::fs::create_dir_all(root.join("nar")).map_err(|_| ActionError::Internal)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn substituter_uri(&self) -> String {
        format!("file://{}", self.root.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_file_cache_layout() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path().join("cache")).unwrap();
        assert!(cache.root().join("nar").is_dir());
        assert!(cache.substituter_uri().starts_with("file://"));
    }
}
