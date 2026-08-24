//! Window PTY: WebSocket on the Host, vsock into the guest. The browser
//! never talks to the Sandbox. The Daemon owns the shell for each Window
//! so closing the tab does not end it; Kill does.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, mpsc, watch};
use uuid::Uuid;

use crate::api::AppState;
use crate::sandbox::{ActionError, State as SandboxState};
use crate::vmm::{Control, Hypervisor, SHELL_PORT};

const REPLAY_MAX: usize = 512 * 1024;
const FRAME_STDIN: u8 = 0;
const FRAME_WINSIZE: u8 = 1;

#[derive(Clone, Default)]
pub struct Sessions {
    inner: Arc<Mutex<HashMap<Uuid, Arc<Live>>>>,
}

struct Live {
    sandbox: Uuid,
    replay: Mutex<Vec<u8>>,
    out: broadcast::Sender<Vec<u8>>,
    inn: mpsc::Sender<Vec<u8>>,
    stop: watch::Sender<bool>,
}

impl Sessions {
    pub fn drop_window(&self, id: Uuid) {
        if let Some(live) = self.inner.lock().expect("pty").remove(&id) {
            let _ = live.stop.send(true);
        }
    }

    pub fn drop_sandbox(&self, sandbox: Uuid) {
        let mut map = self.inner.lock().expect("pty");
        let ids: Vec<Uuid> = map
            .iter()
            .filter(|(_, l)| l.sandbox == sandbox)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(live) = map.remove(&id) {
                let _ = live.stop.send(true);
            }
        }
    }
}

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
    let sessions = state.sessions.clone();
    Ok(ws.on_upgrade(move |socket| pump(socket, sessions, vmm, id, win.sandbox)))
}

