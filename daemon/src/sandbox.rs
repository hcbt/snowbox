use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
const DEFAULT_CPU: u32 = 2;
const DEFAULT_RAM: u64 = 2 * GIB;
const DEFAULT_DISK: u64 = 16 * GIB;
const MIN_CPU: u32 = 1;
const MIN_RAM: u64 = 512 * MIB;
const MIN_DISK: u64 = GIB;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Stopped,
    Running,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub cpu: u32,
    pub ram: u64,
    pub disk: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            cpu: DEFAULT_CPU,
            ram: DEFAULT_RAM,
            disk: DEFAULT_DISK,
        }
    }
}

impl Limits {
    pub fn validate(self) -> Result<Self, ActionError> {
        if self.cpu < MIN_CPU {
            return Err(ActionError::BadRequest("cpu must be at least 1"));
        }
        if self.ram < MIN_RAM {
            return Err(ActionError::BadRequest("ram must be at least 512 MiB"));
        }
        if !self.ram.is_multiple_of(MIB) {
            return Err(ActionError::BadRequest("ram must be a multiple of 1 MiB"));
        }
        if self.disk < MIN_DISK {
            return Err(ActionError::BadRequest("disk must be at least 1 GiB"));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Sandbox {
    pub id: Uuid,
    pub name: String,
    pub state: State,
    pub booting: bool,
    pub home: Vec<String>,
    pub limits: Limits,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Meta {
    id: Uuid,
    name: String,
    home: Vec<String>,
    #[serde(default)]
    limits: Limits,
}

struct Record {
    meta: Meta,
    state: State,
    booting: bool,
}

#[derive(Debug)]
pub enum ActionError {
    NotFound,
    Conflict(&'static str),
    BadRequest(&'static str),
    Failed(String),
    Internal,
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::Conflict(d) => write!(f, "{d}"),
            Self::BadRequest(d) => write!(f, "{d}"),
            Self::Failed(d) => write!(f, "{d}"),
            Self::Internal => write!(f, "internal error"),
        }
    }
}

impl std::error::Error for ActionError {}

pub struct Store {
    root: PathBuf,
    inner: Mutex<HashMap<Uuid, Record>>,
}

impl Store {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ActionError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|_| ActionError::Internal)?;
        let mut map = HashMap::new();
        for entry in fs::read_dir(&root).map_err(|_| ActionError::Internal)? {
            let entry = entry.map_err(|_| ActionError::Internal)?;
            if !entry
                .file_type()
                .map_err(|_| ActionError::Internal)?
                .is_dir()
            {
                continue;
            }
            let meta_path = entry.path().join("meta.json");
            if !meta_path.exists() {
                continue;
            }
            let raw = fs::read_to_string(&meta_path).map_err(|_| ActionError::Internal)?;
            let meta: Meta = serde_json::from_str(&raw).map_err(|_| ActionError::Internal)?;
            map.insert(
                meta.id,
                Record {
                    meta,
                    state: State::Stopped,
                    booting: false,
                },
            );
        }
        Ok(Self {
            root,
            inner: Mutex::new(map),
        })
    }

    pub fn list(&self) -> Vec<Sandbox> {
        let map = self.inner.lock().expect("sandbox store");
        let mut v: Vec<_> = map.values().map(view).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        v
    }

    #[cfg(test)]
    pub fn create(&self, name: Option<String>) -> Result<Sandbox, ActionError> {
        self.create_with(name, Limits::default(), None)
    }

    pub fn create_with(
        &self,
        name: Option<String>,
        limits: Limits,
        template: Option<&Path>,
    ) -> Result<Sandbox, ActionError> {
        let limits = limits.validate()?;
        let id = Uuid::new_v4();
        let name = match name {
            Some(n) if !n.trim().is_empty() => n,
            _ => format!("sandbox-{}", &id.to_string()[..8]),
        };
        let meta = Meta {
            id,
            name,
            home: Vec::new(),
            limits,
        };
        let dir = self.dir(id);
        fs::create_dir_all(dir.join("workspace")).map_err(|_| ActionError::Internal)?;
        fs::create_dir_all(dir.join("home")).map_err(|_| ActionError::Internal)?;
        fs::create_dir_all(dir.join("system")).map_err(|_| ActionError::Internal)?;
        if let Some(template) = template {
            crate::environment::write_from_template(&dir, template)?;
        } else {
            crate::environment::write_default(&dir)?;
        }
        crate::environment::snapshot_create(&dir)?;
        write_meta(&dir, &meta)?;
        let sandbox = view(&Record {
            meta: meta.clone(),
            state: State::Stopped,
            booting: false,
        });
        self.inner.lock().expect("sandbox store").insert(
            id,
            Record {
                meta,
                state: State::Stopped,
                booting: false,
            },
        );
        Ok(sandbox)
    }

    pub fn get(&self, id: Uuid) -> Result<Sandbox, ActionError> {
        let map = self.inner.lock().expect("sandbox store");
        map.get(&id).map(view).ok_or(ActionError::NotFound)
    }

    pub fn set_limits(&self, id: Uuid, limits: Limits) -> Result<Sandbox, ActionError> {
        let limits = limits.validate()?;
        let mut map = self.inner.lock().expect("sandbox store");
        let rec = map.get_mut(&id).ok_or(ActionError::NotFound)?;
        rec.meta.limits = limits;
        write_meta(&self.root.join(id.to_string()), &rec.meta)?;
        Ok(view(rec))
    }

    pub fn start(&self, id: Uuid) -> Result<Sandbox, ActionError> {
        let mut map = self.inner.lock().expect("sandbox store");
        let rec = map.get_mut(&id).ok_or(ActionError::NotFound)?;
        if rec.state == State::Running {
            return Err(ActionError::Conflict("already running"));
        }
        rec.booting = false;
        rec.state = State::Running;
        Ok(view(rec))
    }

    pub fn begin_boot(&self, id: Uuid) -> Result<(), ActionError> {
        let mut map = self.inner.lock().expect("sandbox store");
        let rec = map.get_mut(&id).ok_or(ActionError::NotFound)?;
        if rec.state == State::Running || rec.booting {
            return Err(ActionError::Conflict("already running"));
        }
        rec.booting = true;
        Ok(())
    }

    pub fn abort_boot(&self, id: Uuid) {
        let mut map = self.inner.lock().expect("sandbox store");
        if let Some(rec) = map.get_mut(&id) {
            rec.booting = false;
        }
    }

    pub fn stop(&self, id: Uuid) -> Result<Sandbox, ActionError> {
        self.set_state(id, State::Running, State::Stopped, "already stopped")
    }

    pub fn reset(&self, id: Uuid) -> Result<Sandbox, ActionError> {
        let _ = {
            let map = self.inner.lock().expect("sandbox store");
            map.get(&id).ok_or(ActionError::NotFound)?;
        };
        let dir = self.dir(id);
        reset_tree(&dir)?;
        self.get(id)
    }

    pub fn destroy(&self, id: Uuid) -> Result<(), ActionError> {
        let _ = self.get(id)?;
        let dir = self.dir(id);
        if dir.exists() {
            make_deletable(&dir)
                .map_err(|e| ActionError::Failed(format!("destroy disk: {e}")))?;
            fs::remove_dir_all(&dir)
                .map_err(|e| ActionError::Failed(format!("destroy disk: {e}")))?;
        }
        self.inner.lock().expect("sandbox store").remove(&id);
        Ok(())
    }

    pub fn copy_in(&self, id: Uuid, from: &Path, replace: bool) -> Result<Sandbox, ActionError> {
        self.require_stopped(id)?;
        let from = jail_source(from)?;
        if !from.exists() {
            return Err(ActionError::BadRequest("source does not exist"));
        }
        let workspace = self.dir(id).join("workspace");
        if is_non_empty(&workspace) && !replace {
            return Err(ActionError::Conflict("replace required"));
        }
        if replace && workspace.exists() {
            fs::remove_dir_all(&workspace).map_err(|_| ActionError::Internal)?;
            fs::create_dir_all(&workspace).map_err(|_| ActionError::Internal)?;
        }
        put_into_workspace(&from, &workspace)?;
        self.get(id)
    }

    pub fn copy_out(&self, id: Uuid, to: &Path, replace: bool) -> Result<Sandbox, ActionError> {
        self.require_stopped(id)?;
        let to = jail_dest(to)?;
        let workspace = self.dir(id).join("workspace");
        if is_non_empty(&to) && !replace {
            return Err(ActionError::Conflict("replace required"));
        }
        if replace && to.exists() {
            if to.is_dir() {
                fs::remove_dir_all(&to).map_err(|_| ActionError::Internal)?;
            } else {
                fs::remove_file(&to).map_err(|_| ActionError::Internal)?;
            }
        }
        if is_non_empty(&workspace) {
            copy_tree(&workspace, &to)?;
        } else {
            fs::create_dir_all(&to).map_err(|_| ActionError::Internal)?;
        }
        self.get(id)
    }

    fn require_stopped(&self, id: Uuid) -> Result<(), ActionError> {
        let map = self.inner.lock().expect("sandbox store");
        let rec = map.get(&id).ok_or(ActionError::NotFound)?;
        if rec.state != State::Stopped {
            return Err(ActionError::Conflict("sandbox is running"));
        }
        Ok(())
    }

    fn set_state(
        &self,
        id: Uuid,
        from: State,
        to: State,
        conflict: &'static str,
    ) -> Result<Sandbox, ActionError> {
        let mut map = self.inner.lock().expect("sandbox store");
        let rec = map.get_mut(&id).ok_or(ActionError::NotFound)?;
        if rec.state != from {
            return Err(ActionError::Conflict(conflict));
        }
        rec.booting = false;
        rec.state = to;
        Ok(view(rec))
    }

    pub(crate) fn dir(&self, id: Uuid) -> PathBuf {
        self.root.join(id.to_string())
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

fn view(rec: &Record) -> Sandbox {
    Sandbox {
        id: rec.meta.id,
        name: rec.meta.name.clone(),
        state: rec.state,
        booting: rec.booting,
        home: rec.meta.home.clone(),
        limits: rec.meta.limits,
    }
}

fn write_meta(dir: &Path, meta: &Meta) -> Result<(), ActionError> {
    let path = dir.join("meta.json");
    let raw = serde_json::to_string_pretty(meta).map_err(|_| ActionError::Internal)?;
    fs::write(path, raw).map_err(|_| ActionError::Internal)
}

fn reset_tree(dir: &Path) -> Result<(), ActionError> {
    crate::environment::restore_create(dir)?;
    crate::environment::clear_secrets(dir);

    let system = dir.join("system");
    if system.exists() {
        fs::remove_dir_all(&system).map_err(|_| ActionError::Internal)?;
    }
    fs::create_dir_all(&system).map_err(|_| ActionError::Internal)?;

    let home = dir.join("home");
    if home.exists() {
        make_deletable(&home).map_err(|_| ActionError::Internal)?;
        fs::remove_dir_all(&home).map_err(|_| ActionError::Internal)?;
    }
    fs::create_dir_all(&home).map_err(|_| ActionError::Internal)?;

    fs::create_dir_all(dir.join("workspace")).map_err(|_| ActionError::Internal)?;

    for entry in fs::read_dir(dir).map_err(|_| ActionError::Internal)? {
        let entry = entry.map_err(|_| ActionError::Internal)?;
        let name = entry.file_name();
        if name == "workspace"
            || name == "home"
            || name == "system"
            || name == "meta.json"
            || name == "environment"
            || name == crate::environment::CREATE_DIR
        {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path).map_err(|_| ActionError::Internal)?;
        } else {
            fs::remove_file(&path).map_err(|_| ActionError::Internal)?;
        }
    }
    Ok(())
}

fn put_into_workspace(from: &Path, workspace: &Path) -> Result<(), ActionError> {
    fs::create_dir_all(workspace).map_err(|_| ActionError::Internal)?;
    let meta = from.symlink_metadata().map_err(|_| ActionError::Internal)?;
    if meta.is_symlink() {
        return Err(ActionError::BadRequest("source is a symlink"));
    }
    if meta.is_file() {
        let name = from
            .file_name()
            .ok_or(ActionError::BadRequest("source has no name"))?;
        copy_tree(from, &workspace.join(name))?;
        return Ok(());
    }
    copy_tree(from, workspace)
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), ActionError> {
    let meta = src.symlink_metadata().map_err(|_| ActionError::Internal)?;
    if meta.is_symlink() {
        return Err(ActionError::BadRequest("refusing to copy symlink"));
    }
    if meta.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|_| ActionError::Internal)?;
        }
        fs::copy(src, dst).map_err(|_| ActionError::Internal)?;
        return Ok(());
    }
    fs::create_dir_all(dst).map_err(|_| ActionError::Internal)?;
    for entry in fs::read_dir(src).map_err(|_| ActionError::Internal)? {
        let entry = entry.map_err(|_| ActionError::Internal)?;
        copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
    }
    Ok(())
}

