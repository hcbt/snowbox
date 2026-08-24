//! Guest-side vsock control plane. The Daemon speaks this protocol.

pub mod agent;
pub mod frame;
pub mod shell;

#[cfg(target_os = "linux")]
pub mod vsock;
