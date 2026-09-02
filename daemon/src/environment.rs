//! Environment is a Host flake. Copied into each Sandbox dir; never /workspace.

pub const DEFAULT_FLAKE: &str = include_str!("../../environment/empty/flake.nix");
pub const DEFAULT_LOCK: &str = include_str!("../../environment/empty/flake.lock");
pub const DEFAULT_HOME: &str = include_str!("../../environment/empty/home.nix");
pub const DEFAULT_CONFIG: &str = include_str!("../../environment/empty/config.json");
pub const DEFAULT_FORM: &str = include_str!("../../environment/empty/form.json");

const FILES: [&str; 4] = ["flake.nix", "flake.lock", "home.nix", "config.json"];
pub const CREATE_DIR: &str = "create-environment";

use std::path::Path;

use crate::sandbox::ActionError;

pub fn write_env_dir(dest: &Path, src: &Path) -> Result<(), ActionError> {
    std::fs::create_dir_all(dest).map_err(|_| ActionError::Internal)?;
    for f in FILES {
        let from = src.join(f);
        if from.is_file() {
            std::fs::copy(&from, dest.join(f)).map_err(|_| ActionError::Internal)?;
        }
    }
    Ok(())
}

pub fn write_from_template(sandbox: &Path, template: &Path) -> Result<(), ActionError> {
    write_env_dir(&sandbox.join("environment"), template)
}

pub fn snapshot_create(sandbox: &Path) -> Result<(), ActionError> {
    write_env_dir(&sandbox.join(CREATE_DIR), &sandbox.join("environment"))
}

pub fn restore_create(sandbox: &Path) -> Result<(), ActionError> {
    let src = sandbox.join(CREATE_DIR);
    if !src.join("flake.nix").is_file() {
        return Ok(());
    }
    write_env_dir(&sandbox.join("environment"), &src)
}

pub fn write_default(dir: &Path) -> Result<(), ActionError> {
    let env_dir = dir.join("environment");
    std::fs::create_dir_all(&env_dir).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("flake.nix"), DEFAULT_FLAKE).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("flake.lock"), DEFAULT_LOCK).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("home.nix"), DEFAULT_HOME).map_err(|_| ActionError::Internal)?;
    std::fs::write(env_dir.join("config.json"), DEFAULT_CONFIG.trim())
        .map_err(|_| ActionError::Internal)?;
    Ok(())
}

pub fn fingerprint(dir: &Path) -> Result<String, ActionError> {
    let env = dir.join("environment");
    let cfg = std::fs::read(env.join("config.json")).map_err(|_| ActionError::Internal)?;
    let lock = std::fs::read(env.join("flake.lock")).map_err(|_| ActionError::Internal)?;
    let home = std::fs::read(env.join("home.nix")).map_err(|_| ActionError::Internal)?;
    let flake = std::fs::read(env.join("flake.nix")).map_err(|_| ActionError::Internal)?;
    let mut out = Vec::new();
    out.extend_from_slice(&cfg);
    out.push(b'\n');
    out.extend_from_slice(&lock);
    out.push(b'\n');
    out.extend_from_slice(&home);
    out.push(b'\n');
    out.extend_from_slice(&flake);
    Ok(String::from_utf8_lossy(&out).into_owned())
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
    let value = sanitize(&value)?;
    let raw = serde_json::to_string_pretty(&value).map_err(|_| ActionError::Internal)?;
    std::fs::write(dir.join("environment/config.json"), raw).map_err(|_| ActionError::Internal)?;
    config(dir)
}

pub fn sanitize(value: &serde_json::Value) -> Result<serde_json::Value, ActionError> {
    if !value.is_object() {
        return Err(ActionError::BadRequest("config must be an object"));
    }
    let mut value = value.clone();
    clamp_programs(&mut value);
    check_extra_packages(&value)?;
    Ok(value)
}

pub fn load_agent_schema() -> Result<serde_json::Value, String> {
    let value: serde_json::Value = serde_json::from_str(DEFAULT_FORM).map_err(|e| e.to_string())?;
    let Some(programs) = value.get("programs").and_then(|p| p.as_array()) else {
        return Err("form.json missing programs".into());
    };
    if programs.is_empty() {
        return Err("form.json has no programs".into());
    }
    for p in programs {
        let Some(name) = p.get("name").and_then(|n| n.as_str()) else {
            return Err("form.json program missing name".into());
        };
        if name.is_empty() {
            return Err("form.json program missing name".into());
        }
        if p.get("options").and_then(|o| o.as_array()).is_none() {
            return Err("form.json program missing options".into());
        }
    }
    Ok(value)
}

pub fn agent_names() -> Vec<String> {
    load_agent_schema()
        .ok()
        .and_then(|v| v.get("programs").cloned())
        .and_then(|p| p.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|p| p.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect()
}

fn nix_attr(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\'' || c == '-')
}

fn clamp_programs(value: &mut serde_json::Value) {
    let allowed = agent_names();
    if allowed.is_empty() {
        return;
    }
    let Some(programs) = value.get_mut("programs").and_then(|p| p.as_object_mut()) else {
        return;
    };
    programs.retain(|k, _| allowed.iter().any(|a| a == k));
}

