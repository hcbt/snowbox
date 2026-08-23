//! Guest-side vsock control plane. The Daemon speaks this protocol.

use std::path::PathBuf;

pub mod agent;

/// Login shell. NixOS has no `/bin/bash`; systemd/wrap sets `SNOWBOX_BASH`.
pub fn bash_path() -> PathBuf {
    std::env::var_os("SNOWBOX_BASH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bash"))
}

#[cfg(target_os = "linux")]
pub mod shell;
#[cfg(target_os = "linux")]
pub mod vsock;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_path_defaults_to_bash_on_path() {
        let prev = std::env::var_os("SNOWBOX_BASH");
        unsafe { std::env::remove_var("SNOWBOX_BASH") };
        assert_eq!(bash_path(), PathBuf::from("bash"));
        unsafe { std::env::set_var("SNOWBOX_BASH", "/nix/store/fake/bin/bash") };
        assert_eq!(bash_path(), PathBuf::from("/nix/store/fake/bin/bash"));
        match prev {
            Some(v) => unsafe { std::env::set_var("SNOWBOX_BASH", v) },
            None => unsafe { std::env::remove_var("SNOWBOX_BASH") },
        }
    }
}
