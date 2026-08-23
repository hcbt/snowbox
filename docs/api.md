# Daemon API

Version **1**. Bound to `127.0.0.1`. The bundled UI is a client of this contract. Breaking it is a decision, not a refactor.

Base URL: `http://127.0.0.1:5418/api/v1`

## Auth

Every `/api/v1` request sends the user token:

```
Authorization: Bearer <token>
```

The token is a file in the Host config directory (`snowbox/token` under `dirs::config_dir`, e.g. `~/Library/Application Support/snowbox/token` on macOS). The Daemon creates it on first start. Loopback is not authentication. Missing or wrong token → `401` with `{"error":"unauthorized"}`.

## Sandboxes

Each Sandbox has a disk the Daemon owns on the Host (under the data directory, `snowbox/sandboxes/{id}`). That disk holds Workspace (`/workspace`), Home (allowlist), a system tree, and the Environment flake (a Host document, never `/workspace`). Start restores a saved machine state when one exists for that disk. A New Sandbox restores a pre-booted snapshot of the guest runtime (the Daemon boots that snapshot once if it is missing). Stop writes machine state next to the disk and refreshes the snapshot. The guest root is a clone of the runtime disk; Workspace and Home are synced over vsock, not a Host mount. Quit the Daemon: running guests are saved then stopped; disks, machine state, and Layout stay. Destroy is the only verb that deletes the Workspace.

Many Sandboxes may run at once. They share Host CPU, RAM, and disk capacity, not a filesystem and not a network: each guest has its own disk and its own NAT. The allowed exception is the Cache (Host-side; the Daemon writes, guests copy).

The Cache is a Snowbox store under the data directory (`snowbox/cache`), a `file://` substituter the Daemon writes. Guests copy from it; they do not share `/nix/store` with the Host.

JSON `state` is `stopped` or `running`. `home` is the allowlist of paths under the guest home that survive Reset. v1 default: `.gitconfig`. `limits` are per-Sandbox CPU count, RAM bytes, and disk bytes. Defaults: 2 CPUs, 2 GiB RAM, 16 GiB disk. Set at create; PATCH later. CPU and RAM take effect at start. Disk is the guest root image size on the Host; growing applies at start, shrinking needs Reset first.

Copy-in and copy-out run only while `stopped` (`409` `sandbox is running` otherwise). Non-empty destination without `"replace": true` → `409` `replace required`. No merge. A directory source is copied as the contents of `/workspace`; a file lands as `/workspace/{filename}`.

Reset restores the system (drops the writable guest disk so the next start is a fresh root), keeps Workspace, keeps the Environment flake, and keeps only allowlisted Home paths.

| Method | Path | Meaning |
| --- | --- | --- |
| `GET` | `/health` | `{"ok":true}` |
| `GET` | `/sandboxes` | List |
| `POST` | `/sandboxes` | Create. Body `{"name": "...", "limits": {...}, "template": "empty"}` — all optional. `template` is a Template name. `201` |
| `GET` | `/templates` | Shipped and saved Templates. |
| `POST` | `/templates` | Save the current Environment as a Template. Body `{"name":"work","sandbox":"<id>"}`. `201`. Cannot overwrite a shipped Template. |
| `GET` | `/agent-options` | home-manager Agent option schema the hatch renders. |
| `GET` | `/sandboxes/{id}` | One record. `404` if missing |
| `PATCH` | `/sandboxes/{id}` | Update Limits. Body `{"limits":{"cpu":4,"ram":4294967296,"disk":34359738368}}` — any field optional. Applied on the next start. `404` if missing |
| `POST` | `/sandboxes/{id}/start` | `stopped` → `running`. Restores machine state when present, otherwise boots. `409` if already running |
| `POST` | `/sandboxes/{id}/stop` | `running` → `stopped`. Writes machine state; disk kept. `409` if already stopped |
| `POST` | `/sandboxes/{id}/reset` | Keep Workspace + Home allowlist; restore system. `404` if missing |
| `POST` | `/sandboxes/{id}/copy-in` | Body `{"from":"/host/path","replace":false}` |
| `POST` | `/sandboxes/{id}/copy-out` | Body `{"to":"/host/path","replace":false}` |
| `GET` | `/sandboxes/{id}/environment` | Current home-manager Agent config (`config.json`). |
| `PUT` | `/sandboxes/{id}/environment` | Replace that config. Updates the Host Environment. If the Sandbox is running, realises into the Cache and activates (no reboot). |
| `GET` | `/sandboxes/{id}/publish` | Published ports for this Sandbox. Empty while none. |
| `POST` | `/sandboxes/{id}/publish` | Body `{"port":3000,"host_port":null}`. Bind `127.0.0.1` only. Sandbox must be running. `201` `{port,host_port,url}`. |
| `DELETE` | `/sandboxes/{id}/publish/{port}` | Drop a published port. `204` |
| `DELETE` | `/sandboxes/{id}` | Destroy. Deletes the disk. Allowed in either state. `404` if missing |

Create body:

```json
{
  "name": "work",
  "limits": { "cpu": 2, "ram": 2147483648, "disk": 17179869184 }
}
```

`name` and `limits` (and each limits field) are optional. Omitted limits fields get the defaults.

Sandbox object:

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "work",
  "state": "stopped",
  "home": [".gitconfig"],
  "limits": { "cpu": 2, "ram": 2147483648, "disk": 17179869184 }
}
```

`cpu` is at least 1. `ram` is at least 512 MiB and a multiple of 1 MiB. `disk` is at least 1 GiB. Invalid limits → `400` `bad_request`.

List is `{"sandboxes":[...]}`.

Errors: `{"error":"<code>"}` with optional `"detail"`. Codes: `unauthorized`, `not_found`, `conflict`, `bad_request`, `internal`.

Same-origin Canvas requests may send the token as cookie `snowbox` instead of `Authorization`. The Daemon sets that cookie on every response.

## Layout and Windows

The Canvas is a client of this API. Layout is Host-side JSON the Daemon persists. Closing the browser does not forget it. A Window is a shell the Daemon owns; PTY bytes travel on a WebSocket to the Daemon, which bridges vsock into the guest.

| Method | Path | Meaning |
| --- | --- | --- |
| `GET` | `/layout` | Windows and Icon Manager. |
| `PUT` | `/layout` | Replace Layout (geometry, iconify, Icon Manager). |
| `POST` | `/sandboxes/{id}/windows` | Open a Window on that Sandbox. `201` |
| `DELETE` | `/windows/{id}` | Close. Ends that shell. `204` |
| `GET` | `/windows/{id}/pty` | WebSocket. Binary PTY I/O. `409` if the Sandbox is not running. |

Layout object:

```json
{
  "windows": [
    {
      "id": "…",
      "sandbox": "…",
      "title": "work — xterm",
      "x": 40,
      "y": 40,
      "w": 640,
      "h": 400,
      "z": 1,
      "iconified": false
    }
  ],
  "icon_manager": { "x": 8, "y": 8, "visible": true }
}
```
