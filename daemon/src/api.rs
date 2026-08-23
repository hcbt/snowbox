use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::require_token;
use crate::sandbox::{ActionError, Store};

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub store: Arc<Store>,
}

#[derive(Deserialize, Default)]
pub struct CreateBody {
    pub name: Option<String>,
}

pub fn router(state: AppState) -> Router {
    let v1 = Router::new()
        .route("/health", get(health))
        .route("/sandboxes", get(list).post(create))
        .route("/sandboxes/{id}", get(get_one).delete(destroy))
        .route("/sandboxes/{id}/start", post(start))
        .route("/sandboxes/{id}/stop", post(stop))
        .route("/sandboxes/{id}/reset", post(reset))
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
) -> (StatusCode, Json<serde_json::Value>) {
    let name = body.and_then(|Json(b)| b.name);
    let sandbox = state.store.create(name);
    (
        StatusCode::CREATED,
        Json(serde_json::to_value(sandbox).expect("sandbox json")),
    )
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
    action(state.store.start(id))
}

async fn stop(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
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
    state.store.destroy(id).map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

fn action(
    result: Result<crate::sandbox::Sandbox, ActionError>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sandbox = result.map_err(map_err)?;
    Ok(Json(serde_json::to_value(sandbox).expect("sandbox json")))
}

fn map_err(err: ActionError) -> (StatusCode, Json<serde_json::Value>) {
    match err {
        ActionError::NotFound => error_body(StatusCode::NOT_FOUND, "not_found", None),
        ActionError::Conflict(detail) => error_body(StatusCode::CONFLICT, "conflict", Some(detail)),
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

    fn app() -> Router {
        router(AppState {
            token: "test-token".into(),
            store: Arc::new(Store::new()),
        })
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
        let (status, json) = send(
            app(),
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
        let (status, json) = send(
            app(),
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
        let (status, json) = send(
            app(),
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
        let state = AppState {
            token: "test-token".into(),
            store: Arc::new(Store::new()),
        };
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
}
