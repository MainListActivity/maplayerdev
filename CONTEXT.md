# Maplayer

Manage local coding-agent sessions from any device. One host daemon owns the
agent processes; desktop and Android clients control them over iroh.

## Language

### Topology

**Host**:
The development machine where coding agents execute. A host runs exactly one
server daemon and holds all local state (credentials, session files, config).
_Avoid_: server machine, node, backend

**Server**:
The `maplayer-server` daemon on the host. The single owner of agent lifecycle
and the only component that touches agent credentials and files.
_Avoid_: backend, relay, daemon (when precision matters)

**Client**:
A control endpoint — the Android app or the desktop app. Clients hold no agent
credentials and run no agents.
_Avoid_: frontend, viewer, app

**Endpoint**:
An iroh endpoint — one logical participant with a stable Ed25519 identity.
Every server and every client is an endpoint.
_Avoid_: node, peer

**Endpoint ID**:
The public key that identifies an endpoint. The unit of trust in pairing and
the allowlist.
_Avoid_: node ID (iroh's legacy name), device ID

### Sessions

**Session**:
A conversation with a coding agent. Two disjoint kinds exist; never conflate.
_Avoid_: thread, chat, process

**Managed session**:
A session whose agent process the server spawned (as an ACP child). Full
control: prompt, permissions, cancel, kill. Exists only while the server runs.
_Avoid_: internal session, owned session

**External session**:
A session the user started outside the server (their own terminal). Discovered
read-only — displayed but not controllable.
_Avoid_: unmanaged, orphaned, foreign session

**Provider**:
The integration for one agent family. `codex` and `cursor` are providers;
each maps session operations onto that agent's interface.
_Avoid_: adapter, driver

**ACP**:
Agent Client Protocol — the stdio JSON-RPC interface both supported providers
expose. The substrate every managed session rides on.
_Avoid_: protocol (ambiguous — also the wire protocol)

### Accounts

**Profile**:
One codex account: a name bound to an isolated `CODEX_HOME` directory on the
host. Choosing a profile chooses the account a new session runs under.
_Avoid_: account, login, auth profile (codex's own name for the same idea)

**Default profile**:
The profile applied when a new-session request omits `profile`.

### Connectivity & trust

**Pairing**:
The one-time ceremony that adds a client's endpoint ID to the server's
allowlist. Opened explicitly on the host; the window admits exactly one
client, closing on the first successful `pair_hello` or when the `pair`
daemon exits.
_Avoid_: registration, enrollment

**Pairing window**:
The interval while the server accepts PIN-based `pair_hello` calls from
un-authorized endpoints. One-shot: admits the first client that proves the
PIN, then closes.
_Avoid_: pairing mode

**Launcher token**:
The shared secret in `~/.maplayer/local_token` (0600, same-user only) that
authorizes a same-host client — the desktop app — via `pair_hello` without
consuming the pairing window.
_Avoid_: local secret, loopback auth

**Ticket**:
The JSON payload `{addr, pin}` the host displays during pairing. Carries
everything a client needs to reach and prove itself to the server.
_Avoid_: QR code (a rendering of the ticket), invite

**PIN**:
The single-use secret inside the ticket — expires when the window closes
(first successful pair, or the pairing daemon exits). Proof that the person
pairing can see the host's screen.
_Avoid_: code, token

**Allowlist**:
The persisted set of endpoint IDs authorized to use the full control plane;
`pair_hello` is the only method reachable without being on it. The only
authorization mechanism.
_Avoid_: ACL, authorized keys

**Control plane**:
The `maplayer/*` JSON-RPC surface for listing, spawning, and administering —
distinct from the ACP data plane that carries conversation.
_Avoid_: API, management channel

**ACP stream**:
One iroh bi-stream bound to a managed session, carrying raw ACP frames.
Clients open one per attached session.
_Avoid_: channel, tunnel

**Relay**:
The official n0 relay used only when direct hole-punching fails. Never a
custom or self-hosted relay in v1.
_Avoid_: proxy, TURN server
