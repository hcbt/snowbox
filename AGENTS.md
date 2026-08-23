# AGENTS.md

Snowbox runs a coding Agent inside an isolated Nix-built Linux Sandbox on the Host. v1 is a local Nix program (`nix run`), not an `.app` and not a cloud platform.

## Language and decisions

- **[CONTEXT.md](CONTEXT.md)** — glossary. Use those nouns (Host, Sandbox, Workspace, Home, Daemon, Cache, Package, Template, Environment). Read it before naming anything in code or docs.
- **[docs/adr/](docs/adr/)** — irreversible trade-offs. Read the matching ADR before changing isolation, the Cache, the Environment, the Daemon API, the Host OS/hypervisor, or the Daemon language (Rust, [0019](docs/adr/0019-daemon-is-rust.md)).

## How to work here

- Enter the env with `devenv shell -- <cmd>`. Do not use host Python/Node/toolchains.
- The Daemon is Rust. Host glue that is not the Daemon stays **inline in Nix**. Do not add standalone `.sh` files.
- Layout: `flake.nix` is the flake; `devenv.nix` is the Host shell.

## Invariants

- Workspace lives on the Sandbox disk at `/workspace`, not a Host mount.
- Environment is a Host document. The Daemon is the only Cache writer. Guests copy from the Cache; they do not share `/nix/store`.
- Daemon API is `127.0.0.1` plus a token. Loopback is not auth.
- SSH and editor-remote are Unix side effects, not product features.
- Do not put personal tracker identifiers (issue IDs, private board URLs, git branch names generated from those IDs) in this public repo.