fn canonical_absolute(path: &Path) -> Result<PathBuf, ActionError> {
    if !path.is_absolute() {
        return Err(ActionError::BadRequest("path must be absolute"));
    }
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|_| ActionError::BadRequest("path is not allowed"));
    }
    let mut cur = path.to_path_buf();
    let mut missing = Vec::new();
    while !cur.exists() {
        let name = cur
            .file_name()
            .ok_or(ActionError::BadRequest("path must be absolute"))?
            .to_os_string();
        missing.push(name);
        match cur.parent() {
            Some(parent) if parent != cur.as_path() => cur = parent.to_path_buf(),
            _ => return Err(ActionError::BadRequest("path is not allowed")),
        }
    }
    let mut resolved = cur
        .canonicalize()
        .map_err(|_| ActionError::BadRequest("path is not allowed"))?;
    for name in missing.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

fn jail_source(from: &Path) -> Result<PathBuf, ActionError> {
    canonical_absolute(from)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::RootDir => out.push("/"),
            std::path::Component::Prefix(p) => out.push(p.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                let _ = out.pop();
            }
            std::path::Component::Normal(s) => out.push(s),
        }
    }
    out
}

fn jail_dest(to: &Path) -> Result<PathBuf, ActionError> {
    if !to.is_absolute() {
        return Err(ActionError::BadRequest("path must be absolute"));
    }
    if dest_forbidden(&lexical_normalize(to)) {
        return Err(ActionError::BadRequest("path is not allowed"));
    }
    let to = canonical_absolute(to)?;
    if dest_forbidden(&to) {
        return Err(ActionError::BadRequest("path is not allowed"));
    }
    Ok(to)
}

