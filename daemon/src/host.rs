//! Host id: created on first start, stored next to the Cache.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use uuid::Uuid;

pub fn load_or_create(path: &Path) -> Result<Uuid> {
    if path.exists() {
        let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let v: serde_json::Value =
            serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
        if let Some(id) = v
            .get("id")
            .and_then(|x| x.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            return Ok(id);
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let id = Uuid::new_v4();
    let body = serde_json::json!({ "id": id }).to_string();
    fs::write(path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_reuses_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host.json");
        let a = load_or_create(&path).unwrap();
        let b = load_or_create(&path).unwrap();
        assert_eq!(a, b);
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains(&a.to_string()));
    }
}
