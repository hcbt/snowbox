use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use serde::Deserialize;
use std::path::PathBuf;
use uuid::Uuid;

use crate::auth::require_token;
use crate::cache::Cache;
use crate::catalog::Catalog;
use crate::layout::{Layout, LayoutStore};
use crate::sandbox::{ActionError, Limits, State as SandboxState, Store};
use crate::vz::Hypervisor;

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub store: Arc<Store>,
    pub cache: Arc<Cache>,
    pub layout: Arc<LayoutStore>,
    pub catalog: Arc<Catalog>,
    pub vmm: Option<Arc<Hypervisor>>,
}

#[derive(Deserialize, Default)]
pub struct CreateBody {
    pub name: Option<String>,
    #[serde(default)]
    pub limits: LimitsPatch,
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

#[derive(Deserialize)]
pub struct AddPackageBody {
    pub add: String,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default)]
    pub unfree: bool,
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
        .route("/packages", get(search_packages))
        .route(
            "/sandboxes/{id}/packages",
            get(list_packages).post(add_package),
        )
        .route("/layout", get(get_layout).put(put_layout))
        .route("/sandboxes/{id}/windows", post(open_window))
        .route("/windows/{id}", axum::routing::delete(close_window))
        .route("/windows/{id}/pty", get(crate::pty::upgrade))
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
    let sandbox = state
        .store
        .create_with(body.name, limits)
        .map_err(map_err)?;
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
    if let Some(vmm) = state.vmm.clone() {
        let store = state.store.clone();
        let cache = state.cache.clone();
        let result = tokio::task::spawn_blocking(move || boot(store, cache, vmm, id))
            .await
            .map_err(|_| ActionError::Internal)
            .and_then(|r| r);
        return action(result);
    }
    action(state.store.start(id))
}

async fn stop(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Some(vmm) = state.vmm.clone() {
        let store = state.store.clone();
        let result = tokio::task::spawn_blocking(move || halt(store, vmm, id))
            .await
            .map_err(|_| ActionError::Internal)
            .and_then(|r| r);
        return action(result);
    }
    action(state.store.stop(id))
}

async fn reset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    action(state.store.reset(id))
}

async fn destroy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if let Some(vmm) = state.vmm.clone() {
        let store = state.store.clone();
        let result = tokio::task::spawn_blocking(move || {
            if store.get(id).map(|s| s.state).ok() == Some(SandboxState::Running) {
                let _ = halt(store.clone(), vmm, id);
            }
            store.destroy(id)
        })
        .await
        .map_err(|_| ActionError::Internal)
        .and_then(|r| r);
        result.map_err(map_err)?;
        let _ = state.layout.close_sandbox_windows(id);
        return Ok(StatusCode::NO_CONTENT);
    }
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

async fn search_packages(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let catalog = state.catalog.clone();
    let q = query.q;
    let unfree = query.unfree;
    let hits = tokio::task::spawn_blocking(move || catalog.search(&q, unfree, 30))
        .await
        .map_err(|_| ActionError::Internal)
        .and_then(|r| r)
        .map_err(map_err)?;
    Ok(Json(serde_json::json!({ "packages": hits })))
}

async fn list_packages(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let _ = state.store.get(id).map_err(map_err)?;
    let pkgs = crate::environment::packages(&state.store.dir(id)).map_err(map_err)?;
    Ok(Json(serde_json::json!({ "packages": pkgs })))
}

async fn add_package(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddPackageBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sandbox = state.store.get(id).map_err(map_err)?;
    let pkgs =
        crate::environment::add_package(&state.store.dir(id), body.add.trim()).map_err(map_err)?;
    if sandbox.state == SandboxState::Running {
        if let Some(vmm) = state.vmm.clone() {
            let store = state.store.clone();
            let cache = state.cache.clone();
            tokio::task::spawn_blocking(move || apply_env(&store, &cache, &vmm, id))
                .await
                .map_err(|_| ActionError::Internal)
                .and_then(|r| r)
                .map_err(map_err)?;
        }
    }
    Ok(Json(serde_json::json!({ "packages": pkgs })))
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
    Ok(StatusCode::NO_CONTENT)
}

