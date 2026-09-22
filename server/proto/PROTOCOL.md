# Maplayer wire protocol

All traffic rides on iroh QUIC connections under ALPN `maplayer/1`.

## Stream framing

Every bidirectional stream opens with one header line (newline-delimited JSON):

```json
{ "kind": "rpc", "v": 1 }
{ "kind": "acp", "v": 1, "session_id": "<uuid>" }
```

- `kind: "rpc"` — control-plane. The client then writes a single JSON-RPC 2.0
  request line; the server writes a single JSON-RPC 2.0 response line and
  finishes the stream.
- `kind: "acp"` — passthrough. After the header, the stream carries raw
  newline-delimited ACP JSON-RPC between the client and the agent process the
  server spawned. Server → client direction begins with any buffered backlog
  lines, then live output.

## Authorization

The server keeps an allowlist of endpoint IDs (`~/.maplayer/allowlist.json`).
Every connection is checked: if the remote ID is not allowed, the only
reachable method is `maplayer/pair_hello` — and only while a pairing window is
open. Everything else is rejected and the stream is closed.

## Control-plane methods (`maplayer/*`)

All are JSON-RPC 2.0 over an `rpc` stream.

| Method | Params | Result |
| --- | --- | --- |
| `maplayer/pair_hello` | `{ pin: string, label?: string }` | `{ server_id: string, name: string }` |
| `maplayer/sessions` | `{}` | `{ managed: ManagedSession[], external: ExternalSession[] }` |
| `maplayer/session_new` | `{ provider: "codex" \| "cursor", profile?: string, cwd: string, mode?: string }` | `{ session_id: string }` |
| `maplayer/session_kill` | `{ session_id: string }` | `{}` |
| `maplayer/profiles` | `{}` | `{ profiles: Profile[], default: string \| null }` |
| `maplayer/profile_new` | `{ name: string, credential: "chatgpt" \| "api-key" }` | `{ name: string, codex_home: string, login: LoginInstruction }` |
| `maplayer/profile_default` | `{ name: string }` | `{}` |
| `maplayer/ping` | `{}` | `{ server_id: string, version: string }` |

`ManagedSession`: `{ session_id, provider, profile, cwd, state: "running" | "exited", pid, created_at }`
`ExternalSession`: `{ provider, ref, title?, last_active, alive: bool, detail }` — read-only.
`LoginInstruction`: `{ kind: "url" | "device_code" | "api_key_prompt", text: string }` — tells the
client how the user must complete codex login for that profile.

## Session attach

To interact with a managed session the client opens an `acp` stream per
session. ACP `initialize`/`session/new`/`session/prompt` etc. go straight to
the agent's stdio. The server only multiplexes; it does not interpret ACP.
