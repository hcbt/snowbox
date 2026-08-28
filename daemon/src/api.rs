use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use serde::Deserialize;
use std::path::PathBuf;
use uuid::Uuid;

use crate::auth::{require_token, require_ws_origin};
use crate::cache::Cache;
use crate::layout::{Layout, LayoutStore};
use crate::publish::Publisher;
use crate::resume::Resume;
use crate::sandbox::{ActionError, Limits, State as SandboxState, Store};
use crate::templates::Library;
use crate::vmm::{Hypervisor, SAVE_NAME, StartKind};

pub const LISTEN_PORT: u16 = 5418;

pub fn listen_port() -> u16 {
    LISTEN_PORT
}

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub store: Arc<Store>,
    pub cache: Arc<Cache>,
    pub layout: Arc<LayoutStore>,
    pub templates: Arc<Library>,
    pub publish: Publisher,
    pub sessions: crate::pty::Sessions,
    pub vmm: Option<Arc<Hypervisor>>,
    pub resume: Arc<Resume>,
    pub agent_options: Arc<Result<serde_json::Value, String>>,
}

#[derive(Deserialize, Default)]
pub struct CreateBody {
    pub name: Option<String>,
    #[serde(default)]
    pub limits: LimitsPatch,
    pub template: Option<String>,
    pub environment: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
pub struct PatchBody {
    #[serde(default)]
    pub limits: LimitsPatch,
}

#[derive(Clone, Deserialize, Default)]
pub struct LimitsPatch {
    pub cpu: Option<u32>,
    pub ram: Option<u64>,
    pub disk: Option<u64>,
}

impl LimitsPatch {
    fn apply(self, base: Limits) -> Limits {
        Limits {
            cpu: self.cpu.unwrap_or(base.cpu),
            ram: self.ram.unwrap_or(base.ram),
            disk: self.disk.unwrap_or(base.disk),
        }
    }
}

#[derive(Deserialize)]
pub struct CopyInBody {
    pub from: PathBuf,
    #[serde(default)]
    pub replace: bool,
}

#[derive(Deserialize)]
pub struct CopyOutBody {
    pub to: PathBuf,
    #[serde(default)]
    pub replace: bool,
}

pub fn router(state: AppState) -> Router {
    let v1 = Router::new()
        .route("/health", get(health))
        .route("/sandboxes", get(list).post(create))
        .route("/sandboxes/{id}", get(get_one).patch(patch).delete(destroy))
        .route("/sandboxes/{id}/start", post(start))
        .route("/sandboxes/{id}/stop", post(stop))
        .route("/sandboxes/{id}/reset", post(reset))
        .route("/sandboxes/{id}/copy-in", post(copy_in))
        .route("/sandboxes/{id}/copy-out", post(copy_out))
        .route("/agent-options", get(agent_options))
        .route("/templates", get(list_templates).post(save_template))
        .route("/templates/{name}", get(get_template).put(put_template))
        .route(
            "/sandboxes/{id}/publish",
            get(list_publish).post(publish_port),
        )
        .route(
            "/sandboxes/{id}/publish/{port}",
            axum::routing::delete(unpublish_port),
        )
        .route(
            "/sandboxes/{id}/environment",
            get(get_environment).put(put_environment),
        )
        .route("/layout", get(get_layout).put(put_layout))
        .route("/sandboxes/{id}/windows", post(open_window))
        .route("/windows/{id}", axum::routing::delete(close_window))
        .route(
            "/windows/{id}/pty",
            get(crate::pty::upgrade).layer(middleware::from_fn(require_ws_origin)),
        )
        .layer(middleware::from_fn_with_state(state.clone(), require_token));

    Router::new().nest("/api/v1", v1).with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn list(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "sandboxes": state.store.list() }))
}

async fn create(
    State(state): State<AppState>,
    body: Option<Json<CreateBody>>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let limits = body.limits.apply(Limits::default());
    let template = match body.template.as_deref() {
        None | Some("") | Some("empty") => None,
        Some(name) => Some(state.templates.resolve(name).map_err(map_err)?),
    };
    let sandbox = state
        .store
        .create_with(body.name, limits, template.as_deref())
        .map_err(map_err)?;
    if let Some(environment) = body.environment {
        let dir = state.store.dir(sandbox.id);
        crate::environment::set_document(&dir, &environment).map_err(map_err)?;
        crate::environment::snapshot_create(&dir).map_err(map_err)?;
    }
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(sandbox).expect("sandbox json")),
    ))
}

