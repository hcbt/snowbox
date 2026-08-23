use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use axum::response::Html;
use rand::Rng;

mod agent;
mod api;
mod auth;
mod cache;
mod environment;
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
        vmm,
    };
    let app = api::router(state).fallback(canvas);

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
