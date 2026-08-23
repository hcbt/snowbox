use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use axum::{
    extract::{Request, State},
    http::{HeaderValue, header::SET_COOKIE},
    middleware::{self, Next},
    response::{Html, Response},
};
use rand::Rng;

mod agent;
mod api;
mod auth;
mod cache;
mod environment;
mod layout;
mod nar;
mod nix;
mod pty;
mod runtime;
mod sandbox;
mod sign;
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
    SocketAddr::from(([127, 0, 0, 1], 5418))
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
    vz::pump_main_run_loop();
    Ok(())
}

async fn run_daemon() -> Result<()> {
    let token_path = token_path()?;
    let token = load_or_create_token(&token_path)?;

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
        Arc::new(vz::Hypervisor::new(rt))
    });
    if vmm.is_none() {
        eprintln!("runtime missing (build guest, or set SNOWBOX_RUNTIME)");
    }
    eprintln!("cache {}", cache.root().display());
    let state = api::AppState {
        token,
        store: Arc::new(store),
        cache: Arc::new(cache),
        layout: Arc::new(layout),
        vmm,
    };
    let app = with_ui(api::router(state.clone()), state.clone()).layer(
        middleware::from_fn_with_state(state.clone(), attach_session),
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
        if vz::is_supported() {
            "supported"
        } else {
            "unsupported"
        }
    );
    open_browser(&url);

    axum::serve(listener, app).await.context("serve")?;
    Ok(())
}

fn with_ui(router: axum::Router, _state: api::AppState) -> axum::Router {
    if let Some(dir) = ui_dir() {
        use tower_http::services::{ServeDir, ServeFile};
        let index = dir.join("index.html");
        router.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
    } else {
        router.fallback(canvas)
    }
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

async fn attach_session(
    State(state): State<api::AppState>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let cookie = format!("snowbox={}; Path=/; HttpOnly; SameSite=Strict", state.token);
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(SET_COOKIE, value);
    }
    response
}

async fn canvas() -> Html<&'static str> {
    Html(FALLBACK_CANVAS)
}

fn token_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config directory")?;
    Ok(base.join("snowbox").join("token"))
}

fn load_or_create_token(path: &Path) -> Result<String> {
    if path.exists() {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        return Ok(raw.trim().to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let bytes: [u8; 32] = rand::rng().random();
    let token = hex::encode(bytes);
    std::fs::write(path, &token).with_context(|| format!("write {}", path.display()))?;
    Ok(token)
}

fn open_browser(url: &str) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(url).spawn();
}
