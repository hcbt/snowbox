# The Daemon is Rust

The Host process has to drive vfkit, speak a documented localhost HTTP API, and talk to Nix (realize an Environment, copy into the Cache, copy into a Sandbox). Go is the boring match for vfkit. Python matches more of this author's other apps.

Rust wins because the Nix interaction is core, not incidental, and the crate ecosystem for store/NAR/copy work is where we will spend time. Shipping remains `nix run`. Spike Host drivers can stay inline Nix; the product Daemon does not.

Drive Nix through its CLI and the store (`nix copy`, realize, path-info). Do not embed an evaluator until something the CLI cannot do forces it.