async fn pump(
    mut socket: WebSocket,
    sessions: Sessions,
    vmm: Arc<Hypervisor>,
    window: Uuid,
    sandbox: Uuid,
) {
    let live = match attach(&sessions, vmm, window, sandbox).await {
        Ok(live) => live,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("shell: {e}").into()))
                .await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };
    if *live.stop.borrow() {
        let _ = socket.send(Message::Close(None)).await;
        return;
    }
    let (replay, mut rx) = snapshot(&live);
    if !replay.is_empty() && socket.send(Message::Binary(replay.into())).await.is_err() {
        return;
    }
    let mut dying = live.stop.subscribe();
    loop {
        tokio::select! {
            _ = dying.changed() => break,
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Binary(b))) => {
                        if live.inn.send(frame_stdin(&b)).await.is_err() { break; }
                    }
                    Some(Ok(Message::Text(t))) => {
                        if let Some((rows, cols)) = parse_resize(&t) {
                            if live.inn.send(frame_winsize(rows, cols)).await.is_err() {
                                break;
                            }
                        } else if live.inn.send(frame_stdin(t.as_bytes())).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            chunk = rx.recv() => {
                match chunk {
                    Ok(b) => {
                        if socket.send(Message::Binary(b.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn attach(
    sessions: &Sessions,
    vmm: Arc<Hypervisor>,
    window: Uuid,
    sandbox: Uuid,
) -> Result<Arc<Live>, String> {
    {
        let map = sessions.inner.lock().expect("pty");
        if let Some(live) = map.get(&window) {
            if !*live.stop.borrow() {
                return Ok(live.clone());
            }
        }
    }
    let connected = tokio::task::spawn_blocking(move || Control::vsock(&*vmm, sandbox, SHELL_PORT))
        .await
        .map_err(|_| "vsock join".to_string())?;
    let stream = connected?;
    stream
        .set_nonblocking(true)
        .map_err(|e| format!("shell: {e}"))?;
    let stream = tokio::net::UnixStream::from_std(stream).map_err(|e| format!("shell: {e}"))?;
    let (out, _) = broadcast::channel::<Vec<u8>>(64);
    let (inn_tx, inn_rx) = mpsc::channel::<Vec<u8>>(32);
    let (stop_tx, stop_rx) = watch::channel(false);
    let live = Arc::new(Live {
        sandbox,
        replay: Mutex::new(Vec::new()),
        out: out.clone(),
        inn: inn_tx,
        stop: stop_tx,
    });
    {
        let mut map = sessions.inner.lock().expect("pty");
        if let Some(existing) = map.get(&window) {
            if !*existing.stop.borrow() {
                let _ = live.stop.send(true);
                return Ok(existing.clone());
            }
        }
        map.insert(window, live.clone());
    }
    tokio::spawn(run_guest(stream, inn_rx, out, live.clone(), stop_rx));
    Ok(live)
}

async fn run_guest(
    stream: tokio::net::UnixStream,
    mut inn: mpsc::Receiver<Vec<u8>>,
    out: broadcast::Sender<Vec<u8>>,
    live: Arc<Live>,
    mut stop: watch::Receiver<bool>,
) {
    let (mut half_r, mut half_w) = stream.into_split();
    loop {
        let mut buf = [0u8; 4096];
        tokio::select! {
            _ = stop.changed() => break,
            msg = inn.recv() => {
                let Some(b) = msg else { break };
                if half_w.write_all(&b).await.is_err() { break; }
            }
            n = half_r.read(&mut buf) => {
                match n {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        append_replay(&live.replay, &chunk);
                        let _ = out.send(chunk);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    }
                    Err(_) => break,
                }
            }
        }
    }
    let _ = live.stop.send(true);
}

fn snapshot(live: &Live) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
    let replay = live.replay.lock().expect("pty replay");
    let rx = live.out.subscribe();
    (replay.clone(), rx)
}

/// WebSocket `resize COLS ROWS` → (rows, cols) for the vsock frame.
fn parse_resize(text: &str) -> Option<(u16, u16)> {
    let rest = text.strip_prefix("resize ")?;
    let mut sp = rest.split_whitespace();
    let cols: u16 = sp.next()?.parse().ok()?;
    let rows: u16 = sp.next()?.parse().ok()?;
    (cols > 0 && rows > 0).then_some((rows, cols))
}

fn frame_stdin(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + bytes.len());
    out.push(FRAME_STDIN);
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

fn frame_winsize(rows: u16, cols: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    out.push(FRAME_WINSIZE);
    out.extend_from_slice(&rows.to_le_bytes());
    out.extend_from_slice(&cols.to_le_bytes());
    out
}

fn append_replay(buf: &Mutex<Vec<u8>>, chunk: &[u8]) {
    let mut g = buf.lock().expect("pty replay");
    g.extend_from_slice(chunk);
    if g.len() > REPLAY_MAX {
        let drop = g.len() - REPLAY_MAX;
        g.drain(..drop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_replay_caps_at_max() {
        let buf = Mutex::new(vec![0u8; REPLAY_MAX - 2]);
        append_replay(&buf, &[1, 2, 3, 4]);
        let g = buf.lock().unwrap();
        assert_eq!(g.len(), REPLAY_MAX);
        assert_eq!(&g[g.len() - 4..], &[1, 2, 3, 4]);
    }

    #[test]
    fn drop_sandbox_removes_only_that_sandbox() {
        let sessions = Sessions::default();
        let (out, _) = broadcast::channel(1);
        let (inn, _) = mpsc::channel(1);
        let (stop, _) = watch::channel(false);
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        sessions.inner.lock().unwrap().insert(
            a,
            Arc::new(Live {
                sandbox: Uuid::from_u128(10),
                replay: Mutex::new(Vec::new()),
                out: out.clone(),
                inn: inn.clone(),
                stop: stop.clone(),
            }),
        );
        sessions.inner.lock().unwrap().insert(
            b,
            Arc::new(Live {
                sandbox: Uuid::from_u128(11),
                replay: Mutex::new(Vec::new()),
                out,
                inn,
                stop,
            }),
        );
        sessions.drop_sandbox(Uuid::from_u128(10));
        let map = sessions.inner.lock().unwrap();
        assert!(!map.contains_key(&a));
        assert!(map.contains_key(&b));
    }

    #[test]
    fn parse_resize_cols_then_rows() {
        assert_eq!(parse_resize("resize 80 24"), Some((24, 80)));
        assert_eq!(parse_resize("resize 0 24"), None);
        assert_eq!(parse_resize("hello"), None);
    }

    #[test]
    fn frame_stdin_layout() {
        let f = frame_stdin(b"abc");
        assert_eq!(f[0], FRAME_STDIN);
        assert_eq!(&f[1..5], &3u32.to_le_bytes());
        assert_eq!(&f[5..], b"abc");
    }

    #[test]
    fn frame_winsize_layout() {
        assert_eq!(frame_winsize(24, 80), vec![FRAME_WINSIZE, 24, 0, 80, 0]);
    }
}