async fn patch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let current = state.store.get(id).map_err(map_err)?;
    let limits = body.limits.apply(current.limits);
    action(state.store.set_limits(id, limits))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sandbox = state.store.get(id).map_err(map_err)?;
    Ok(Json(serde_json::to_value(sandbox).expect("sandbox json")))
}

async fn start(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let vmm = hypervisor(&state)?;
    let store = state.store.clone();
    let cache = state.cache.clone();
    let resume = state.resume.clone();
    let result = tokio::task::spawn_blocking(move || boot(store, cache, vmm, resume, id))
        .await
        .map_err(|_| ActionError::Internal)
        .and_then(|r| r);
    action(result)
}

async fn stop(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let vmm = hypervisor(&state)?;
    let store = state.store.clone();
    let resume = state.resume.clone();
    let result = tokio::task::spawn_blocking(move || halt(store, vmm, resume, id, true))
        .await
        .map_err(|_| ActionError::Internal)
        .and_then(|r| r);
    if result.is_ok() {
        state.publish.drop_sandbox(id);
        state.sessions.drop_sandbox(id);
    }
    action(result)
}

async fn reset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sandbox = state.store.get(id).map_err(map_err)?;
    if sandbox.state == SandboxState::Running {
        let vmm = hypervisor(&state)?;
        let store = state.store.clone();
        let resume = state.resume.clone();
        let result = tokio::task::spawn_blocking(move || {
            halt(store.clone(), vmm, resume, id, true)?;
            store.reset(id)
        })
        .await
        .map_err(|_| ActionError::Internal)
        .and_then(|r| r);
        if result.is_ok() {
            state.publish.drop_sandbox(id);
            state.sessions.drop_sandbox(id);
        }
        return action(result);
    }
    action(state.store.reset(id))
}

async fn destroy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    state.publish.drop_sandbox(id);
    state.sessions.drop_sandbox(id);
    if let Some(vmm) = state.vmm.clone() {
        let store = state.store.clone();
        let resume = state.resume.clone();
        let result = tokio::task::spawn_blocking(move || {
            if store.get(id).map(|s| s.state).ok() == Some(SandboxState::Running) {
                let _ = halt(store.clone(), vmm, resume.clone(), id, true);
            }
            resume.unmark(id);
            store.destroy(id)
        })
        .await
        .map_err(|_| ActionError::Internal)
        .and_then(|r| r);
        result.map_err(map_err)?;
        let _ = state.layout.close_sandbox_windows(id);
        return Ok(StatusCode::NO_CONTENT);
    }
    state.publish.drop_sandbox(id);
    state.sessions.drop_sandbox(id);
    state.store.destroy(id).map_err(map_err)?;
    let _ = state.layout.close_sandbox_windows(id);
    Ok(StatusCode::NO_CONTENT)
}

async fn copy_in(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CopyInBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    action(state.store.copy_in(id, &body.from, body.replace))
}

async fn copy_out(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CopyOutBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    action(state.store.copy_out(id, &body.to, body.replace))
}

#[derive(Deserialize)]
pub struct SaveTemplateBody {
    pub name: String,
    pub sandbox: Uuid,
}

#[derive(Deserialize)]
pub struct PublishBody {
    pub port: u16,
    pub host_port: Option<u16>,
}

async fn list_publish(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let _ = state.store.get(id).map_err(map_err)?;
    Ok(Json(serde_json::json!({
        "published": state.publish.list(id)
    })))
}

async fn publish_port(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PublishBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let sandbox = state.store.get(id).map_err(map_err)?;
    if sandbox.state != SandboxState::Running {
        return Err(map_err(ActionError::Conflict("sandbox is not running")));
    }
    let Some(vmm) = state.vmm.clone() else {
        return Err(map_err(ActionError::Failed("no hypervisor".into())));
    };
    let mapping = state
        .publish
        .publish(vmm, id, body.port, body.host_port)
        .await
        .map_err(map_err)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(mapping).expect("mapping json")),
    ))
}

async fn unpublish_port(
    State(state): State<AppState>,
    Path((id, port)): Path<(Uuid, u16)>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let _ = state.store.get(id).map_err(map_err)?;
    state.publish.unpublish(id, port).map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_templates(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let list = state.templates.list().map_err(map_err)?;
    Ok(Json(serde_json::json!({ "templates": list })))
}

async fn save_template(
    State(state): State<AppState>,
    Json(body): Json<SaveTemplateBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let _ = state.store.get(body.sandbox).map_err(map_err)?;
    let env = state.store.dir(body.sandbox).join("environment");
    let t = state.templates.save(&body.name, &env).map_err(map_err)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(t).expect("template json")),
    ))
}

