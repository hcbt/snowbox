//! Environment is a Host flake. Copied into each Sandbox dir; never /workspace.

pub const DEFAULT_FLAKE: &str = include_str!("../../environment/empty/flake.nix");
pub const DEFAULT_LOCK: &str = include_str!("../../environment/empty/flake.lock");
pub const DEFAULT_HOME: &str = include_str!("../../environment/empty/home.nix");
pub const DEFAULT_CONFIG: &str = include_str!("../../environment/empty/config.json");
pub const SCHEMA: &str = include_str!("../../environment/empty/schema.json");
pub const DEFAULT_DUMP: &str = include_str!("../../environment/empty/dump-options.nix");

const FILES: [&str; 5] = [
    "flake.nix",
    "flake.lock",
    "home.nix",
    "config.json",
    "dump-options.nix",
];
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
    std::fs::write(env_dir.join("dump-options.nix"), DEFAULT_DUMP)
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
    let mut value = value.clone();
    clamp_extra_packages(&mut value);
    let raw = serde_json::to_string_pretty(&value).map_err(|_| ActionError::Internal)?;
    std::fs::write(dir.join("environment/config.json"), raw).map_err(|_| ActionError::Internal)?;
    config(dir)
}

fn clamp_extra_packages(value: &mut serde_json::Value) {
    let Some(programs) = value.get_mut("programs").and_then(|p| p.as_object_mut()) else {
        return;
    };
    for (name, cfg) in programs.iter_mut() {
        let Some(obj) = cfg.as_object_mut() else {
            continue;
        };
        if name != "pi-coding-agent" {
            obj.remove("extraPackages");
            continue;
        }
        let Some(arr) = obj.get_mut("extraPackages").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        arr.retain(|v| v.as_str().map(|s| PI_EXTRA.contains(&s)).unwrap_or(false));
    }
}

fn secrets_path(dir: &Path) -> std::path::PathBuf {
    dir.join("secrets.json")
}

