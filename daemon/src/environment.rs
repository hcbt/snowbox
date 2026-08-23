//! Environment is a Host flake. Copied into each Sandbox dir; never /workspace.

pub const DEFAULT_FLAKE: &str = include_str!("../../environment/empty/flake.nix");
pub const DEFAULT_LOCK: &str = include_str!("../../environment/empty/flake.lock");
pub const DEFAULT_PACKAGES: &str = include_str!("../../environment/empty/packages.json");

use std::path::Path;

use crate::sandbox::ActionError;

pub fn write_default(dir: &Path) -> Result<(), ActionError> {
    let env_dir = dir.join("environment");
    std::fs::create_dir_all(&env_dir).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("flake.nix"), DEFAULT_FLAKE).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("flake.lock"), DEFAULT_LOCK).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("packages.json"), DEFAULT_PACKAGES.trim())
        .map_err(|_| ActionError::Internal)?;
    Ok(())
}

pub fn packages(dir: &Path) -> Result<Vec<String>, ActionError> {
    let raw = std::fs::read_to_string(dir.join("environment/packages.json"))
        .map_err(|_| ActionError::Internal)?;
    serde_json::from_str(&raw).map_err(|_| ActionError::Internal)
}

pub fn add_package(dir: &Path, name: &str) -> Result<Vec<String>, ActionError> {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        || name.is_empty()
    {
        return Err(ActionError::BadRequest("invalid package name"));
    }
    let mut pkgs = packages(dir)?;
    if !pkgs.iter().any(|p| p == name) {
        pkgs.push(name.to_string());
        pkgs.sort();
        let raw = serde_json::to_string_pretty(&pkgs).map_err(|_| ActionError::Internal)?;
        std::fs::write(dir.join("environment/packages.json"), raw)
            .map_err(|_| ActionError::Internal)?;
    }
    Ok(pkgs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_flake_is_nixos_26_05() {
        assert!(DEFAULT_FLAKE.contains("nixos-26.05"));
        assert!(!DEFAULT_FLAKE.to_lowercase().contains("nixos-unstable"));
    }

    #[test]
    fn write_default_creates_host_flake() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let flake = std::fs::read_to_string(dir.path().join("environment/flake.nix")).unwrap();
        assert!(flake.contains("Snowbox Environment"));
        assert!(dir.path().join("environment/flake.lock").is_file());
        let pkgs = packages(dir.path()).unwrap();
        assert!(pkgs.contains(&"hello".to_string()));
    }

    #[test]
    fn add_package_updates_json() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let pkgs = add_package(dir.path(), "jq").unwrap();
        assert!(pkgs.contains(&"jq".to_string()));
        let again = add_package(dir.path(), "jq").unwrap();
        assert_eq!(again.iter().filter(|p| *p == "jq").count(), 1);
    }
}
