use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    body::Body,
    http::{StatusCode, Uri, header::CONTENT_TYPE},
    middleware,
    response::{Html, IntoResponse, Response},
};

mod agent;
mod api;
mod auth;
mod cache;
mod disk;
mod environment;
mod kvm;
mod layout;
#[cfg(test)]
mod nar;
mod nix;
mod progress;
mod pty;
mod publish;
mod ready;
mod resume;
mod runtime;
mod sandbox;
mod sign;
mod templates;
mod vmm;
#[cfg(target_os = "macos")]
mod vz;

const FALLBACK_CANVAS: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>snowbox</title>
    <style>
      html, body { margin: 0; height: 100%; background: #0c0c0e; color: #8a8a93; font: 13px/1.4 ui-sans-serif, system-ui, sans-serif; }
      .canvas { height: 100%; }
      .mark { position: fixed; top: 12px; left: 14px; color: #c8c8ce; letter-spacing: 0.04em; }
    </style>
  </head>
  <body>
    <div class="canvas">
      <div class="mark">snowbox</div>
    </div>
  </body>
</html>
"#;

fn bind_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], api::LISTEN_PORT))
}

fn main() -> Result<()> {
    sign::ensure_signed();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;
    runtime.spawn(async {
        if let Err(e) = run_daemon().await {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    });
    vmm::pump_main_run_loop();
    Ok(())
}

async fn run_daemon() -> Result<()> {
    let token_path = token_path()?;
    let token = auth::load_or_create_token(&token_path)?;

    let data = dirs::data_dir()
        .context("no data directory")?
        .join("snowbox");
    let store = sandbox::Store::open(data.join("sandboxes")).context("open sandbox store")?;
    let cache = cache::Cache::open(data.join("cache")).context("open cache")?;
    let layout =
        layout::LayoutStore::open(data.join("layout.json")).context("open canvas layout")?;
    let vmm = runtime::Runtime::discover().map(|rt| {
        eprintln!(
            "runtime {}",
            rt.kernel.parent().unwrap_or(rt.kernel.as_path()).display()
        );
        vmm::attach(rt, data.clone())
    });
    if vmm.is_none() {
        eprintln!("runtime missing (build guest, or set SNOWBOX_RUNTIME)");
    }
    eprintln!("cache {}", cache.root().display());
    let agent_options = match environment::load_agent_schema() {
        Ok(value) => {
            eprintln!("agent options ready");
            Arc::new(Ok(value))
        }
        Err(e) => {
            eprintln!("agent options failed: {e}");
            Arc::new(Err(e))
        }
    };
    let state = api::AppState {
        token,
        store: Arc::new(store),
        cache: Arc::new(cache),
        layout: Arc::new(layout),
        templates: Arc::new(templates::Library {
            shipped: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../environment"),
            user: data.join("templates"),
        }),
        publish: publish::Publisher::default(),
        sessions: pty::Sessions::default(),
        progress: progress::Progress::new(),
        vmm,
        resume: Arc::new(resume::Resume::open(data.join("running.json"))),
        agent_options,
    };
    let app = with_ui(api::router(state.clone()), state.clone()).layer(
        middleware::from_fn_with_state(state.clone(), auth::attach_session),
    );

    let bind = bind_addr();
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("bind {bind}"))?;

    let url = format!("http://{bind}/");
    eprintln!("snowbox {url}");
    eprintln!("token {}", token_path.display());
    eprintln!(
        "virtualization {}",
        if vmm::is_supported() {
            "supported"
        } else {
            "unsupported"
        }
    );
    open_browser(&url);
    api::resume_and_warm(state.clone());

    axum::serve(listener, app)
        .with_graceful_shutdown(api::on_quit(state))
        .await
        .context("serve")?;
    Ok(())
}

fn with_ui(router: axum::Router, state: api::AppState) -> axum::Router {
    let token = state.token.clone();
    router.fallback(move |uri: Uri| {
        let token = token.clone();
        async move { serve_canvas(uri, &token).await }
    })
}

fn ui_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SNOWBOX_UI") {
        let p = PathBuf::from(p);
        if p.join("index.html").is_file() {
            return Some(p);
        }
    }
    let from_crate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../canvas/dist");
    if from_crate.join("index.html").is_file() {
        return Some(from_crate);
    }
    let from_cwd = PathBuf::from("canvas/dist");
    if from_cwd.join("index.html").is_file() {
        return Some(from_cwd);
    }
    None
}

async fn serve_canvas(uri: Uri, token: &str) -> Response {
    if let Some(dir) = ui_dir() {
        if let Some(path) = safe_ui_file(&dir, uri.path()) {
            if path.is_file() {
                if path.extension().and_then(|e| e.to_str()) == Some("html") {
                    return html_with_token(&path, token);
                }
                return file_response(&path);
            }
        }
        let index = dir.join("index.html");
        if index.is_file() {
            return html_with_token(&index, token);
        }
    }
    Html(auth::embed_token_in_canvas(FALLBACK_CANVAS, token)).into_response()
}

fn safe_ui_file(dir: &std::path::Path, url_path: &str) -> Option<PathBuf> {
    let rel = url_path.trim_start_matches('/');
    if rel.is_empty() {
        return Some(dir.join("index.html"));
    }
    let mut out = dir.to_path_buf();
    for comp in std::path::Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(c) => out.push(c),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

fn html_with_token(path: &std::path::Path, token: &str) -> Response {
    match std::fs::read_to_string(path) {
        Ok(html) => {
            let body = auth::embed_token_in_canvas(&html, token);
            ([(CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn file_response(path: &std::path::Path) -> Response {
    match std::fs::read(path) {
        Ok(bytes) => {
            let mime = match path.extension().and_then(|e| e.to_str()) {
                Some("js") => "text/javascript",
                Some("css") => "text/css",
                Some("svg") => "image/svg+xml",
                Some("png") => "image/png",
                Some("ico") => "image/x-icon",
                Some("woff2") => "font/woff2",
                Some("map" | "json") => "application/json",
                Some("html") => "text/html; charset=utf-8",
                _ => "application/octet-stream",
            };
            ([(CONTENT_TYPE, mime)], Body::from(bytes)).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn token_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config directory")?;
    Ok(base.join("snowbox").join("token"))
}

fn open_browser(url: &str) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(url).spawn();
}
