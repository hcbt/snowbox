# Guest control plane is Rust

`snowbox-agent` and `snowbox-shell` are a guest-side Rust program, built into the runtime image. The Daemon talks vsock to that process. Nix installs and starts it.

They are not `writeShellApplication` in the NixOS module, not `socat EXEC` of a script, and not Host glue. No inline shell glue — not in Nix, not as `.sh` files. [0019](0019-daemon-is-rust.md) is the Host Daemon; this is the matching guest process.

The wire stays the existing verbs (`PING`, `TAR_IN`/`OUT`, `NAR_IN`, `PROFILE`, `RESET`, `STTY`, `CONNECT`; a Window is a PTY login shell). A new language on the socket is a decision, not a refactor.
