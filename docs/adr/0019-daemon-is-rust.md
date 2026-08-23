# The Daemon is Rust

The Host process has to drive vfkit, speak a documented localhost HTTP API, and talk to Nix (realize an Environment, copy into the Cache, copy into a Sandbox). Go is the boring match for vfkit. Python matches more of this author's other apps.

Rust wins because the Nix interaction is core, not incidental, and the crate ecosystem for store/NAR/copy work is where we will spend time. Shipping remains `nix run`. The product Daemon is Rust, not inline Nix.

Talk to Nix in-process via [nix-bindings-rust](https://github.com/nixops4/nix-bindings-rust) (the Nix C API: store, flakes, eval). Realize, copy, and path-info go through `nix-bindings-store` / `nix-bindings-flake`, not `std::process::Command` of the `nix` CLI. That couples us to a Nix version and their flake module; that is the cost of not parsing CLI output. The Daemon still uses the same Nix the flake pins.
