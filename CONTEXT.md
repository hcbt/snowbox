# Snowbox

A local tool that runs a coding Agent inside a Sandbox on a Host, so the Agent does not use the Host as its computer.

v1 is local-only: one person, one Host. Later is one person, many Hosts they own, one Canvas. A multi-tenant cloud platform is a different later product, not this one. The Daemon has a documented API bound to `127.0.0.1` in v1; other programs on this Host may call it with the token. That is a local integration surface, not the platform.

## Language

**Host**:
A computer that runs a Daemon. Each Host has an id the Daemon creates on first start; the Canvas may show a label. Address is how you reach it this time. The human may use a browser that is not on this computer. A v1 Host is macOS or Linux. macOS is first. Snowbox itself is a Nix program the user runs; not an `.app`, not an installer. The computer showing the Canvas is not a Host unless it also runs a Daemon.
_Avoid_: local machine, laptop, server, the browser's machine, hostname (as identity)

**Sandbox**:
A persistent isolated Nix-built Linux environment on one Host. A Sandbox belongs to exactly one Host. GPU in the Sandbox is out of v1 planning (research later, not a promise). You start, stop, reset, and destroy a Sandbox. Snowbox is the product; a Sandbox is one running or stopped instance. A Sandbox may run zero or more Agents; they share Workspace, Home, and Environment and can conflict. Many Sandboxes may run at once; they do not share filesystem or network with each other. They may copy from the Cache. You work in a Sandbox through its Windows on the Canvas. SSH and editor-remote are possible if the Environment contains the usual Unix packages; they are not Snowbox features. New Sandbox names a Host; Templates, Cache, Limits, and the guest runtime are that Host’s.
_Avoid_: guest, VM, microVM, container, box, machine, environment (for the instance), NixOS (as a button), Ubuntu

**Workspace**:
The project files stored on a Sandbox’s own disk at `/workspace`. Exactly one Workspace per Sandbox. The Sandbox is the source of truth. The Host has no live view of these files. Files enter from inside the Sandbox.
_Avoid_: mount, share, project folder, repo (when you mean the Workspace as a whole), copy-in, copy-out

**Home**:
The Linux user home inside the Sandbox. Stop keeps it on disk. Reset wipes it (logins, extra files, `.gitconfig`). There is no keep-list of paths that survive Reset.
_Avoid_: profile, state, volume, allowlist, $HOME

**Agent**:
A command that runs *inside* a Sandbox and drives coding work. First-class Agents are the home-manager `programs.*` modules the Environment overlay exposes. Snowbox may start zero or one such command; anything else you run in the shell is just a process. Subprocesses run in the Sandbox userspace; no Docker/Podman-in-Sandbox. Starts unprivileged with passwordless sudo. Isolation is Host-vs-Sandbox, not user-vs-root inside the Sandbox.
_Avoid_: bot, assistant, LLM, model (as product nouns)

**Daemon**:
The Host process that owns that Host’s Sandboxes, Cache, Environment, Layout, and token, and serves the Canvas files plus the API. Closing the browser does not stop Sandboxes or forget Layout. Quitting the Daemon writes machine state and stops running guests; disks, Layout, and that state stay. Callers present the token from that Host’s user config. Loopback is not auth. The API listens on all interfaces; callers may be this Host or a Canvas the person Attached from another machine. It is the only writer of the Cache on this Host.
_Avoid_: server, backend, engine, runtime, app (when you mean this process), platform, cloud

**Cache**:
A Snowbox-managed store on that Host of Environment closures already fetched. Separate from the Host user’s Nix store. Each Host has its own Cache. Each Sandbox has its own store and copies from the Cache instead of downloading again. Sandboxes cannot write the Cache. In-guest Nix builds may read it; they do not publish back. v1 warms it when the Daemon realizes an Environment.
_Avoid_: /nix/store, substituter, binary cache, shared store, mount

**Template**:
A named starting Environment: devenv plus a home-manager Agent configuration. It is not a running Sandbox and not a Package list. Snowbox ships a devenv-only default; you author the rest in the Canvas and may save a Sandbox’s Environment as a Template for later New Sandboxes. Saving a Template does not change Sandboxes that already exist. “Flake” is not a GUI noun.
_Avoid_: image, stack, preset, distro, profile, flake, package set