fn dest_forbidden(path: &Path) -> bool {
    if path == Path::new("/") {
        return true;
    }
    if path == Path::new("/Users") || path == Path::new("/home") || path == Path::new("/root") {
        return true;
    }
    if is_unix_home_root(path) {
        return true;
    }
    if let Some(home) = dirs::home_dir() {
        if path == home {
            return true;
        }
    }
    if let Some(data) = dirs::data_dir() {
        let snow = data.join("snowbox");
        if path == snow || path.starts_with(&snow) {
            return true;
        }
    }
    path.starts_with("/nix/store") || path.starts_with("/etc") || path.starts_with("/private/etc")
}

fn is_unix_home_root(path: &Path) -> bool {
    let comps: Vec<_> = path.components().collect();
    matches!(
        comps.as_slice(),
        [std::path::Component::RootDir, std::path::Component::Normal(dir), std::path::Component::Normal(_name)]
            if *dir == "Users" || *dir == "home"
    )
}

/// home-manager writes `home/.nix-profile` as 0555. `remove_dir_all`
/// cannot unlink children of a directory without owner-write.
fn make_deletable(path: &Path) -> std::io::Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    let mut perm = meta.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut mode = perm.mode();
        mode |= 0o200;
        if meta.is_dir() {
            mode |= 0o100;
        }
        perm.set_mode(mode);
        fs::set_permissions(path, perm)?;
    }
    #[cfg(not(unix))]
    {
        perm.set_readonly(false);
        let _ = fs::set_permissions(path, perm);
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            make_deletable(&entry?.path())?;
        }
    }
    Ok(())
}