async fn get_template(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let cfg = state.templates.config(&name).map_err(map_err)?;
    Ok(Json(cfg))
}

async fn put_template(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let cfg = state.templates.set_config(&name, &body).map_err(map_err)?;
    Ok(Json(cfg))
}

async fn agent_options(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.agent_options.as_ref() {
        Ok(value) => Ok(Json(value.clone())),
        Err(detail) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"failed","detail": detail})),
        )),
    }
}

async fn get_environment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let _ = state.store.get(id).map_err(map_err)?;
    let cfg = crate::environment::document(&state.store.dir(id)).map_err(map_err)?;
    Ok(Json(cfg))
}

async fn put_environment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sandbox = state.store.get(id).map_err(map_err)?;
    let dir = state.store.dir(id);
    let cfg = crate::environment::set_document(&dir, &body).map_err(map_err)?;
    if sandbox.state == SandboxState::Running {
        if let Some(vmm) = state.vmm.clone() {
            let store = state.store.clone();
            let cache = state.cache.clone();
            let home = dir.join("home");
            tokio::task::spawn_blocking(move || {
                apply_env(&store, &cache, &vmm, id)?;
                crate::agent::tar_in(&vmm, id, "/home/snow", &home).map_err(ActionError::Failed)
            })
            .await
            .map_err(|_| ActionError::Internal)
            .and_then(|r| r)
            .map_err(map_err)?;
        }
    }
    Ok(Json(cfg))
}

async fn get_layout(State(state): State<AppState>) -> Json<Layout> {
    Json(state.layout.get())
}

async fn put_layout(
    State(state): State<AppState>,
    Json(body): Json<Layout>,
) -> Result<Json<Layout>, (StatusCode, Json<serde_json::Value>)> {
    Ok(Json(state.layout.put(body).map_err(map_err)?))
}

async fn open_window(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let sandbox = state.store.get(id).map_err(map_err)?;
    let title = format!("{} — xterm", sandbox.name);
    let win = state.layout.open_window(id, title).map_err(map_err)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(win).expect("window json")),
    ))
}

async fn close_window(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    state.layout.close_window(id).map_err(map_err)?;
    state.sessions.drop_window(id);
    Ok(StatusCode::NO_CONTENT)
}

fn boot(
    store: Arc<Store>,
    cache: Arc<Cache>,
    vmm: Arc<Hypervisor>,
    resume: Arc<Resume>,
    id: Uuid,
) -> Result<crate::sandbox::Sandbox, ActionError> {
    store.begin_boot(id)?;
    match boot_claimed(&store, &cache, &vmm, id) {
        Ok(sandbox) => {
            resume.mark(id);
            kick_replenish(vmm, cache, store.root().to_path_buf());
            Ok(sandbox)
        }
        Err(err) => {
            let _ = vmm.stop(id);
            store.abort_boot(id);
            Err(err)
        }
    }
}

fn boot_claimed(
    store: &Store,
    cache: &Cache,
    vmm: &Hypervisor,
    id: Uuid,
) -> Result<crate::sandbox::Sandbox, ActionError> {
    let t0 = std::time::Instant::now();
    let sandbox = store.get(id)?;
    let dir = store.dir(id);
    let hatching = !dir.join("disk").join("root.raw").is_file() && !dir.join(SAVE_NAME).is_file();
    if hatching {
        wait_ready_snapshot(vmm, cache, store.root());
    }
    let kind = vmm
        .start(id, &dir, sandbox.limits)
        .map_err(ActionError::Failed)?;
    eprintln!(
        "sandbox {id}: hypervisor {}ms ({kind:?})",
        t0.elapsed().as_millis()
    );
    let t1 = std::time::Instant::now();
    let ready_for = match kind {
        StartKind::Restored => std::time::Duration::from_secs(8),
        StartKind::Cold => std::time::Duration::from_secs(180),
    };
    if let Err(e) = crate::agent::wait_ready(vmm, id, ready_for) {
        if kind == StartKind::Restored {
            eprintln!("sandbox {id}: restored agent dead ({e}); booting");
            let _ = vmm.stop(id);
            let _ = std::fs::remove_file(dir.join(SAVE_NAME));
            vmm.start_cold(id, &dir, sandbox.limits)
                .map_err(ActionError::Failed)?;
            crate::agent::wait_ready(vmm, id, std::time::Duration::from_secs(180))
                .map_err(ActionError::Failed)?;
        } else {
            return Err(ActionError::Failed(e));
        }
    }
    eprintln!("sandbox {id}: agent {}ms", t1.elapsed().as_millis());
    if dir.join(crate::vmm::HATCHED).is_file() {
        let _ = crate::agent::reset_dir(vmm, id, "/workspace");
        let _ = crate::agent::reset_dir(vmm, id, "/home/snow");
        forget_hatch_applied(&dir);
    }
    crate::agent::tar_in(vmm, id, "/workspace", &dir.join("workspace"))
        .map_err(ActionError::Failed)?;
    let _ = crate::agent::tar_in(vmm, id, "/home/snow", &dir.join("home"));
    let t2 = std::time::Instant::now();
    apply_env_at(&dir, cache, vmm, id)?;
    eprintln!("sandbox {id}: environment {}ms", t2.elapsed().as_millis());
    eprintln!("sandbox {id}: start {}ms", t0.elapsed().as_millis());
    store.start(id)
}

