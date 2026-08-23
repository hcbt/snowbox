//! Window PTY: WebSocket on the Host, vsock into the guest. The browser
//! never talks to the Sandbox.

use std::sync::Arc;

use axum::{
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use crate::api::AppState;
use crate::sandbox::{ActionError, State as SandboxState};
use crate::vz;

pub async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, (axum::http::StatusCode, axum::Json<serde_json::Value>)> {
    let win = state.layout.window(id).map_err(crate::api::map_err)?;
    let sandbox = state.store.get(win.sandbox).map_err(crate::api::map_err)?;
    if sandbox.state != SandboxState::Running {
        return Err(crate::api::map_err(ActionError::Conflict(
            "sandbox is not running",
        )));
    }
    let Some(vmm) = state.vmm.clone() else {
        return Err(crate::api::map_err(ActionError::Failed(
            "no hypervisor".into(),
        )));
    };
    Ok(ws.on_upgrade(move |socket| pump(socket, vmm, win.sandbox)))
}

async fn pump(mut socket: WebSocket, vmm: Arc<vz::Hypervisor>, sandbox: Uuid) {
    let connected = tokio::task::spawn_blocking(move || vmm.vsock(sandbox, vz::SHELL_PORT)).await;
    let stream = match connected {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            let _ = socket
                .send(Message::Text(format!("shell: {e}").into()))
                .await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
        Err(_) => {
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };
    if let Err(e) = stream.set_nonblocking(true) {
        let _ = socket
            .send(Message::Text(format!("shell: {e}").into()))
            .await;
        return;
    }
    let Ok(stream) = tokio::net::UnixStream::from_std(stream) else {
        return;
    };
    let (mut half_r, mut half_w) = stream.into_split();
    loop {
        let mut buf = [0u8; 4096];
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Binary(b))) => {
                        if half_w.write_all(&b).await.is_err() { break; }
                    }
                    Some(Ok(Message::Text(t))) => {
                        if half_w.write_all(t.as_bytes()).await.is_err() { break; }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            n = half_r.read(&mut buf) => {
                match n {
                    Ok(0) => break,
                    Ok(n) => {
                        if socket.send(Message::Binary(buf[..n].to_vec().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    }
                    Err(_) => break,
                }
            }
        }
    }
}