fn is_non_empty(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(meta) = path.symlink_metadata() else {
        return false;
    };
    if meta.is_file() {
        return meta.len() > 0;
    }
    fs::read_dir(path).ok().and_then(|mut i| i.next()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn create_applies_default_limits() {
        let (_tmp, store) = store();
        let sb = store.create(None).unwrap();
        assert_eq!(sb.limits, Limits::default());
        assert_eq!(sb.limits.cpu, 2);
        assert_eq!(sb.limits.ram, 2 * GIB);
        assert_eq!(sb.limits.disk, 16 * GIB);
    }

    #[test]
    fn missing_limits_in_meta_are_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let sb = dir.path().join(id.to_string());
        fs::create_dir_all(&sb).unwrap();
        fs::write(
            sb.join("meta.json"),
            format!(r#"{{"id":"{id}","name":"old","home":[".gitconfig"]}}"#),
        )
        .unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.get(id).unwrap().limits, Limits::default());
    }

    #[test]
    fn persist_custom_limits_across_open() {
        let (dir, store) = store();
        let limits = Limits {
            cpu: 4,
            ram: 4 * GIB,
            disk: 32 * GIB,
        };
        let created = store
            .create_with(Some("work".into()), limits, None)
            .unwrap();
        assert_eq!(created.limits, limits);
        drop(store);

        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.get(created.id).unwrap().limits, limits);
    }

    #[test]
    fn create_rejects_zero_cpu() {
        let (_tmp, store) = store();
        let err = store
            .create_with(
                None,
                Limits {
                    cpu: 0,
                    ram: 2 * GIB,
                    disk: 16 * GIB,
                },
                None,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            ActionError::BadRequest("cpu must be at least 1")
        ));
    }

    #[test]
    fn create_rejects_unaligned_ram() {
        let (_tmp, store) = store();
        let err = store
            .create_with(
                None,
                Limits {
                    cpu: 1,
                    ram: 512 * MIB + 1,
                    disk: 16 * GIB,
                },
                None,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            ActionError::BadRequest("ram must be a multiple of 1 MiB")
        ));
    }

    #[test]
    fn create_rejects_small_disk() {
        let (_tmp, store) = store();
        let err = store
            .create_with(
                None,
                Limits {
                    cpu: 1,
                    ram: 512 * MIB,
                    disk: GIB - 1,
                },
                None,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            ActionError::BadRequest("disk must be at least 1 GiB")
        ));
    }

    #[test]
    fn set_limits_persists() {
        let (dir, store) = store();
        let sb = store.create(None).unwrap();
        let limits = Limits {
            cpu: 1,
            ram: 512 * MIB,
            disk: 4 * GIB,
        };
        assert_eq!(store.set_limits(sb.id, limits).unwrap().limits, limits);
        drop(store);

        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.get(sb.id).unwrap().limits, limits);
    }

    #[test]
    fn persist_across_open() {
        let (dir, store) = store();
        let created = store.create(Some("work".into())).unwrap();
        fs::write(
            dir.path()
                .join(created.id.to_string())
                .join("workspace/a.txt"),
            "hi",
        )
        .unwrap();
        drop(store);

        let store = Store::open(dir.path()).unwrap();
        let got = store.get(created.id).unwrap();
        assert_eq!(got.name, "work");
        assert_eq!(got.state, State::Stopped);
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join(created.id.to_string())
                    .join("workspace/a.txt")
            )
            .unwrap(),
            "hi"
        );
    }

    #[test]
    fn copy_in_replace_and_copy_out() {
        let (tmp, store) = store();
        let sb = store.create(None).unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("readme"), "one").unwrap();
        store.copy_in(sb.id, &src, false).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join(sb.id.to_string()).join("workspace/readme"))
                .unwrap(),
            "one"
        );

        fs::write(src.join("readme"), "two").unwrap();
        let err = store.copy_in(sb.id, &src, false).unwrap_err();
        assert!(matches!(err, ActionError::Conflict("replace required")));

        store.copy_in(sb.id, &src, true).unwrap();
        let dest = tmp.path().join("out");
        store.copy_out(sb.id, &dest, false).unwrap();
        assert_eq!(fs::read_to_string(dest.join("readme")).unwrap(), "two");
    }

    #[test]
    fn copy_in_refused_while_running() {
        let (tmp, store) = store();
        let sb = store.create(None).unwrap();
        store.start(sb.id).unwrap();
        let src = tmp.path().join("f.txt");
        fs::write(&src, "x").unwrap();
        let err = store.copy_in(sb.id, &src, true).unwrap_err();
        assert!(matches!(err, ActionError::Conflict("sandbox is running")));
    }

    #[test]
    fn reset_rewinds_environment_and_wipes_home() {
        let (_tmp, store) = store();
        let sb = store.create(Some("r".into())).unwrap();
        let dir = store.dir(sb.id);
        fs::write(dir.join("workspace/proj"), "code").unwrap();
        fs::write(dir.join("home/.gitconfig"), "git").unwrap();
        fs::create_dir_all(dir.join("home/.npm")).unwrap();
        fs::write(dir.join("home/.npm/junk"), "no").unwrap();
        fs::write(dir.join("system/undeclared"), "tool").unwrap();
        fs::write(dir.join("extra"), "drop").unwrap();
        let mut cfg = crate::environment::config(&dir).unwrap();
        cfg["programs"]["claude-code"]["enable"] = serde_json::Value::Bool(true);
        crate::environment::set_config(&dir, &cfg).unwrap();

        store.reset(sb.id).unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("workspace/proj")).unwrap(),
            "code"
        );
        assert!(!dir.join("home/.gitconfig").exists());
        assert!(!dir.join("home/.npm").exists());
        assert!(!dir.join("system/undeclared").exists());
        assert!(!dir.join("extra").exists());
        assert!(dir.join("system").is_dir());
        assert!(dir.join("environment/flake.nix").is_file());
        assert_eq!(
            crate::environment::config(&dir).unwrap()["programs"]["claude-code"]["enable"],
            false
        );
    }

    #[test]
    fn create_writes_host_environment_flake() {
        let (_tmp, store) = store();
        let sb = store.create(None).unwrap();
        let flake = fs::read_to_string(store.dir(sb.id).join("environment/flake.nix")).unwrap();
        assert!(flake.contains("home-manager"));
        assert!(flake.contains("nixpkgs-unstable"));
        assert!(!store.dir(sb.id).join("workspace/flake.nix").exists());
    }

    #[test]
    fn workspaces_are_not_shared() {
        let (_tmp, store) = store();
        let a = store.create(Some("a".into())).unwrap();
        let b = store.create(Some("b".into())).unwrap();
        fs::write(store.dir(a.id).join("workspace/secret"), "only-a").unwrap();
        assert!(!store.dir(b.id).join("workspace/secret").exists());
        assert_ne!(store.dir(a.id), store.dir(b.id));
    }

    #[test]
    fn begin_boot_rejects_a_second_start() {
        let (_tmp, store) = store();
        let sb = store.create(None).unwrap();
        store.begin_boot(sb.id).unwrap();
        let err = store.begin_boot(sb.id).unwrap_err();
        assert!(matches!(err, ActionError::Conflict("already running")));
        store.abort_boot(sb.id);
        store.begin_boot(sb.id).unwrap();
    }

    #[test]
    fn begin_boot_is_listed_while_start_runs() {
        let (_tmp, store) = store();
        let sb = store.create(None).unwrap();
        assert!(!sb.booting);
        store.begin_boot(sb.id).unwrap();
        let mid = store.get(sb.id).unwrap();
        assert!(mid.booting);
        assert_eq!(mid.state, State::Stopped);
        store.start(sb.id).unwrap();
        let running = store.get(sb.id).unwrap();
        assert!(!running.booting);
        assert_eq!(running.state, State::Running);
    }

    #[test]
    fn destroy_one_keeps_the_other() {
        let (_tmp, store) = store();
        let a = store.create(Some("keep-a".into())).unwrap();
        let b = store.create(Some("drop-b".into())).unwrap();
        fs::write(store.dir(a.id).join("workspace/x"), "a").unwrap();
        store.destroy(b.id).unwrap();
        assert_eq!(
            fs::read_to_string(store.dir(a.id).join("workspace/x")).unwrap(),
            "a"
        );
        assert!(matches!(
            store.get(b.id).unwrap_err(),
            ActionError::NotFound
        ));
    }

    #[test]
    fn copy_out_replace_home_is_refused() {
        let (tmp, store) = store();
        let sb = store.create(None).unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a"), "1").unwrap();
        store.copy_in(sb.id, &src, false).unwrap();
        let home = dirs::home_dir().expect("home");
        let err = store.copy_out(sb.id, &home, true).unwrap_err();
        assert!(matches!(
            err,
            ActionError::BadRequest("path is not allowed")
        ));
        assert!(home.exists());
    }

    #[test]
    fn copy_out_traversal_to_users_is_refused() {
        let (tmp, store) = store();
        let sb = store.create(None).unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a"), "1").unwrap();
        store.copy_in(sb.id, &src, false).unwrap();
        let sneaky = PathBuf::from("/tmp/../Users");
        let err = store.copy_out(sb.id, &sneaky, true).unwrap_err();
        assert!(matches!(
            err,
            ActionError::BadRequest("path is not allowed")
        ));
    }

    #[test]
    fn copy_out_to_root_and_etc_are_refused() {
        let (tmp, store) = store();
        let sb = store.create(None).unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a"), "1").unwrap();
        store.copy_in(sb.id, &src, false).unwrap();
        let err = store.copy_out(sb.id, Path::new("/"), true).unwrap_err();
        assert!(matches!(
            err,
            ActionError::BadRequest("path is not allowed")
        ));
        let err = store
            .copy_out(sb.id, Path::new("/etc/passwd"), true)
            .unwrap_err();
        assert!(matches!(
            err,
            ActionError::BadRequest("path is not allowed")
        ));
    }

    #[test]
    fn copy_in_relative_path_is_refused() {
        let (_tmp, store) = store();
        let sb = store.create(None).unwrap();
        let err = store
            .copy_in(sb.id, Path::new("relative"), false)
            .unwrap_err();
        assert!(matches!(
            err,
            ActionError::BadRequest("path must be absolute")
        ));
    }

    #[test]
    fn destroy_removes_a_readonly_nix_profile() {
        let (tmp, store) = store();
        let sb = store.create(None).unwrap();
        let bin = store.dir(sb.id).join("home/.nix-profile/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("hello"), "x").unwrap();
        let profile = store.dir(sb.id).join("home/.nix-profile");
        let mut perm = fs::metadata(&profile).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perm.set_mode(0o555);
            fs::set_permissions(&profile, perm.clone()).unwrap();
            fs::set_permissions(&bin, perm).unwrap();
        }
        store.destroy(sb.id).unwrap();
        assert!(!store.dir(sb.id).exists());
        let reopened = Store::open(tmp.path()).unwrap();
        assert!(reopened.list().is_empty());
    }

    #[test]
    fn destroy_deletes_disk_stop_does_not() {
        let (tmp, store) = store();
        let sb = store.create(None).unwrap();
        let disk = tmp.path().join(sb.id.to_string());
        fs::write(disk.join("workspace/f"), "x").unwrap();
        store.start(sb.id).unwrap();
        store.stop(sb.id).unwrap();
        assert!(disk.join("workspace/f").exists());
        store.destroy(sb.id).unwrap();
        assert!(!disk.exists());
        let reopened = Store::open(tmp.path()).unwrap();
        assert!(reopened.list().is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn destroy_keeps_the_sandbox_if_the_disk_cannot_be_removed() {
        let (tmp, store) = store();
        let sb = store.create(None).unwrap();
        let locked = store.dir(sb.id).join("locked.bin");
        fs::write(&locked, b"x").unwrap();
        let flag = std::process::Command::new("chflags")
            .args(["uchg", locked.to_str().unwrap()])
            .status()
            .expect("chflags");
        assert!(flag.success());
        let err = store.destroy(sb.id);
        let _ = std::process::Command::new("chflags")
            .args(["nouchg", locked.to_str().unwrap()])
            .status();
        assert!(err.is_err(), "destroy must fail while the disk remains");
        assert!(
            store.get(sb.id).is_ok(),
            "forgetting before the disk is gone makes Destroy look successful until restart"
        );
        store.destroy(sb.id).unwrap();
        let reopened = Store::open(tmp.path()).unwrap();
        assert!(reopened.list().is_empty());
    }
}