fn apply_env(store: &Store, cache: &Cache, vmm: &Hypervisor, id: Uuid) -> Result<(), ActionError> {
    apply_env_at(&store.dir(id), cache, vmm, id)
}

/// Ready-snapshot hatch RESET of /home/snow leaves environment.applied matching
/// the stamp, so apply_env_at would skip profile. Drop the stamp so a New
/// Sandbox always activates Environment (devenv on PATH).
fn forget_hatch_applied(dir: &std::path::Path) -> bool {
    let hatched = dir.join(crate::vmm::HATCHED);
    if !hatched.is_file() {
        return false;
    }
    let _ = std::fs::remove_file(dir.join("environment.applied"));
    let _ = std::fs::remove_file(&hatched);
    true
}

fn apply_env_at(
    dir: &std::path::Path,
    cache: &Cache,
    vmm: &Hypervisor,
    id: Uuid,
) -> Result<(), ActionError> {
    let stamp = crate::environment::fingerprint(dir)?;
    let mark = dir.join("environment.applied");
    if std::fs::read_to_string(&mark).ok().as_deref() == Some(stamp.as_str()) {
        return Ok(());
    }
    let realized = crate::nix::realize_environment(&dir.join("environment"), cache)?;
    crate::agent::nar_in(vmm, id, &realized.export).map_err(ActionError::Failed)?;
    crate::agent::profile(vmm, id, &realized.out_path).map_err(ActionError::Failed)?;
    std::fs::write(&mark, stamp).map_err(|_| ActionError::Internal)?;
    Ok(())
}

fn halt(
    store: Arc<Store>,
    vmm: Arc<Hypervisor>,
    resume: Arc<Resume>,
    id: Uuid,
    forget: bool,
) -> Result<crate::sandbox::Sandbox, ActionError> {
    let sandbox = store.get(id)?;
    if sandbox.state != SandboxState::Running {
        return Err(ActionError::Conflict("already stopped"));
    }
    let dir = store.dir(id);
    crate::agent::tar_out(&vmm, id, "/workspace", &dir.join("workspace"))
        .map_err(ActionError::Failed)?;
    let _ = crate::agent::tar_out(&vmm, id, "/home/snow", &dir.join("home"));
    let save = dir.join(SAVE_NAME);
    if let Err(e) = vmm.save_and_stop(id, &save) {
        eprintln!("sandbox {id}: save failed ({e}); power off");
        let _ = std::fs::remove_file(&save);
        vmm.stop(id).map_err(ActionError::Failed)?;
    }
    if forget {
        resume.unmark(id);
    }
    store.stop(id)
}

fn kick_replenish(vmm: Arc<Hypervisor>, cache: Arc<Cache>, sandboxes: PathBuf) {
    std::thread::spawn(move || wait_ready_snapshot(&vmm, &cache, &sandboxes));
}

fn wait_ready_snapshot(vmm: &Hypervisor, cache: &Cache, sandboxes: &std::path::Path) {
    crate::ready::ensure(
        || vmm.ready_snapshot_exists(sandboxes),
        || warm_once(vmm, cache, sandboxes),
    );
}

