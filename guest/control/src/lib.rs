//! Guest-side vsock control plane. The Daemon speaks this protocol.

pub mod agent;

#[cfg(target_os = "linux")]
pub mod shell;
#[cfg(target_os = "linux")]
pub mod vsock;
