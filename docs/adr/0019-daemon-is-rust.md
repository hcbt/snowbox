# The Daemon is Rust

The Host process has to embed Virtualization.framework on macOS ([0022](0022-daemon-embeds-virtualization-framework.md)), speak a documented localhost HTTP API, and talk to Nix (realize an Environment, copy into the Cache, copy into a Sandbox). Go would match a vfkit child process; we are not doing that.

Rust wins because the Nix interaction is core, not incidental, and the crate ecosystem for store/NAR/copy work is where we will spend time. Shipping remains `nix run`. The product Daemon is Rust, not inline Nix.

Talk to Nix through the C API ([nix-bindings-rust](https://github.com/nixops4/nix-bindings-rust)): store, flakes, eval. Realize, copy, and path-info go through those bindings, not by parsing `nix eval --json` or other CLI output. That couples us to a Nix version and their flake module; that is the cost of not parsing CLI output.

The C API cannot live in the same Mach task as Virtualization.framework. Nix links Boehm GC; `GC_stop_world` / `thread_suspend` on VF and GCD threads aborts the Daemon (SIGABRT 134). Registering threads with `gc_register_my_thread` does not fix this.

So the C API runs in a posix_spawned helper, `snowbox-eval`. `std::process::Command` on macOS is posix_spawn; never `fork` after VF or Boehm. The helper links nix-bindings, evaluates, realises, and writes NAR to a file. Protocol: one JSON line on stdin, one JSON line on stdout; NAR bytes are a path in that JSON, not a pipe blob. The `snowbox` binary that embeds `VZVirtualMachine` must not link libgc or nix-bindings. It locates the helper next to `current_exe()` as `snowbox-eval`, or via `SNOWBOX_EVAL`. `cargo build -p snowbox-eval` is required before `cargo run -p snowbox`.

Guest Environments are Linux. The helper prefers `packages.aarch64-linux` from the flake and defaults to that on a Darwin Host (linux-builder).

[NotAShelf/nix-bindings](https://github.com/NotAShelf/nix-bindings) wraps the same C API and already has `copy_closure` / `copy_path`. Stay on nixops4 until a concrete gap (missing copy, or the nci tax) forces a switch. Do not dual-track both crates.