fn warm_once(vmm: &Hypervisor, cache: &Cache, sandboxes: &std::path::Path) -> Result<(), String> {
    eprintln!("ready snapshot: warming");
    let dir = sandboxes.join(".warm");
    let _ = std::fs::remove_dir_all(&dir);
    let id = Uuid::new_v4();
    let run = (|| {
        std::fs::create_dir_all(&dir).map_err(|e| format!("warm mkdir: {e}"))?;
        crate::environment::write_default(&dir).map_err(|e| e.to_string())?;
        vmm.start_cold(id, &dir, Limits::default())?;
        crate::agent::wait_ready(vmm, id, std::time::Duration::from_secs(180))?;
        apply_env_at(&dir, cache, vmm, id).map_err(|e| e.to_string())?;
        vmm.save_and_stop(id, &dir.join(SAVE_NAME))?;
        Ok::<(), String>(())
    })();
    if run.is_err() {
        let _ = vmm.stop(id);
    }
    let _ = std::fs::remove_dir_all(&dir);
    run
}

/// Warm a ready snapshot in the background. Does not auto-start Sandboxes
/// listed in running.json; Start is a user action that restores that
/// Sandbox's saved machine state.
pub fn resume_and_warm(state: AppState) {
    tokio::spawn(async move {
        let Some(vmm) = state.vmm.clone() else {
            return;
        };
        let cache = state.cache.clone();
        let root = state.store.root().to_path_buf();
        let _ = tokio::task::spawn_blocking(move || wait_ready_snapshot(&vmm, &cache, &root)).await;
    });
}

pub async fn on_quit(state: AppState) {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => {
                    let _ = ctrl_c.await;
                    return save_running(state).await;
                }
            };
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
    save_running(state).await;
}

async fn save_running(state: AppState) {
    let Some(vmm) = state.vmm.clone() else {
        return;
    };
    eprintln!("writing machine state");
    for id in state.resume.ids() {
        if state.store.get(id).map(|s| s.state).ok() != Some(SandboxState::Running) {
            continue;
        }
        let store = state.store.clone();
        let vmm = vmm.clone();
        let resume = state.resume.clone();
        match tokio::task::spawn_blocking(move || halt(store, vmm, resume, id, false)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => eprintln!("sandbox {id}: quit save ({e})"),
            Err(e) => eprintln!("sandbox {id}: quit save join ({e})"),
        }
    }
}

fn hypervisor(state: &AppState) -> Result<Arc<Hypervisor>, (StatusCode, Json<serde_json::Value>)> {
    state
        .vmm
        .clone()
        .ok_or_else(|| map_err(ActionError::Failed("no hypervisor".into())))
}

fn action(
    result: Result<crate::sandbox::Sandbox, ActionError>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sandbox = result.map_err(map_err)?;
    Ok(Json(serde_json::to_value(sandbox).expect("sandbox json")))
}

pub(crate) fn map_err(err: ActionError) -> (StatusCode, Json<serde_json::Value>) {
    match err {
        ActionError::NotFound => error_body(StatusCode::NOT_FOUND, "not_found", None),
        ActionError::Conflict(detail) => error_body(StatusCode::CONFLICT, "conflict", Some(detail)),
        ActionError::BadRequest(detail) => {
            error_body(StatusCode::BAD_REQUEST, "bad_request", Some(detail))
        }
        ActionError::Failed(detail) if detail == "no hypervisor" => {
            error_body(StatusCode::SERVICE_UNAVAILABLE, "failed", Some(&detail))
        }
        ActionError::Failed(detail) => {
            error_body(StatusCode::INTERNAL_SERVER_ERROR, "internal", Some(&detail))
        }
        ActionError::Internal => error_body(StatusCode::INTERNAL_SERVER_ERROR, "internal", None),
    }
}

