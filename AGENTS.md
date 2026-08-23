# AGENTS.md

Snowbox runs a coding Agent inside an isolated Nix-built Linux Sandbox on the Host. v1 is a local Nix program (`nix run`), not an `.app` and not a cloud platform.

## Language and decisions

- **[CONTEXT.md](CONTEXT.md)** — glossary. Use those nouns (Host, Sandbox, Workspace, Home, Daemon, Cache, Package, Template, Environment, Canvas, Window, Layout). Read it before naming anything in code or docs.
- **[docs/adr/](docs/adr/)** — irreversible trade-offs. Read the matching ADR before changing isolation, the Cache, the Environment, the Daemon API, the Host OS/hypervisor, the Daemon language (Rust, [0019](docs/adr/0019-daemon-is-rust.md)), the macOS VMM ([0022](docs/adr/0022-daemon-embeds-virtualization-framework.md), [0023](docs/adr/0023-start-restores-machine-state.md)), or the GUI ([0020](docs/adr/0020-gui-is-a-canvas-of-windows.md), [0021](docs/adr/0021-ui-is-a-solid-spa.md)).

## How to work here

- Enter the env with `devenv shell -- <cmd>`. Do not use host Python/Node/toolchains. Rust is `languages.rust` in devenv. The Canvas JS toolchain is Bun (`languages.javascript.bun`), not npm.
- Run the stack with `devenv up` (or `devenv shell -- snowbox`). That builds the Canvas, builds the guest runtime if `guest/result` is missing, and starts the Daemon. `flake.nix` is empty until we package for `nix run`.
- The Daemon is Rust. On macOS it embeds Virtualization.framework via `objc2-virtualization`, not vfkit ([0022](docs/adr/0022-daemon-embeds-virtualization-framework.md)). The snowbox binary is ad-hoc signed (`com.apple.security.virtualization`) and the main thread pumps `NSRunLoop`; HTTP runs on a background tokio runtime. Nix store/flake work goes through [nix-bindings-rust](https://github.com/nixops4/nix-bindings-rust), not the `nix` CLI ([0019](docs/adr/0019-daemon-is-rust.md)). Host glue that is not the Daemon stays **inline in Nix**. Do not add standalone `.sh` files.
- The guest runtime is NixOS **26.05** (`guest/`, `nixpkgs` pin `nixos-26.05`). systemd stage 1, `image.repart` via `image.modules`, kernel loaded by the hypervisor (`boot.loader.external`). Do not import `profiles/qemu-guest.nix` (9p/virtiofs), do not use `make-disk-image` / scripted initrd / GRUB. Build with `nix build path:./guest#packages.aarch64-linux.runtime --out-link guest/result` (linux-builder on Darwin). Point the Daemon at it with `SNOWBOX_RUNTIME` or that `guest/result` path.
- Environment flakes live on the Host (`sandboxes/{id}/environment`, default from `environment/empty`). Workspace is not a virtiofs. Cache is `data/snowbox/cache`.
- The bundled UI is a Solid 2 + Vite + Tailwind SPA built with Bun (`canvas/`) and served by the Daemon, a client of the documented API ([0021](docs/adr/0021-ui-is-a-solid-spa.md)). The GUI is a Canvas of Windows ([0020](docs/adr/0020-gui-is-a-canvas-of-windows.md)). `devenv up` builds `canvas/dist`; the Daemon serves that directory.
- Layout: `devenv.nix` is the Host shell. `flake.nix` is future `nix run` packaging, not how you run today. `daemon/` is the Rust Daemon.

## Invariants

- Workspace lives on the Sandbox disk at `/workspace`, not a Host mount.
- Environment is a Host document. The Daemon is the only Cache writer. Guests copy from the Cache; they do not share `/nix/store`.
- Daemon API is `127.0.0.1` plus a token. Loopback is not auth. Contract: [docs/api.md](docs/api.md).
- SSH and editor-remote are Unix side effects, not product features.
- Do not put personal tracker identifiers (issue IDs, private board URLs, git branch names generated from those IDs) in this public repo.