pub fn secrets(dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    let Ok(raw) = std::fs::read_to_string(secrets_path(dir)) else {
        return serde_json::Map::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn set_secrets(
    dir: &Path,
    env: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ActionError> {
    if env.is_empty() {
        clear_secrets(dir);
        return Ok(());
    }
    let raw = serde_json::to_string_pretty(env).map_err(|_| ActionError::Internal)?;
    std::fs::write(secrets_path(dir), raw).map_err(|_| ActionError::Internal)
}

pub fn clear_secrets(dir: &Path) {
    let _ = std::fs::remove_file(secrets_path(dir));
}

pub fn write_guest_env(
    home: &Path,
    env: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ActionError> {
    std::fs::create_dir_all(home).map_err(|_| ActionError::Internal)?;
    let path = home.join(".snowbox-env");
    if env.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let mut buf = String::new();
    for (k, v) in env {
        let Some(val) = v.as_str() else {
            continue;
        };
        if !key_ok(k) {
            return Err(ActionError::BadRequest("invalid env name"));
        }
        buf.push_str("export ");
        buf.push_str(k);
        buf.push_str("='");
        buf.push_str(&val.replace('\'', "'\\''"));
        buf.push_str("'\n");
    }
    std::fs::write(path, buf).map_err(|_| ActionError::Internal)
}

fn key_ok(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn document(dir: &Path) -> Result<serde_json::Value, ActionError> {
    let mut cfg = config(dir)?;
    let env = secrets(dir);
    if !env.is_empty() {
        cfg["env"] = serde_json::Value::Object(env);
    }
    Ok(cfg)
}

const DROP_OPTIONS: &[&str] = &["package", "finalPackage", "enableMcpIntegration"];
const PI_EXTRA: [&str; 2] = ["nodejs", "bun"];

pub fn schema_from_hm(raw: &serde_json::Value) -> Result<serde_json::Value, ActionError> {
    let Some(programs) = raw.get("programs").and_then(|p| p.as_array()) else {
        return Err(ActionError::Internal);
    };
    let mut out = Vec::new();
    for p in programs {
        let Some(name) = p.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let desc = p
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or(name);
        let Some(opts) = p.get("options").and_then(|o| o.as_array()) else {
            continue;
        };
        let mut options = Vec::new();
        for opt in opts {
            if let Some(filtered) = filter_option(opt) {
                options.push(filtered);
            }
        }
        out.push(serde_json::json!({
            "name": name,
            "description": desc,
            "options": options,
        }));
    }
    Ok(serde_json::json!({ "programs": out }))
}

fn filter_option(opt: &serde_json::Value) -> Option<serde_json::Value> {
    let name = opt.get("name")?.as_str()?;
    if DROP_OPTIONS.contains(&name) {
        return None;
    }
    if name.to_ascii_lowercase().contains("mcp") {
        return None;
    }
    if opt.get("internal").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    if opt.get("readOnly").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let ty = opt.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let ty_l = ty.to_ascii_lowercase();
    if name != "extraPackages" && ty_l.contains("package") && !ty_l.contains("list") {
        return None;
    }
    let mapped = map_type(name, &ty_l);
    let default = if name == "extraPackages" {
        serde_json::json!(PI_EXTRA)
    } else {
        opt.get("default")
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    Some(serde_json::json!({
        "name": name,
        "type": mapped,
        "default": default,
        "description": opt.get("description").and_then(|d| d.as_str()).unwrap_or(""),
    }))
}

pub fn agent_schema() -> serde_json::Value {
    static CACHE: std::sync::Mutex<Option<(String, serde_json::Value)>> =
        std::sync::Mutex::new(None);
    if let Ok(guard) = CACHE.lock()
        && let Some((key, value)) = guard.as_ref()
        && key == DEFAULT_LOCK
    {
        return value.clone();
    }
    match live_schema() {
        Ok(value) => {
            if let Ok(mut guard) = CACHE.lock() {
                *guard = Some((DEFAULT_LOCK.to_string(), value.clone()));
            }
            value
        }
        Err(_) => serde_json::from_str(SCHEMA).expect("schema json"),
    }
}

fn live_schema() -> Result<serde_json::Value, ActionError> {
    let dir = tempfile::tempdir().map_err(|_| ActionError::Internal)?;
    write_default(dir.path())?;
    let flake = dir
        .path()
        .join("environment")
        .canonicalize()
        .map_err(|_| ActionError::Internal)?;
    let url = crate::nix::path_flake_url_pub(&flake)?;
    let expr = format!("let f = builtins.getFlake ''{url}''; in f.agentOptions");
    let raw = crate::nix::eval_string(&expr, "<snowbox-agent-options>")?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| ActionError::Internal)?;
    schema_from_hm(&parsed)
}

fn map_type(name: &str, ty: &str) -> &'static str {
    if name == "extraPackages" {
        return "packages";
    }
    if ty.contains("bool") {
        return "boolean";
    }
    if name != "extraPackages" && ty.contains("list") {
        return "json";
    }
    if ty.contains("int") || ty.contains("float") || ty.contains("number") {
        return "number";
    }
    if ty.contains("json") || ty.contains("attribute set") || ty.contains("attrs") {
        return "json";
    }
    if ty.contains("path") || ty.contains("string") || ty.contains("lines") {
        return "string";
    }
    "json"
}

pub fn set_document(
    dir: &Path,
    value: &serde_json::Value,
) -> Result<serde_json::Value, ActionError> {
    let mut body = value.clone();
    let env = body.as_object_mut().and_then(|o| o.remove("env"));
    set_config(dir, &body)?;
    match env {
        Some(serde_json::Value::Object(map)) => set_secrets(dir, &map)?,
        Some(_) => return Err(ActionError::BadRequest("env must be an object")),
        None => clear_secrets(dir),
    }
    let env = secrets(dir);
    write_guest_env(&dir.join("home"), &env)?;
    document(dir)
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
    fn document_keeps_env_out_of_flake_config() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let mut body = config(dir.path()).unwrap();
        body["env"] = serde_json::json!({ "ANTHROPIC_API_KEY": "sk-test" });
        set_document(dir.path(), &body).unwrap();
        assert!(config(dir.path()).unwrap().get("env").is_none());
        assert_eq!(
            document(dir.path()).unwrap()["env"]["ANTHROPIC_API_KEY"],
            "sk-test"
        );
        let sourced = std::fs::read_to_string(dir.path().join("home/.snowbox-env")).unwrap();
        assert!(sourced.contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn schema_from_hm_drops_unusable_keeps_agent_options() {
        let raw = serde_json::json!({
            "programs": [{
                "name": "pi-coding-agent",
                "description": "Pi",
                "options": [
                    {"name": "enable", "type": "boolean", "default": false, "description": "on", "internal": false, "readOnly": false},
                    {"name": "package", "type": "package", "default": null, "description": "pkg", "internal": false, "readOnly": false},
                    {"name": "finalPackage", "type": "package", "default": null, "description": "computed", "internal": true, "readOnly": true},
                    {"name": "enableMcpIntegration", "type": "boolean", "default": false, "description": "mcp", "internal": false, "readOnly": false},
                    {"name": "mcpServers", "type": "JSON value", "default": {}, "description": "mcp servers", "internal": false, "readOnly": false},
                    {"name": "keybindings", "type": "JSON value", "default": {}, "description": "keys", "internal": false, "readOnly": false},
                    {"name": "configDir", "type": "string", "default": "/home/snow/.pi/agent", "description": "dir", "internal": false, "readOnly": false},
                    {"name": "extraPackages", "type": "list of package", "default": [], "description": "path extras", "internal": false, "readOnly": false},
                    {"name": "memory.source", "type": "null or path", "default": null, "description": "host file", "internal": false, "readOnly": false}
                ]
            }]
        });
        let out = schema_from_hm(&raw).unwrap();
        let opts = &out["programs"][0]["options"];
        let names: Vec<&str> = opts
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "enable",
                "keybindings",
                "configDir",
                "extraPackages",
                "memory.source"
            ]
        );
        let extra = opts
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "extraPackages")
            .unwrap();
        assert_eq!(extra["type"], "packages");
        assert_eq!(extra["default"], serde_json::json!(["nodejs", "bun"]));
        let path_opt = opts
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "memory.source")
            .unwrap();
        assert_eq!(path_opt["type"], "string");
        let keys = opts
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "keybindings")
            .unwrap();
        assert_eq!(keys["type"], "json");
        assert!(!names.contains(&"mcpServers"));
        assert!(!names.contains(&"enableMcpIntegration"));
    }

    #[test]
    fn extra_packages_are_clamped_to_the_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        write_default(dir.path()).unwrap();
        let mut cfg = config(dir.path()).unwrap();
        cfg["programs"]["pi-coding-agent"]["extraPackages"] =
            serde_json::json!(["nodejs", "hello", "bun"]);
        cfg["programs"]["claude-code"]["extraPackages"] = serde_json::json!(["nodejs"]);
        set_config(dir.path(), &cfg).unwrap();
        let out = config(dir.path()).unwrap();
        assert_eq!(
            out["programs"]["pi-coding-agent"]["extraPackages"],
            serde_json::json!(["nodejs", "bun"])
        );
        assert!(
            out["programs"]["claude-code"]
                .get("extraPackages")
                .is_none()
        );
    }
}