fn boot(
    store: Arc<Store>,
    cache: Arc<Cache>,
    vmm: Arc<Hypervisor>,
    id: Uuid,
) -> Result<crate::sandbox::Sandbox, ActionError> {
    store.begin_boot(id)?;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        boot_claimed(&store, &cache, &vmm, id)
    }));
    match outcome {
        Ok(Ok(sandbox)) => Ok(sandbox),
        Ok(Err(err)) => {
            let _ = vmm.stop(id);
            store.abort_boot(id);
            Err(err)
        }
        Err(_) => {
            let _ = vmm.stop(id);
            store.abort_boot(id);
            Err(ActionError::Internal)
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
    let kind = vmm
        .start(id, &dir, sandbox.limits)
        .map_err(ActionError::Failed)?;
    eprintln!(
        "sandbox {id}: hypervisor {}ms ({kind:?})",
        t0.elapsed().as_millis()
    );
    let t1 = std::time::Instant::now();
    let ready_for = match kind {
        crate::vz::StartKind::Restored => std::time::Duration::from_secs(8),
        crate::vz::StartKind::Cold => std::time::Duration::from_secs(90),
    };
    if let Err(e) = crate::agent::wait_ready(vmm, id, ready_for) {
        if kind == crate::vz::StartKind::Restored {
            eprintln!("sandbox {id}: restored agent dead ({e}); booting");
            let _ = vmm.stop(id);
            let _ = std::fs::remove_file(dir.join(crate::vz::SAVE_NAME));
            vmm.start_cold(id, &dir, sandbox.limits)
                .map_err(ActionError::Failed)?;
            crate::agent::wait_ready(vmm, id, std::time::Duration::from_secs(90))
                .map_err(ActionError::Failed)?;
        } else {
            return Err(ActionError::Failed(e));
        }
    }
    eprintln!("sandbox {id}: agent {}ms", t1.elapsed().as_millis());
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
    id: Uuid,
) -> Result<crate::sandbox::Sandbox, ActionError> {
    let sandbox = store.get(id)?;
    if sandbox.state != SandboxState::Running {
        return Err(ActionError::Conflict("already stopped"));
    }
    let dir = store.dir(id);
    let _ = crate::agent::tar_out(&vmm, id, "/workspace", &dir.join("workspace"));
    let _ = crate::agent::tar_out(&vmm, id, "/home/snow", &dir.join("home"));
    let save = dir.join(crate::vz::SAVE_NAME);
    if let Err(e) = vmm.save_and_stop(id, &save) {
        eprintln!("sandbox {id}: save failed ({e}); power off");
        let _ = std::fs::remove_file(&save);
        vmm.stop(id).map_err(ActionError::Failed)?;
    }
    store.stop(id)
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
            catalog: Arc::new(crate::catalog::Catalog::memory(vec![
                crate::catalog::Package {
                    name: "jq".into(),
                    program: "jq".into(),
                    description: "Lightweight and flexible command-line JSON processor".into(),
                    unfree: false,
                },
                crate::catalog::Package {
                    name: "ripgrep".into(),
                    program: "rg".into(),
                    description: "fast line-oriented search tool".into(),
                    unfree: false,
                },
                crate::catalog::Package {
                    name: "unrar".into(),
                    program: "unrar".into(),
                    description: "RAR archive tool".into(),
                    unfree: true,
                },
            ])),
            vmm: None,
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
        let (status, running) = send(
            make(),
            authed(Request::post(&start)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(running["state"], "running");

        let (status, conflict) = send(
            make(),
            authed(Request::post(&start)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(conflict["error"], "conflict");

        let reset = format!("http://127.0.0.1/api/v1/sandboxes/{id}/reset");
        let (status, after_reset) = send(
            make(),
            authed(Request::post(&reset)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(after_reset["state"], "running");

        let stop = format!("http://127.0.0.1/api/v1/sandboxes/{id}/stop");
        let (status, stopped) = send(
            make(),
            authed(Request::post(&stop)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(stopped["state"], "stopped");

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
    async fn packages_on_host_environment() {
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
        let url = format!("http://127.0.0.1/api/v1/sandboxes/{id}/packages");
        let (status, listed) = send(
            make(),
            authed(Request::get(&url)).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            listed["packages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p == "hello")
        );

        let (status, added) = send(
            make(),
            authed(Request::post(&url))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"add":"jq"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            added["packages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p == "jq")
        );
    }

    #[tokio::test]
    async fn package_search_is_by_program_and_hides_unfree() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        let (status, hits) = send(
            make(),
            authed(Request::get("http://127.0.0.1/api/v1/packages?q=rg"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let list = hits["packages"].as_array().unwrap();
        assert!(
            list.iter()
                .any(|p| p["program"] == "rg" && p["name"] == "ripgrep")
        );
        assert!(list.iter().all(|p| p["unfree"] == false));

        let (status, json) = send(
            make(),
            authed(Request::get("http://127.0.0.1/api/v1/packages?q=json"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            json["packages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "jq")
        );

        let (status, hidden) = send(
            make(),
            authed(Request::get("http://127.0.0.1/api/v1/packages?q=unrar"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(hidden["packages"].as_array().unwrap().is_empty());

        let (status, shown) = send(
            make(),
            authed(Request::get(
                "http://127.0.0.1/api/v1/packages?q=unrar&unfree=true",
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            shown["packages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["unfree"] == true && p["name"] == "unrar")
        );
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
    async fn two_sandboxes_can_run_at_once() {
        let (_dir, state) = harness();
        let make = || router(state.clone());
        let mut ids = Vec::new();
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
            ids.push(created["id"].as_str().unwrap().to_string());
        }
        for id in &ids {
            let start = format!("http://127.0.0.1/api/v1/sandboxes/{id}/start");
            let (status, running) = send(
                make(),
                authed(Request::post(&start)).body(Body::empty()).unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(running["state"], "running");
        }
        let (status, listed) = send(
            make(),
            authed(Request::get("http://127.0.0.1/api/v1/sandboxes"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let running = listed["sandboxes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["state"] == "running")
            .count();
        assert_eq!(running, 2);
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
        assert_eq!(created["home"][0], ".gitconfig");

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
}