fn check_extra_packages(value: &serde_json::Value) -> Result<(), ActionError> {
    let Some(programs) = value.get("programs").and_then(|p| p.as_object()) else {
        return Ok(());
    };
    for cfg in programs.values() {
        let Some(extra) = cfg.get("extraPackages") else {
            continue;
        };
        let Some(arr) = extra.as_array() else {
            return Err(ActionError::BadRequest(
                "extraPackages must be an array of package names",
            ));
        };
        for item in arr {
            let Some(name) = item.as_str() else {
                return Err(ActionError::BadRequest(
                    "extraPackages entries must be strings",
                ));
            };
            if !nix_attr(name) {
                return Err(ActionError::BadRequest("invalid package name"));
            }
        }
    }
    Ok(())
}

pub fn document(dir: &Path) -> Result<serde_json::Value, ActionError> {
    config(dir)
}

pub fn set_document(
    dir: &Path,
    value: &serde_json::Value,
) -> Result<serde_json::Value, ActionError> {
    let mut body = value.clone();
    if let Some(obj) = body.as_object_mut() {
        obj.remove("env");
    }
    set_config(dir, &body)?;
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
        assert!(!dir.path().join("environment/agents.json").is_file());
        assert!(!dir.path().join("environment/form.json").is_file());
        let a = fingerprint(dir.path()).unwrap();
        let b = fingerprint(dir.path()).unwrap();
        assert_eq!(a, b);
        let mut next = cfg.clone();
        next["programs"]["claude-code"]["enable"] = serde_json::Value::Bool(true);
        set_config(dir.path(), &next).unwrap();
        assert_ne!(a, fingerprint(dir.path()).unwrap());
    }

    #[test]
    fn fingerprint_changes_when_home_nix_changes() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let a = fingerprint(dir.path()).unwrap();
        std::fs::write(dir.path().join("environment/home.nix"), "changed home\n").unwrap();
        assert_ne!(a, fingerprint(dir.path()).unwrap());
    }

    #[test]
    fn fingerprint_changes_when_flake_nix_changes() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let a = fingerprint(dir.path()).unwrap();
        std::fs::write(dir.path().join("environment/flake.nix"), "# changed\n").unwrap();
        assert_ne!(a, fingerprint(dir.path()).unwrap());
    }

    #[test]
    fn schema_names_home_manager_agents() {
        let names = agent_names();
        assert!(names.iter().any(|n| n == "claude-code"));
        assert!(names.iter().any(|n| n == "codex"));
        assert!(names.iter().any(|n| n == "pi-coding-agent"));
        let schema = load_agent_schema().unwrap();
        let pi = schema["programs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "pi-coding-agent")
            .unwrap();
        let extra = pi["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "extraPackages")
            .unwrap();
        assert_eq!(extra["type"], "packageNames");
        assert_eq!(extra["default"], serde_json::json!([]));
    }

    #[test]
    fn snapshot_create_is_restored() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        snapshot_create(dir.path()).unwrap();
        let mut next = config(dir.path()).unwrap();
        next["programs"]["claude-code"]["enable"] = serde_json::Value::Bool(true);
        set_config(dir.path(), &next).unwrap();
        assert_eq!(
            config(dir.path()).unwrap()["programs"]["claude-code"]["enable"],
            true
        );
        restore_create(dir.path()).unwrap();
        assert_eq!(
            config(dir.path()).unwrap()["programs"]["claude-code"]["enable"],
            false
        );
    }

    #[test]
    fn document_drops_env() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let mut body = config(dir.path()).unwrap();
        body["env"] = serde_json::json!({ "ANTHROPIC_API_KEY": "sk-test" });
        set_document(dir.path(), &body).unwrap();
        assert!(config(dir.path()).unwrap().get("env").is_none());
        assert!(document(dir.path()).unwrap().get("env").is_none());
        assert!(!dir.path().join("secrets.json").is_file());
        assert!(!dir.path().join("home/.snowbox-env").is_file());
    }

    #[test]
    fn extra_packages_invalid_name_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let mut cfg = config(dir.path()).unwrap();
        cfg["programs"]["pi-coding-agent"]["extraPackages"] =
            serde_json::json!(["hello", "not a package"]);
        let err = set_config(dir.path(), &cfg).unwrap_err();
        assert!(matches!(
            err,
            ActionError::BadRequest("invalid package name")
        ));
    }

    #[test]
    fn extra_packages_valid_names_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let mut cfg = config(dir.path()).unwrap();
        cfg["programs"]["pi-coding-agent"]["extraPackages"] = serde_json::json!(["hello", "git"]);
        set_config(dir.path(), &cfg).unwrap();
        let out = config(dir.path()).unwrap();
        assert_eq!(
            out["programs"]["pi-coding-agent"]["extraPackages"],
            serde_json::json!(["hello", "git"])
        );
    }

    #[test]
    fn extra_packages_must_be_an_array() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let mut cfg = config(dir.path()).unwrap();
        cfg["programs"]["pi-coding-agent"]["extraPackages"] = serde_json::json!("hello");
        let err = set_config(dir.path(), &cfg).unwrap_err();
        assert!(matches!(err, ActionError::BadRequest(_)));
    }
}
