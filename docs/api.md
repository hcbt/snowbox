# Daemon API

Version **1**. Bound to `127.0.0.1`. The bundled UI is a client of this contract. Breaking it is a decision, not a refactor.

Base URL: `http://127.0.0.1:5418/api/v1`

## Auth

Every `/api/v1` request sends the user token:

```
Authorization: Bearer <token>
```

The token is a file in the Host config directory (`snowbox/token` under `dirs::config_dir`, e.g. `~/Library/Application Support/snowbox/token` on macOS). The Daemon creates it on first start with mode `0600`, and tightens an existing file to `0600` if group or other bits are set. Loopback is not authentication. Missing or wrong token → `401` with `{"error":"unauthorized"}`.

## Sandboxes

Each Sandbox has a disk the Daemon owns on the Host (under the data directory, `snowbox/sandboxes/{id}`). That disk holds Workspace (`/workspace`), the Linux home directory, a system tree, and the Environment flake (a Host document, never `/workspace`). Start restores a saved machine state when one exists for that disk. A New Sandbox restores a clone of the first Start’s saved machine state (keyed by guest runtime). Stop writes machine state for that Sandbox; it does not refresh `.ready`. The guest root is a clone of the runtime disk; Workspace and Linux home are synced over vsock, not a Host mount. Quit the Daemon: running guests are saved then stopped; disks, machine state, and Layout stay. Destroy is the only verb that deletes the Workspace.

Many Sandboxes may run at once. They share Host CPU, RAM, and disk capacity, not a filesystem and not a network: each guest has its own disk and its own NAT. The allowed exception is the Cache (Host-side; the Daemon writes, guests copy).

The Cache is a Snowbox store under the data directory (`snowbox/cache`), a `file://` substituter the Daemon writes. Guests copy from it; they do not share `/nix/store` with the Host.

JSON `state` is `stopped` or `running`. `booting` is true while Start is in progress (the Sandbox stays `stopped` until Start finishes). `home` is always `[]` (Reset wipes the Linux home; there is no keep-list). `limits` are per-Sandbox CPU count, RAM bytes, and disk bytes. Defaults: 2 CPUs, 2 GiB RAM, 8 GiB disk. Set at create; PATCH later. CPU and RAM take effect at start. Disk is the guest root image size on the Host; growing applies at start, shrinking needs Reset first.

Copy-in and copy-out run only while `stopped` (`409` `sandbox is running` otherwise). Non-empty destination without `"replace": true` → `409` `replace required`. No merge. A directory source is copied as the contents of `/workspace`; a file lands as `/workspace/{filename}`. `from` and `to` are absolute Host paths (`.`/`..` resolved). copy-out `to` cannot be `/`, `/Users`, `/home`, the home directory root, `/etc`, `/nix/store`, or the Snowbox data directory.

Reset puts the Sandbox back to Create: restores the Environment as it was at Create (not the Template as it exists now), wipes the Linux home, keeps Workspace, drops the writable guest disk so the next start is a fresh root. If the Sandbox is running, Stop (sync Workspace, write machine state) runs first, then Reset.

