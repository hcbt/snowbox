# The Daemon is Rust

The Host process has to embed Virtualization.framework on macOS ([0022](0022-daemon-embeds-virtualization-framework.md)), speak a documented localhost HTTP API, and talk to Nix (realize an Environment, copy into the Cache, copy into a Sandbox). Go would match a vfkit child process; we are not doing that.

Rust wins because the Nix interaction is core, not incidental, and the crate ecosystem for store/NAR/copy work is where we will spend time. Shipping remains `nix run`. The product Daemon is Rust, not inline Nix.

Talk to Nix in-process via [nix-bindings-rust](https://github.com/nixops4/nix-bindings-rust) (the Nix C API: store, flakes, eval). Realize, copy, and path-info go through `nix-bindings-store` / `nix-bindings-flake`, not `std::process::Command` of the `nix` CLI. That couples us to a Nix version and their flake module; that is the cost of not parsing CLI output. The Daemon still uses the same Nix the flake pins.

[NotAShelf/nix-bindings](https://github.com/NotAShelf/nix-bindings) wraps the same C API and already has `copy_closure` / `copy_path`. Stay on nixops4 until a concrete gap (missing copy, or the nci tax) forces a switch. Do not dual-track both crates.
