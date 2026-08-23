# Snowbox

A local tool that runs a coding Agent inside a Sandbox on your Host, so the Agent does not use the Host as its computer.

v1 is local-only: one person, one Host. A multi-tenant cloud platform is a later product, not this one. The Daemon has a documented API bound to `127.0.0.1`; other programs on this Host may call it with the token. That is a local integration surface, not the platform.

## Language

**Host**:
The computer Snowbox is installed on. The human uses the Host; the Agent does not. A v1 Host is macOS or Linux. macOS is first. Snowbox itself is a Nix program the user runs; not an `.app`, not an installer.
_Avoid_: local machine, laptop, server

**Sandbox**:
A persistent isolated Nix-built Linux environment on the Host. GPU in the Sandbox is out of v1 planning (research later, not a promise). You start, stop, reset, and destroy a Sandbox. Snowbox is the product; a Sandbox is one running or stopped instance. A Sandbox may run zero or more Agents; they share Workspace, Home, and Environment and can conflict. Many Sandboxes may run at once; they do not share filesystem or network with each other. They may copy Packages from the Cache. You work in a Sandbox through its Windows on the Canvas. SSH and editor-remote are possible if the Environment contains the usual Unix packages; they are not Snowbox features. A Sandbox port is closed to the Host until explicitly Published.
_Avoid_: guest, VM, microVM, container, box, machine, environment (for the instance), NixOS (as a button), Ubuntu

**Workspace**:
The project files stored on a Sandbox’s own disk at `/workspace`. Exactly one Workspace per Sandbox. The Sandbox is the source of truth. The Host has no live view of these files. Copy-in and copy-out are explicit Host actions, never a background sync. Copy-in or copy-out into a non-empty destination is refused unless the user confirms replace (no merge).
_Avoid_: mount, share, project folder, repo (when you mean the Workspace as a whole)

**Home**:
An allowlist of paths that survive Reset. v1: `.gitconfig` and Secrets Snowbox placed. No vendor Agent auth dirs are pre-seeded; a path is added when we actually learn it. Not the Unix home directory. `~/.local`, `~/.npm`, `~/.cargo`, `~/.config` as a blob, and other install prefixes are not Home.
_Avoid_: profile, state, volume, $HOME

**Agent**:
A command that runs *inside* a Sandbox and drives coding work. Not a named vendor list in v1 — it is whatever command is in the Environment. Snowbox may start zero or one such command; anything else you run in the shell is just a process. Subprocesses run in the Sandbox userspace; no Docker/Podman-in-Sandbox. Starts unprivileged with passwordless sudo. Isolation is Host-vs-Sandbox, not user-vs-root inside the Sandbox.
_Avoid_: bot, assistant, LLM, model, Claude, Grok, Codex (as product nouns)

**Daemon**:
The Host process that owns Sandboxes and serves the Canvas. Closing the browser does not stop Sandboxes or forget Layout. Quitting the Daemon writes machine state and stops running guests; disks, Layout, and that state stay. Starting the Daemon again restores the guests that were running. It exposes a documented API on `127.0.0.1`. Callers need a token stored in the user’s config; loopback is not auth. Remote machines are not callers. It is the only writer of the Cache.
_Avoid_: server, backend, engine, runtime, app (when you mean this process), platform, cloud

**Cache**:
A Snowbox-managed store on the Host of Packages already fetched. Separate from the Host user’s Nix store. Each Sandbox has its own store and copies from the Cache instead of downloading again. Sandboxes cannot write the Cache. In-guest Nix builds may read it; they do not publish back. v1 warms it when the Daemon realizes an Environment.
_Avoid_: /nix/store, substituter, binary cache, shared store, mount

**Package**:
An installable tool or library that can belong to an Environment. The v1 catalog is nixpkgs; the UI searches by program name and description, not by Nix attribute. Unfree packages are off until the user turns them on. Adding a Package updates the Environment and the running Sandbox immediately. Realization fetches into the Cache, then copies into the Sandbox.
_Avoid_: derivation, formula, plugin, app

**Template**:
A named starting Environment. Picking a Template fills the Package set. It is not a running Sandbox. Snowbox ships some; the user can also author and edit them (the hatch). “Flake” is not a GUI noun.
_Avoid_: image, stack, preset, distro, profile, flake

**Environment**:
The Sandbox’s current declared Package set: a Template plus Packages added later. It lives on the Host, owned by Snowbox. The Daemon realizes it into the Cache, then copies it into the Sandbox. The Agent cannot persist a Package add. A flake in `/workspace` is the *project’s*, not the Environment. Reset restores this declaration (the system), not the original Template, and keeps Workspace and Home.
_Avoid_: image, profile, closure, disk, project flake

**Reset**:
The operation that makes the Environment true again. Declared Packages are realized; undeclared tools are gone; Workspace and Home remain.
_Avoid_: rebuild, reboot, reimage, factory reset

**Stop**:
Write the Sandbox’s machine state and keep its disk: Workspace, Home, Environment, and running processes. Start restores that state onto the same disk. A new Sandbox still boots. If restore is impossible, Start boots.
_Avoid_: pause, freeze, suspend (as GUI nouns)

**Destroy**:
Delete a Sandbox. Workspace is gone unless copy-out already happened. The only verb that deletes the Workspace.
_Avoid_: remove, delete, rm, reset

**Publish**:
An explicit user action that maps a Sandbox port onto `127.0.0.1` on the Host so a Host browser can open a server the Agent started. Default is not published. Not bound on the LAN.
_Avoid_: expose, forward, ingress, port map

**Secret**:
A credential that may be placed in a Sandbox in v1 if the user chooses (tokens, keys, agent sockets). A Host-side broker that keeps real secrets out of the Sandbox is a later, optional feature — not enforced.
_Avoid_: credential, env var (when you mean the secret itself)

**Limits**:
Per-Sandbox CPU, RAM, and disk caps, set in the UI at create and editable later. Isolation is not only confidentiality; a Sandbox may not eat the Host by default.
_Avoid_: quota, cgroup, resources (when you mean this)

**Canvas**:
The Host browser UI. One surface: Sandboxes are Windows on it. Package search, Templates, Limits, copy-in/out, and Publish are overlays on that surface, not other home screens.
_Avoid_: dashboard, lobby, desktop, IDE, workspace (that is files)

**Window**:
A Snowbox-owned terminal attached to one Sandbox, a free-floating rectangle on the Canvas. A Sandbox may have several. Opening one starts a shell in that Sandbox; closing it ends that shell, not the Sandbox. Not an Agent.
_Avoid_: pane, tab, session, tmux, xterm, PTY (as a GUI noun)

**Layout**:
The Host-side arrangement of Windows — which exist, position, size, stacking. The Daemon stores it. The Sandbox does not know about it.
_Avoid_: session, workspace, desktop