| Method | Path | Meaning |
| --- | --- | --- |
| `GET` | `/health` | `{"ok":true}` |
| `GET` | `/progress` | Host work log (`{"lines":["…"]}`). New Sandbox, Start, ready-snapshot capture, Environment realize. Last 2000 lines. |
| `GET` | `/sandboxes` | List |
| `POST` | `/sandboxes` | Create. Body `{"name": "...", "limits": {...}, "template": "empty", "environment": {...}}` — all optional. `template` is a Template name. `environment` is optional `config.json` (Customize at Create). `201` |
| `GET` | `/templates` | Shipped and saved Templates. |
| `POST` | `/templates` | Save the current Environment as a Template. Body `{"name":"work","sandbox":"<id>"}`. `201`. Cannot overwrite a shipped Template. Overwrites a user-saved name. |
| `GET` | `/templates/{name}` | That Template’s `config.json`. `404` if missing. |
| `PUT` | `/templates/{name}` | Replace that Template’s `config.json`. Cannot overwrite a shipped Template. |
| `GET` | `/agent-options` | Agent option schema the Environment form renders. Parsed from the repo file `environment/empty/form.json` when the Daemon starts (not a live home-manager eval, not copied into Sandboxes). Bad file → `503` `failed`. |
| `GET` | `/sandboxes/{id}` | One record. `404` if missing |
| `PATCH` | `/sandboxes/{id}` | Update Limits. Body `{"limits":{"cpu":4,"ram":4294967296,"disk":34359738368}}` — any field optional. Applied on the next start. `404` if missing |
| `POST` | `/sandboxes/{id}/start` | `stopped` → `running`. Restores machine state when present, otherwise boots. `409` if already running. No hypervisor → `503` `failed` |
| `POST` | `/sandboxes/{id}/stop` | `running` → `stopped`. Syncs Workspace to the Host, then writes machine state; disk kept. Workspace sync failure → Stop fails and the Sandbox stays running. `409` if already stopped. No hypervisor → `503` `failed` |
| `POST` | `/sandboxes/{id}/reset` | Rewind Environment to Create, wipe Linux home, keep Workspace. Running: halt (Workspace sync + save) first. `404` if missing |
| `POST` | `/sandboxes/{id}/copy-in` | Body `{"from":"/host/path","replace":false}` |
| `POST` | `/sandboxes/{id}/copy-out` | Body `{"to":"/host/path","replace":false}` |
| `GET` | `/sandboxes/{id}/environment` | Current home-manager Agent config (`config.json`). |
| `PUT` | `/sandboxes/{id}/environment` | Replace that config. Updates the Host Environment. If the Sandbox is running, realises into the Cache and activates (no reboot). `extraPackages` entries are nixpkgs attribute names (identifiers). Invalid name → `400` `invalid package name`. |
| `GET` | `/sandboxes/{id}/publish` | Published ports for this Sandbox. Empty while none. |
| `POST` | `/sandboxes/{id}/publish` | Body `{"port":3000,"host_port":null}`. Bind `127.0.0.1` only. Omitted or null `host_port` binds an ephemeral Host port; the response `host_port` is the assigned port. Sandbox must be running. `201` `{port,host_port,url}`. |
| `DELETE` | `/sandboxes/{id}/publish/{port}` | Drop a published port. `204` |
| `DELETE` | `/sandboxes/{id}` | Destroy. Deletes the disk. Allowed in either state. `404` if missing |

Create body:

```json
{
  "name": "work",
  "limits": { "cpu": 2, "ram": 2147483648, "disk": 8589934592 }
}
```

`name` and `limits` (and each limits field) are optional. Omitted limits fields get the defaults.

Sandbox object:

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "work",
  "state": "stopped",
  "booting": false,
  "home": [],
  "limits": { "cpu": 2, "ram": 2147483648, "disk": 8589934592 }
}
```

`cpu` is at least 1. `ram` is at least 512 MiB and a multiple of 1 MiB. `disk` is at least 1 GiB. Invalid limits → `400` `bad_request`.

List is `{"sandboxes":[...]}`.

Errors: `{"error":"<code>"}` with optional `"detail"`. Codes: `unauthorized`, `not_found`, `conflict`, `bad_request`, `internal`, `failed`, `forbidden`.

Same-origin Canvas requests may send the token as cookie `snowbox` instead of `Authorization`. The Daemon sets that cookie only on responses to requests that already presented a valid Bearer token or cookie. Unauthenticated `GET /` does not mint the cookie. The served Canvas HTML includes `window.__SNOWBOX_TOKEN__` so the SPA can send `Authorization: Bearer`.

## Layout and Windows

The Canvas is a client of this API. Layout is Host-side JSON the Daemon persists. Closing the browser does not forget it. A Window is a shell the Daemon owns; PTY bytes travel on a WebSocket to the Daemon, which bridges vsock into the guest.

| Method | Path | Meaning |
| --- | --- | --- |
| `GET` | `/layout` | Windows and Icon Manager. |
| `PUT` | `/layout` | Replace Layout (geometry, iconify, Icon Manager, log Window). |
| `POST` | `/sandboxes/{id}/windows` | Open a Window on that Sandbox. `201` |
| `DELETE` | `/windows/{id}` | Close. Ends that shell. `204` |
| `GET` | `/windows/{id}/pty` | WebSocket. Binary PTY I/O. `409` if the Sandbox is not running. Upgrade requires `Origin` `http://127.0.0.1:<port>` or `http://localhost:<port>` for this Daemon; missing or wrong Origin → `403`. |

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
  "icon_manager": { "x": 8, "y": 8, "w": 200, "h": 240, "visible": true },
  "log": { "x": 240, "y": 72, "w": 560, "h": 280, "visible": false }
}
```
