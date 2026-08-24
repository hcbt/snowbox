use std::path::Path;

use anyhow::{Context, Result};
use axum::{
    extract::{Request, State},
    http::{
        HeaderValue, StatusCode,
        header::{AUTHORIZATION, COOKIE, ORIGIN, SET_COOKIE, UPGRADE},
    },
    middleware::Next,
    response::Response,
};
use constant_time_eq::constant_time_eq;
use rand::Rng;

use crate::api::{AppState, error_body, listen_port};

pub async fn require_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, axum::Json<serde_json::Value>)> {
    if request_has_token(&request, &state.token) {
        Ok(next.run(request).await)
    } else {
        Err(error_body(StatusCode::UNAUTHORIZED, "unauthorized", None))
    }
}

/// Set-Cookie only when the request already presented a valid token.
/// Unauthenticated GET / must not mint loopback auth.
pub async fn attach_session(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let authed = request_has_token(&request, &state.token);
    let mut response = next.run(request).await;
    if authed {
        let cookie = format!("snowbox={}; Path=/; HttpOnly; SameSite=Strict", state.token);
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(SET_COOKIE, value);
        }
    }
    response
}

/// Window WS upgrades must come from this Daemon's Canvas origin.
pub async fn require_ws_origin(
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, axum::Json<serde_json::Value>)> {
    let is_ws = request
        .headers()
        .get(UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.eq_ignore_ascii_case("websocket"));
    if !is_ws {
        return Ok(next.run(request).await);
    }
    let origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if origin_allowed(origin, listen_port()) {
        Ok(next.run(request).await)
    } else {
        Err(error_body(StatusCode::FORBIDDEN, "forbidden", None))
    }
}

pub fn origin_allowed(origin: &str, port: u16) -> bool {
    let Ok(uri) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    if uri.scheme_str() != Some("http") {
        return false;
    }
    let Some(host) = uri.host() else {
        return false;
    };
    if host != "127.0.0.1" && !host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    if uri.port_u16() != Some(port) {
        return false;
    }
    matches!(uri.path(), "" | "/")
}

pub fn embed_token_in_canvas(html: &str, token: &str) -> String {
    let encoded = serde_json::to_string(token).unwrap_or_else(|_| "\"\"".into());
    let script = format!("<script>window.__SNOWBOX_TOKEN__={encoded};</script>");
    if let Some(i) = html.find("</head>") {
        let mut out = String::with_capacity(html.len() + script.len());
        out.push_str(&html[..i]);
        out.push_str(&script);
        out.push_str(&html[i..]);
        out
    } else {
        format!("{script}{html}")
    }
}

pub fn load_or_create_token(path: &Path) -> Result<String> {
    if path.exists() {
        tighten_token_mode(path)?;
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        return Ok(raw.trim().to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let bytes: [u8; 32] = rand::rng().random();
    let token = hex::encode(bytes);
    write_token_0600(path, &token)?;
    Ok(token)
}

fn request_has_token(request: &Request, token: &str) -> bool {
    presented(request).is_some_and(|t| constant_time_eq(t.as_bytes(), token.as_bytes()))
}

fn presented(request: &Request) -> Option<&str> {
    bearer(request).or_else(|| cookie_token(request))
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

#[cfg(unix)]
fn write_token_0600(path: &Path, token: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("write {}", path.display()))?;
    f.write_all(token.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_token_0600(path: &Path, token: &str) -> Result<()> {
    std::fs::write(path, token).with_context(|| format!("write {}", path.display()))
}

#[cfg(unix)]
fn tighten_token_mode(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("chmod {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_token_mode(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn origin_this_daemon_only() {
        assert!(origin_allowed("http://127.0.0.1:5418", 5418));
        assert!(origin_allowed("http://localhost:5418", 5418));
        assert!(origin_allowed("http://127.0.0.1:5418/", 5418));
        assert!(!origin_allowed("http://127.0.0.1:5418", 80));
        assert!(!origin_allowed("http://127.0.0.1", 5418));
        assert!(!origin_allowed("http://evil.example:5418", 5418));
        assert!(!origin_allowed("https://127.0.0.1:5418", 5418));
        assert!(!origin_allowed("", 5418));
        assert!(!origin_allowed("http://127.0.0.1:9999", 5418));
        assert!(!origin_allowed("http://127.0.0.1:5418/pty", 5418));
    }

    #[test]
    fn embed_token_inserts_script_before_head_close() {
        let html = "<html><head><title>x</title></head><body></body></html>";
        let out = embed_token_in_canvas(html, "deadbeef");
        assert!(out.contains(r#"<script>window.__SNOWBOX_TOKEN__="deadbeef";</script>"#));
        assert!(out.contains("</head>"));
        assert!(out.find("__SNOWBOX_TOKEN__").unwrap() < out.find("</head>").unwrap());
    }

    #[test]
    fn embed_token_prepends_when_no_head() {
        let out = embed_token_in_canvas("<p>hi</p>", "ab");
        assert!(out.starts_with(r#"<script>window.__SNOWBOX_TOKEN__="ab";</script>"#));
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_mode_0600_on_create() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        let token = load_or_create_token(&path).unwrap();
        assert_eq!(token.len(), 64);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(load_or_create_token(&path).unwrap(), token);
    }

    #[cfg(unix)]
    #[test]
    fn token_file_loose_perms_are_tightened() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        fs::write(&path, "already-there").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&path, perms).unwrap();
        let token = load_or_create_token(&path).unwrap();
        assert_eq!(token, "already-there");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
