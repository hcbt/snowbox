use axum::{
    extract::{Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::Response,
};
use constant_time_eq::constant_time_eq;

use crate::api::{AppState, error_body};

pub async fn require_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, axum::Json<serde_json::Value>)> {
    let header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    let presented = header.and_then(|h| h.strip_prefix("Bearer "));
    let ok = presented.is_some_and(|t| constant_time_eq(t.as_bytes(), state.token.as_bytes()));
    if ok {
        Ok(next.run(request).await)
    } else {
        Err(error_body(StatusCode::UNAUTHORIZED, "unauthorized", None))
    }
}