**Environment**:
The Sandbox’s current declared Agent configuration and devenv. It lives on the Host, owned by Snowbox. The Daemon realizes it into the Cache, then activates it in the Sandbox. The Agent cannot persist that declaration. A devenv in `/workspace` is the *project’s*, not the Environment. Reset replaces this with the Environment as it was at Create (not the Template as it exists now) and keeps Workspace.
_Avoid_: image, profile, closure, disk, project flake, package set, hatch

**Reset**:
Put this Sandbox back to Create: the Environment from that moment (not the Template as it exists now), empty Linux home, same Workspace. Extra installs and logins are gone. Destroy is what deletes the Workspace.
_Avoid_: rebuild, reboot, reimage, factory reset, destroy

**Stop**:
Write the Sandbox’s machine state and keep its disk: Workspace, Linux home, Environment, and running processes. Start restores that state onto the same disk. A New Sandbox restores a clone of the first Start’s saved machine state for this guest runtime. If restore is impossible, Start boots.
_Avoid_: pause, freeze, suspend (as GUI nouns)

**Destroy**:
Delete a Sandbox. Workspace is gone. The only verb that deletes the Workspace.
_Avoid_: remove, delete, rm, reset

**Publish**:
Not a Snowbox verb. Guest ports stay closed to the Host.
_Avoid_: expose, forward, ingress, port map

**Secret**:
A credential that may be placed in a Sandbox in v1 if the user chooses (tokens, keys, agent sockets). A Host-side broker that keeps real secrets out of the Sandbox is a later, optional feature — not enforced.
_Avoid_: credential, env var (when you mean the secret itself)

**Limits**:
Per-Sandbox CPU, RAM, and disk caps, set in the UI at create and editable later. Isolation is not only confidentiality; a Sandbox may not eat the Host by default.
_Avoid_: quota, cgroup, resources (when you mean this)

**Canvas**:
The browser UI. One surface: Sandboxes are Windows on it. It may be Attached to several Hosts at once. Agent configuration, Templates, Limits, Attach, and Hosts (Detach lives there) are overlays on that surface, not other home screens. Discovery is not an overlay. Hosts are not objects on the surface. When more than one Host is Attached, a Window names its Host. Icon Manager and the log belong to this Canvas, not to a Host. There is no Package catalog. The Canvas is not a Host. v1 is one Host, opened as that Daemon’s URL on loopback.
_Avoid_: dashboard, lobby, desktop, IDE, workspace (that is files), hatch

**Attach**:
An explicit action that lets this Canvas call that Host’s Daemon. Requires that Host’s token. Does not start Sandboxes. Opening a LAN URL is not Attach. Opening loopback may put this Host on the list (the page may include the token). The roster belongs to this Canvas (this browser, this origin), not to a Host; clearing the origin is Detach-all. The Host that served the page is not special.
_Avoid_: connect, pair, login, link, add server

**Detach**:
Forget that Host in this Canvas. Sandboxes on that Host keep running. Unreachable is still Attached until Detach.
_Avoid_: disconnect, logout, unpair

**Discovery**:
Finding Hosts this Canvas already Attached, including when their address changed. Does not Attach. Does not list strangers.
_Avoid_: mDNS, Bonjour, browse, scan, LAN find

**Window**:
A Snowbox-owned terminal attached to one Sandbox, a free-floating rectangle on the Canvas. A Sandbox may have several. Opening one starts a shell in that Sandbox; closing it ends that shell, not the Sandbox. A control on the Window frame opens that Sandbox’s Environment form. When more than one Host is Attached, a Window names its Host. Not an Agent.
_Avoid_: pane, tab, session, tmux, xterm, PTY (as a GUI noun)

**Layout**:
The Host-side arrangement of that Host’s Windows — which exist, position, size, stacking. The Daemon stores it. The Sandbox does not know about it. A Canvas Attached to several Hosts composites those Layouts. Icon Manager and the log are Canvas chrome, stored with this browser, not in a Host Layout.
_Avoid_: session, workspace, desktop
