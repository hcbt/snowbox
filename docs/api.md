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

Each Sandbox has a disk the Daemon owns on the Host (under the data directory, `snowbox/sandboxes/{id}`). That disk holds Workspace (`/workspace`), Home (allowlist), and a system tree (the Environment). The Host has no live view: do not treat those paths as the project. Quit the Daemon: running guests stop; disks stay. Destroy is the only verb that deletes the Workspace.

JSON `state` is `stopped` or `running`. `home` is the allowlist of paths under the guest home that survive Reset. v1 default: `.gitconfig`.

Copy-in and copy-out run only while `stopped` (`409` `sandbox is running` otherwise). Non-empty destination without `"replace": true` → `409` `replace required`. No merge. A directory source is copied as the contents of `/workspace`; a file lands as `/workspace/{filename}`.

Reset restores the system tree (drops undeclared tools), keeps Workspace, and keeps only allowlisted Home paths. Environment *realization* into that system tree is a later concern; Reset still wipes undeclared files there.

| Method | Path | Meaning |
| --- | --- | --- |
| `GET` | `/health` | `{"ok":true}` |
| `GET` | `/sandboxes` | List |
| `POST` | `/sandboxes` | Create. Body `{"name": "..."}` — `name` optional. `201` |
| `GET` | `/sandboxes/{id}` | One record. `404` if missing |
| `POST` | `/sandboxes/{id}/start` | `stopped` → `running`. `409` if already running |
| `POST` | `/sandboxes/{id}/stop` | `running` → `stopped`. Disk kept. `409` if already stopped |
| `POST` | `/sandboxes/{id}/reset` | Keep Workspace + Home allowlist; restore system. `404` if missing |
| `POST` | `/sandboxes/{id}/copy-in` | Body `{"from":"/host/path","replace":false}` |
| `POST` | `/sandboxes/{id}/copy-out` | Body `{"to":"/host/path","replace":false}` |
| `DELETE` | `/sandboxes/{id}` | Destroy. Deletes the disk. Allowed in either state. `404` if missing |

Create body:

```json
{ "name": "work" }
```

Sandbox object:

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "work",
  "state": "stopped",
  "home": [".gitconfig"]
}
```

List is `{"sandboxes":[...]}`.

Errors: `{"error":"<code>"}` with optional `"detail"`. Codes: `unauthorized`, `not_found`, `conflict`, `bad_request`, `internal`.
