use axum::{
    extract::{Request, State},
    http::{
        StatusCode,
        header::{AUTHORIZATION, COOKIE},
    },
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
    let presented = bearer(&request).or_else(|| cookie_token(&request));
    let ok = presented.is_some_and(|t| constant_time_eq(t.as_bytes(), state.token.as_bytes()));
    if ok {
        Ok(next.run(request).await)
    } else {
        Err(error_body(StatusCode::UNAUTHORIZED, "unauthorized", None))
    }
}

fn bearer(request: &Request) -> Option<&str> {
    request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
}

fn cookie_token(request: &Request) -> Option<&str> {
    let raw = request.headers().get(COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|part| {
        let part = part.trim();
        part.strip_prefix("snowbox=")
    })
}
