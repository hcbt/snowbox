//! Templates are flakes. The GUI noun is Template.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::environment;
use crate::sandbox::ActionError;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub shipped: bool,
}

pub struct Library {
    pub shipped: PathBuf,
    pub user: PathBuf,
}

impl Library {
    pub fn list(&self) -> Result<Vec<Template>, ActionError> {
        let mut out = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        for (root, shipped) in [(&self.user, false), (&self.shipped, true)] {
            if !root.is_dir() {
                continue;
            }
            for ent in fs::read_dir(root).map_err(|_| ActionError::Internal)? {
                let ent = ent.map_err(|_| ActionError::Internal)?;
                if !ent.file_type().map_err(|_| ActionError::Internal)?.is_dir() {
                    continue;
                }
                let name = ent.file_name().to_string_lossy().to_string();
                if !valid_name(&name) || names.contains(&name) {
                    continue;
                }
                if !ent.path().join("flake.nix").is_file() {
                    continue;
                }
                names.insert(name.clone());
                out.push(Template { name, shipped });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn resolve(&self, name: &str) -> Result<PathBuf, ActionError> {
        if !valid_name(name) {
            return Err(ActionError::BadRequest("invalid template name"));
        }
        let user = self.user.join(name);
        if user.join("flake.nix").is_file() {
            return Ok(user);
        }
        let shipped = self.shipped.join(name);
        if shipped.join("flake.nix").is_file() {
            return Ok(shipped);
        }
        Err(ActionError::NotFound)
    }

    pub fn save(&self, name: &str, env_dir: &Path) -> Result<Template, ActionError> {
        if !valid_name(name) {
            return Err(ActionError::BadRequest("invalid template name"));
        }
        if self.shipped.join(name).join("flake.nix").is_file() {
            return Err(ActionError::Conflict("shipped template"));
        }
        let dest = self.user.join(name);
        environment::write_env_dir(&dest, env_dir)?;
        Ok(Template {
            name: name.to_string(),
            shipped: false,
        })
    }

    pub fn config(&self, name: &str) -> Result<serde_json::Value, ActionError> {
        let dir = self.resolve(name)?;
        let raw = fs::read_to_string(dir.join("config.json")).map_err(|_| ActionError::Internal)?;
        serde_json::from_str(&raw).map_err(|_| ActionError::Internal)
    }

    pub fn set_config(
        &self,
        name: &str,
        value: &serde_json::Value,
    ) -> Result<serde_json::Value, ActionError> {
        if !valid_name(name) {
            return Err(ActionError::BadRequest("invalid template name"));
        }
        if self.shipped.join(name).join("flake.nix").is_file() {
            return Err(ActionError::Conflict("shipped template"));
        }
        let dest = self.user.join(name);
        if !dest.join("flake.nix").is_file() {
            return Err(ActionError::NotFound);
        }
        if !value.is_object() {
            return Err(ActionError::BadRequest("config must be an object"));
        }
        let raw = serde_json::to_string_pretty(value).map_err(|_| ActionError::Internal)?;
        fs::write(dest.join("config.json"), raw).map_err(|_| ActionError::Internal)?;
        self.config(name)
    }
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_list_user_template() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library {
            shipped: dir.path().join("shipped"),
            user: dir.path().join("user"),
        };
        fs::create_dir_all(&lib.shipped).unwrap();
        let src = tempfile::tempdir().unwrap();
        environment::write_default(src.path()).unwrap();
        let t = lib.save("work", &src.path().join("environment")).unwrap();
        assert!(!t.shipped);
        assert_eq!(t.name, "work");
        let list = lib.list().unwrap();
        assert!(list.iter().any(|x| x.name == "work" && !x.shipped));
        assert!(lib.resolve("work").unwrap().join("flake.nix").is_file());
    }

    #[test]
    fn shipped_environment_lists_empty() {
        let lib = Library {
            shipped: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../environment"),
            user: PathBuf::from("/tmp/snowbox-no-user-templates"),
        };
        let list = lib.list().unwrap();
        assert!(
            list.iter().any(|t| t.name == "empty" && t.shipped),
            "shipped templates: {list:?}"
        );
    }

    #[test]
    fn refuses_to_overwrite_shipped() {
        let dir = tempfile::tempdir().unwrap();
        let shipped = dir.path().join("shipped/empty");
        fs::create_dir_all(&shipped).unwrap();
        fs::write(shipped.join("flake.nix"), "{}").unwrap();
        let lib = Library {
            shipped: dir.path().join("shipped"),
            user: dir.path().join("user"),
        };
        let src = tempfile::tempdir().unwrap();
        environment::write_default(src.path()).unwrap();
        let err = lib
            .save("empty", &src.path().join("environment"))
            .unwrap_err();
        assert!(matches!(err, ActionError::Conflict(_)));
    }

    #[test]
    fn set_config_refuses_shipped_and_updates_user() {
        let dir = tempfile::tempdir().unwrap();
        let shipped = dir.path().join("shipped/empty");
        fs::create_dir_all(&shipped).unwrap();
        fs::write(shipped.join("flake.nix"), "{}").unwrap();
        fs::write(shipped.join("config.json"), "{}").unwrap();
        let lib = Library {
            shipped: dir.path().join("shipped"),
            user: dir.path().join("user"),
        };
        let src = tempfile::tempdir().unwrap();
        environment::write_default(src.path()).unwrap();
        lib.save("work", &src.path().join("environment")).unwrap();
        let err = lib
            .set_config("empty", &serde_json::json!({"programs":{}}))
            .unwrap_err();
        assert!(matches!(err, ActionError::Conflict(_)));
        let next = lib
            .set_config(
                "work",
                &serde_json::json!({"programs":{"claude-code":{"enable":true}}}),
            )
            .unwrap();
        assert_eq!(next["programs"]["claude-code"]["enable"], true);
        assert_eq!(
            lib.config("work").unwrap()["programs"]["claude-code"]["enable"],
            true
        );
    }
}