pub fn error_body(
    status: StatusCode,
    error: &str,
    detail: Option<&str>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut body = serde_json::json!({ "error": error });
    if let Some(detail) = detail {
        body["detail"] = serde_json::Value::String(detail.to_string());
    }
    (status, Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        response::Html,
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn harness() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState {
            token: "test-token".into(),
            store: Arc::new(Store::open(dir.path()).unwrap()),
            cache: Arc::new(Cache::open(dir.path().join("cache")).unwrap()),
            layout: Arc::new(LayoutStore::open(dir.path().join("layout.json")).unwrap()),
            publish: crate::publish::Publisher::default(),
            sessions: crate::pty::Sessions::default(),
            templates: Arc::new(crate::templates::Library {
                shipped: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../environment"),
                user: dir.path().join("templates"),
            }),
            resume: Arc::new(crate::resume::Resume::open(dir.path().join("running.json"))),
            vmm: None,
            agent_options: Arc::new(Ok(serde_json::json!({
                "programs": [
                    {"name": "claude-code", "description": "Claude Code", "options": []},
                    {"name": "codex", "description": "Codex", "options": []},
                    {"name": "pi-coding-agent", "description": "Pi", "options": []}
                ]
            }))),
        };
        (dir, state)
    }

    async fn send(app: Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = app.oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, json)
    }

    fn authed(builder: axum::http::request::Builder) -> axum::http::request::Builder {
        builder.header("Authorization", "Bearer test-token")
    }

    #[tokio::test]
    async fn no_token_is_unauthorized() {
        let (_dir, state) = harness();
        let (status, json) = send(
            router(state),
            Request::get("http://127.0.0.1/api/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["error"], "unauthorized");
    }

    #[tokio::test]
    async fn wrong_token_is_unauthorized() {
        let (_dir, state) = harness();
        let (status, json) = send(
            router(state),
            Request::get("http://127.0.0.1/api/v1/health")
                .header("Authorization", "Bearer nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["error"], "unauthorized");
    }

    #[tokio::test]
    async fn health_ok() {
        let (_dir, state) = harness();
        let (status, json) = send(
            router(state),
            authed(Request::get("http://127.0.0.1/api/v1/health"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["ok"], true);
    }

    #[tokio::test]
    async fn sandbox_lifecycle() {
        let (_dir, state) = harness();
        let make = || router(state.clone());

        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"work"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["name"], "work");
        assert_eq!(created["state"], "stopped");
        let id = created["id"].as_str().unwrap().to_string();

        let (status, listed) = send(
            make(),
            authed(Request::get("http://127.0.0.1/api/v1/sandboxes"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed["sandboxes"].as_array().unwrap().len(), 1);

        let start = format!("http://127.0.0.1/api/v1/sandboxes/{id}/start");
        let (status, failed) = send(
            make(),
            authed(Request::post(&start)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(failed["error"], "failed");
        assert_eq!(failed["detail"], "no hypervisor");

        let stop = format!("http://127.0.0.1/api/v1/sandboxes/{id}/stop");
        let (status, failed) = send(
            make(),
            authed(Request::post(&stop)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(failed["error"], "failed");

        let reset = format!("http://127.0.0.1/api/v1/sandboxes/{id}/reset");
        let (status, after_reset) = send(
            make(),
            authed(Request::post(&reset)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(after_reset["state"], "stopped");

        let url = format!("http://127.0.0.1/api/v1/sandboxes/{id}");
        let (status, _) = send(
            make(),
            authed(Request::delete(&url)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, json) = send(
            make(),
            authed(Request::get(&url)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["error"], "not_found");
    }

    #[tokio::test]
    async fn create_and_patch_limits() {
        let (_dir, state) = harness();
        let make = || router(state.clone());

        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["limits"]["cpu"].as_u64(), Some(2));
        assert_eq!(
            created["limits"]["ram"].as_u64(),
            Some(2 * 1024 * 1024 * 1024)
        );
        assert_eq!(
            created["limits"]["disk"].as_u64(),
            Some(16 * 1024 * 1024 * 1024)
        );

        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"fat","limits":{"cpu":4,"ram":4294967296}}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["name"], "fat");
        assert_eq!(created["limits"]["cpu"].as_u64(), Some(4));
        assert_eq!(
            created["limits"]["ram"].as_u64(),
            Some(4 * 1024 * 1024 * 1024)
        );
        assert_eq!(
            created["limits"]["disk"].as_u64(),
            Some(16 * 1024 * 1024 * 1024)
        );

        let id = created["id"].as_str().unwrap();
        let url = format!("http://127.0.0.1/api/v1/sandboxes/{id}");
        let (status, patched) = send(
            make(),
            authed(Request::patch(&url))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"limits":{"disk":34359738368}}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(patched["limits"]["cpu"].as_u64(), Some(4));
        assert_eq!(
            patched["limits"]["disk"].as_u64(),
            Some(32 * 1024 * 1024 * 1024)
        );

        let (status, bad) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"limits":{"cpu":0}}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(bad["error"], "bad_request");
    }

    #[tokio::test]
    async fn environment_is_home_manager_config() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = created["id"].as_str().unwrap();
        let url = format!("http://127.0.0.1/api/v1/sandboxes/{id}/environment");
        let (status, listed) = send(
            make(),
            authed(Request::get(&url)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed["programs"]["claude-code"]["enable"], false);

        let (status, patched) = send(
            make(),
            authed(Request::put(&url))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"programs":{"claude-code":{"enable":true},"codex":{"enable":false},"pi-coding-agent":{"enable":false}}}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(patched["programs"]["claude-code"]["enable"], true);
    }

    #[tokio::test]
    async fn create_environment_is_the_reset_snapshot() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"environment":{"programs":{"claude-code":{"enable":true},"codex":{"enable":false},"pi-coding-agent":{"enable":false}}}}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = created["id"].as_str().unwrap();
        let url = format!("http://127.0.0.1/api/v1/sandboxes/{id}/environment");
        let (status, patched) = send(
            make(),
            authed(Request::put(&url))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"programs":{"claude-code":{"enable":false},"codex":{"enable":true},"pi-coding-agent":{"enable":false}}}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(patched["programs"]["codex"]["enable"], true);
        let (status, _) = send(
            make(),
            authed(Request::post(format!(
                "http://127.0.0.1/api/v1/sandboxes/{id}/reset"
            )))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, listed) = send(
            make(),
            authed(Request::get(&url)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed["programs"]["claude-code"]["enable"], true);
        assert_eq!(listed["programs"]["codex"]["enable"], false);
    }

    #[tokio::test]
    async fn templates_ship_empty() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        let (status, listed) = send(
            make(),
            authed(Request::get("http://127.0.0.1/api/v1/templates"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = listed["templates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"empty"));
        assert!(!names.contains(&"python"));
        assert!(!names.contains(&"rust"));
    }

    #[tokio::test]
    async fn publish_refused_while_stopped() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = created["id"].as_str().unwrap();
        let (status, err) = send(
            make(),
            authed(Request::post(format!(
                "http://127.0.0.1/api/v1/sandboxes/{id}/publish"
            )))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"port":3000}"#))
            .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(err["error"], "conflict");
    }

    #[tokio::test]
    async fn agent_options_are_home_manager_programs() {
        let (_dir, state) = harness();
        let (status, json) = send(
            router(state),
            authed(Request::get("http://127.0.0.1/api/v1/agent-options"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = json["programs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"claude-code"));
        assert!(names.contains(&"codex"));
        assert!(names.contains(&"pi-coding-agent"));
    }

    #[tokio::test]
    async fn agent_options_failed_dump_is_unavailable() {
        let (_dir, mut state) = harness();
        state.agent_options = Arc::new(Err("eval failed".into()));
        let (status, json) = send(
            router(state),
            authed(Request::get("http://127.0.0.1/api/v1/agent-options"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json["error"], "failed");
        assert_eq!(json["detail"], "eval failed");
    }

    #[tokio::test]
    async fn cookie_token_is_accepted() {
        let (_dir, state) = harness();
        let (status, json) = send(
            router(state),
            Request::get("http://127.0.0.1/api/v1/health")
                .header("Cookie", "snowbox=test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["ok"], true);
    }

    #[tokio::test]
    async fn layout_windows_persist() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"work"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = created["id"].as_str().unwrap();
        let url = format!("http://127.0.0.1/api/v1/sandboxes/{id}/windows");
        let (status, win) = send(
            make(),
            authed(Request::post(&url)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(win["sandbox"], id);
        let wid = win["id"].as_str().unwrap();

        let (status, layout) = send(
            make(),
            authed(Request::get("http://127.0.0.1/api/v1/layout"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(layout["windows"].as_array().unwrap().len(), 1);

        let del = format!("http://127.0.0.1/api/v1/windows/{wid}");
        let (status, _) = send(
            make(),
            authed(Request::delete(&del)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn two_sandboxes_exist_at_once() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        for name in ["one", "two"] {
            let (status, created) = send(
                make(),
                authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"name":"{name}"}}"#)))
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
            assert_eq!(created["state"], "stopped");
        }
        let (status, listed) = send(
            make(),
            authed(Request::get("http://127.0.0.1/api/v1/sandboxes"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed["sandboxes"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn copy_in_out_replace() {
        let (dir, state) = harness();
        let make = || router(state.clone());
        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = created["id"].as_str().unwrap();
        assert!(created["home"].as_array().unwrap().is_empty());

        let src = dir.path().join("proj");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("a"), "1").unwrap();
        let copy_in = format!("http://127.0.0.1/api/v1/sandboxes/{id}/copy-in");
        let body = serde_json::json!({ "from": src, "replace": false });
        let (status, _) = send(
            make(),
            authed(Request::post(&copy_in))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let dest = dir.path().join("out");
        let copy_out = format!("http://127.0.0.1/api/v1/sandboxes/{id}/copy-out");
        let body = serde_json::json!({ "to": dest, "replace": false });
        let (status, _) = send(
            make(),
            authed(Request::post(&copy_out))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(std::fs::read_to_string(dest.join("a")).unwrap(), "1");
    }

    fn with_session(state: AppState) -> Router {
        router(state.clone()).layer(middleware::from_fn_with_state(
            state,
            crate::auth::attach_session,
        ))
    }

    #[tokio::test]
    async fn cookie_not_set_on_unauthenticated_get() {
        let (_dir, state) = harness();
        let app = router(state.clone())
            .fallback(|| async { Html("<html></html>") })
            .layer(middleware::from_fn_with_state(
                state,
                crate::auth::attach_session,
            ));
        let response = app
            .oneshot(
                Request::get("http://127.0.0.1/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("set-cookie").is_none());
    }

    #[tokio::test]
    async fn cookie_set_on_authenticated_request() {
        let (_dir, state) = harness();
        let app = with_session(state);
        let response = app
            .oneshot(
                authed(Request::get("http://127.0.0.1/api/v1/health"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cookie.starts_with("snowbox=test-token;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
    }

    #[tokio::test]
    async fn websocket_missing_origin_is_forbidden() {
        let (_dir, state) = harness();
        let id = Uuid::nil();
        let (status, json) = send(
            router(state),
            authed(Request::get(format!(
                "http://127.0.0.1/api/v1/windows/{id}/pty"
            )))
            .header("Upgrade", "websocket")
            .header("Connection", "upgrade")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13")
            .body(Body::empty())
            .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(json["error"], "forbidden");
    }

    #[tokio::test]
    async fn websocket_daemon_origin_is_not_forbidden() {
        let (_dir, state) = harness();
        let id = Uuid::nil();
        let (status, json) = send(
            router(state),
            authed(Request::get(format!(
                "http://127.0.0.1/api/v1/windows/{id}/pty"
            )))
            .header("Upgrade", "websocket")
            .header("Connection", "upgrade")
            .header("Origin", format!("http://127.0.0.1:{LISTEN_PORT}"))
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13")
            .body(Body::empty())
            .unwrap(),
        )
        .await;
        assert_ne!(status, StatusCode::FORBIDDEN);
        assert_ne!(json["error"], "forbidden");
    }

    #[tokio::test]
    async fn websocket_wrong_origin_is_forbidden() {
        let (_dir, state) = harness();
        let id = Uuid::nil();
        let (status, json) = send(
            router(state),
            authed(Request::get(format!(
                "http://127.0.0.1/api/v1/windows/{id}/pty"
            )))
            .header("Upgrade", "websocket")
            .header("Connection", "upgrade")
            .header("Origin", "http://evil.example:5418")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13")
            .body(Body::empty())
            .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(json["error"], "forbidden");
    }

    #[tokio::test]
    async fn start_without_hypervisor_fails() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = created["id"].as_str().unwrap();
        let (status, json) = send(
            make(),
            authed(Request::post(format!(
                "http://127.0.0.1/api/v1/sandboxes/{id}/start"
            )))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json["error"], "failed");
        assert_eq!(json["detail"], "no hypervisor");
        let got = state.store.get(id.parse().unwrap()).unwrap();
        assert_eq!(got.state, SandboxState::Stopped);
    }

    #[tokio::test]
    async fn reset_while_running_without_hypervisor_fails() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        let (status, created) = send(
            make(),
            authed(Request::post("http://127.0.0.1/api/v1/sandboxes"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
        state.store.start(id).unwrap();
        let (status, json) = send(
            make(),
            authed(Request::post(format!(
                "http://127.0.0.1/api/v1/sandboxes/{id}/reset"
            )))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json["error"], "failed");
        assert_eq!(json["detail"], "no hypervisor");
        assert_eq!(state.store.get(id).unwrap().state, SandboxState::Running);
    }

    #[test]
    fn hatch_drops_environment_applied() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("environment.applied"), "stamp").unwrap();
        std::fs::write(dir.path().join(crate::vmm::HATCHED), b"").unwrap();
        assert!(forget_hatch_applied(dir.path()));
        assert!(!dir.path().join("environment.applied").exists());
        assert!(!dir.path().join(crate::vmm::HATCHED).exists());
        assert!(!forget_hatch_applied(dir.path()));
    }
}
