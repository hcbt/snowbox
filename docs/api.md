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

A Sandbox in this API is a record the Daemon owns. Quit the Daemon and the records are gone (running guests would stop with the process). Disk, copy-in/out, and Reset *implementation* are not this document.

JSON field `state` is `stopped` or `running`.

| Method | Path | Meaning |
| --- | --- | --- |
| `GET` | `/health` | `{"ok":true}` |
| `GET` | `/sandboxes` | List |
| `POST` | `/sandboxes` | Create. Body `{"name": "..."}` — `name` optional. `201` |
| `GET` | `/sandboxes/{id}` | One record. `404` if missing |
| `POST` | `/sandboxes/{id}/start` | `stopped` → `running`. `409` if already running |
| `POST` | `/sandboxes/{id}/stop` | `running` → `stopped`. `409` if already stopped |
| `POST` | `/sandboxes/{id}/reset` | Restore declared Environment (no-op until the Daemon implements it). Same `state`. `404` if missing |
| `DELETE` | `/sandboxes/{id}` | Destroy. Allowed in either state. `404` if missing |

Create body:

```json
{ "name": "work" }
```

Sandbox object:

```json
{ "id": "550e8400-e29b-41d4-a716-446655440000", "name": "work", "state": "stopped" }
```

List is `{"sandboxes":[...]}`.

Errors: `{"error":"<code>"}` with optional `"detail"`. Codes: `unauthorized`, `not_found`, `conflict`, `bad_request`.
