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

The header line and the `rpc` request line are each capped at 1 MiB; the
server drops streams whose lines exceed that. Clients are expected to apply
the same cap when reading the `rpc` response. ACP payload lines on an `acp`
stream are not length-checked by the server — they pass through verbatim.

## Authorization

The server keeps an allowlist of endpoint IDs (`~/.maplayer/allowlist.json`,
shaped `{ "endpoints": { "<id>": "<label?>" } }`).

- Remote endpoint **not allowlisted**: the connection is accepted but every
  `rpc` method except `maplayer/pair_hello` is rejected with error `-32001`,
  and an `acp` stream fails immediately. pair_hello only succeeds with a
  valid pairing PIN (while a window is open) or the local launcher token.
- Remote endpoint **allowlisted**: the full control-plane surface is
  reachable, plus `acp` session attach. Authorization is re-checked per
  stream, so a client that pairs mid-connection is authorized from its next
  stream on.

**Local launcher token**: the server writes `~/.maplayer/local_token` (0600)
when it binds. A same-host client that presents that token as the `pin` in
`pair_hello` is added to the allowlist *without* consuming the pairing
window — the desktop app uses this to manage its own daemon while leaving
the one-shot window free for a phone.

## Errors

RPC responses carry the usual `{"jsonrpc": "2.0", "id", "error": {code, message}}`
envelope. Three codes are in use:

| Code | Meaning |
| --- | --- |
| `-32001` | Unauthorized: a non-allowlisted endpoint called a method other than `maplayer/pair_hello`. |
| `-32000` | Any method-level failure. The message is the server-side error text — e.g. `pairing closed or bad pin`, `unknown provider`, `unknown profile`, `session not found`. |
| `-32601` | Method not found (the JSON-RPC-standard code). |

## Control-plane methods (`maplayer/*`)

All are JSON-RPC 2.0 over an `rpc` stream.

| Method | Params | Result |
| --- | --- | --- |
| `maplayer/pair_hello` | `{ pin: string, label?: string }` | `{ server_id: string, name: string }` |
| `maplayer/sessions` | `{}` | `{ managed: ManagedSession[], external: ExternalSession[] }` |
| `maplayer/session_new` | `{ provider: "codex" \| "cursor", profile?: string, cwd: string }` | `{ session_id: string }` |
| `maplayer/session_kill` | `{ session_id: string }` | `{}` |
| `maplayer/session_tail` | `{ reference: string, lines?: number }` | `{ lines: string[], offset: number }` |
| `maplayer/profiles` | `{}` | `{ profiles: Profile[], default: string \| null }` |
| `maplayer/profile_new` | `{ name: string, credential: "chatgpt" \| "api-key" }` | `{ name: string, codex_home: string, login: LoginInstruction }` |
| `maplayer/profile_default` | `{ name: string }` | `{}` |
| `maplayer/ping` | `{}` | `{ server_id: string, version: string }` |

Method notes:

- `pair_hello`: succeeds with the pairing `pin` while a window is open
  (one success closes the window), or with the local launcher token at any
  time; on success the caller's endpoint ID is added to the allowlist (with
  `label` recorded when given).
- `session_tail`: `reference` must be an external Codex rollout file
  (`rollout-*.jsonl` under `~/.codex/sessions`); anything else is refused.
  Returns the last `lines` (default 50, max 500) plus the byte offset the
  tail starts at.
- `session_new`: `provider` is one of `codex` or `cursor`; anything else is
  rejected. When `profile` is omitted the server's default profile is used,
  falling back to the agent's own `CODEX_HOME` (typically `~/.codex`).
- `profile_new`: `name` must be 1–64 chars of `[a-zA-Z0-9_-]`.

Types:

`ManagedSession`: `{ session_id: string, provider: string, profile: string | null, cwd: string, state: "running" | "exited", pid: number | null, created_at: string }` —
`created_at` is the session's spawn time as unix seconds (string form).

`ExternalSession`: `{ provider: string, ref: string, title: string | null, last_active: string | null, alive: bool, detail: string }` —
read-only; `ref` is a host-local path to the discovered session artifact
(codex rollout `.jsonl` or cursor `store.db`), `last_active` is its mtime as
unix seconds (string form), `alive` reflects whether a matching agent
process was seen on the host.

`Profile`: `{ name: string, credential: "chatgpt" | "api-key", codex_home: string }`.

`LoginInstruction`: `{ kind: "url" | "api_key_prompt", text: string }` — tells the
client how the user must complete codex login for that profile; login always
happens on the host, never over the wire.

## Session attach

To interact with a managed session the client opens an `acp` stream per
session, carrying the `session_id` in the stream header. An unknown
`session_id` fails the stream. On attach the server first replays the
session's buffered backlog (capped at the most recent ~4000 lines; a
slow reader can also miss live lines and jumps forward), then forwards live
agent output. ACP `initialize`/`session/new`/`session/prompt` etc. from the
client go straight to the agent's stdio. The server only multiplexes; it
does not interpret ACP.

## Pairing ticket

`maplayer-server pair` prints the ticket a client must present — it is
out-of-band input (paste / QR payload), not an RPC message:

```json
{ "addr": { "id": "<endpoint id>", "addrs": [ { "Relay": "<url>" }, { "Ip": "<sockaddr>" }, ... ] }, "pin": "<6-digit>" }
```

`addr` is the server's iroh `EndpointAddr`; `pin` is the secret the client
sends in `pair_hello`. The pairing window admits **exactly one client**: it
closes on the first successful PIN pair, or when that `pair` process exits.
