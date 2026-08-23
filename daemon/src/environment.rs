//! Environment is a Host flake. Copied into each Sandbox dir; never /workspace.

pub const DEFAULT_FLAKE: &str = include_str!("../../environment/empty/flake.nix");
pub const DEFAULT_LOCK: &str = include_str!("../../environment/empty/flake.lock");
pub const DEFAULT_HOME: &str = include_str!("../../environment/empty/home.nix");
pub const DEFAULT_CONFIG: &str = include_str!("../../environment/empty/config.json");
pub const SCHEMA: &str = include_str!("../../environment/empty/schema.json");

const FILES: [&str; 4] = ["flake.nix", "flake.lock", "home.nix", "config.json"];

use std::path::Path;

use crate::sandbox::ActionError;

pub fn write_env_dir(dest: &Path, src: &Path) -> Result<(), ActionError> {
    std::fs::create_dir_all(dest).map_err(|_| ActionError::Internal)?;
    for f in FILES {
        std::fs::copy(src.join(f), dest.join(f)).map_err(|_| ActionError::Internal)?;
    }
    Ok(())
}

pub fn write_from_template(sandbox: &Path, template: &Path) -> Result<(), ActionError> {
    write_env_dir(&sandbox.join("environment"), template)
}

pub fn write_default(dir: &Path) -> Result<(), ActionError> {
    let env_dir = dir.join("environment");
    std::fs::create_dir_all(&env_dir).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("flake.nix"), DEFAULT_FLAKE).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("flake.lock"), DEFAULT_LOCK).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("home.nix"), DEFAULT_HOME).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("config.json"), DEFAULT_CONFIG.trim()).map_err(|_| ActionError::Internal)?;
    Ok(())
}

pub fn fingerprint(dir: &Path) -> Result<String, ActionError> {
    let cfg = std::fs::read_to_string(dir.join("environment/config.json"))
        .map_err(|_| ActionError::Internal)?;
    let lock = std::fs::read_to_string(dir.join("environment/flake.lock"))
        .map_err(|_| ActionError::Internal)?;
    Ok(format!("{cfg}\n{lock}"))
}

pub fn config(dir: &Path) -> Result<serde_json::Value, ActionError> {
    let raw = std::fs::read_to_string(dir.join("environment/config.json"))
        .map_err(|_| ActionError::Internal)?;
    serde_json::from_str(&raw).map_err(|_| ActionError::Internal)
}

pub fn set_config(dir: &Path, value: &serde_json::Value) -> Result<serde_json::Value, ActionError> {
    if !value.is_object() {
        return Err(ActionError::BadRequest("config must be an object"));
    }
    let raw = serde_json::to_string_pretty(value).map_err(|_| ActionError::Internal)?;
    std::fs::write(dir.join("environment/config.json"), raw).map_err(|_| ActionError::Internal)?;
    config(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_flake_is_home_manager() {
        assert!(DEFAULT_FLAKE.contains("home-manager"));
        assert!(DEFAULT_FLAKE.contains("nixpkgs-unstable"));
        assert!(DEFAULT_HOME.contains("pkgs.devenv"));
    }

    #[test]
    fn write_default_creates_host_flake() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let flake = std::fs::read_to_string(dir.path().join("environment/flake.nix")).unwrap();
        assert!(flake.contains("Snowbox Environment"));
        assert!(dir.path().join("environment/flake.lock").is_file());
        let cfg = config(dir.path()).unwrap();
        assert_eq!(cfg["programs"]["claude-code"]["enable"], false);
        let a = fingerprint(dir.path()).unwrap();
        let b = fingerprint(dir.path()).unwrap();
        assert_eq!(a, b);
        let mut next = cfg.clone();
        next["programs"]["claude-code"]["enable"] = serde_json::Value::Bool(true);
        set_config(dir.path(), &next).unwrap();
        assert_ne!(a, fingerprint(dir.path()).unwrap());
    }

    #[test]
    fn schema_names_home_manager_agents() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let names: Vec<&str> = schema["programs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"claude-code"));
        assert!(names.contains(&"codex"));
        assert!(names.contains(&"pi-coding-agent"));
    }
}
