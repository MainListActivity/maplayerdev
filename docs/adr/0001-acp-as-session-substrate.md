# ACP as the session substrate

Managed sessions are spawned by the server as ACP-mode child processes
(`npx @agentclientprotocol/codex-acp`, `agent acp`) and their stdio JSON-RPC
is bridged verbatim over iroh bi-streams. We chose ACP over the alternatives:

- **PTY wrapping** (spawn `codex`/`cursor` TUI in a pty, forward raw bytes):
  gives control of arbitrary terminal sessions, but forces every client to be
  a terminal emulator, loses structured events (permissions, tool calls), and
  still cannot attach to sessions the user already started.
- **Session-file parsing** (read `~/.codex/sessions/*.jsonl`,
  `~/.cursor/chats/*/store.db`): read-only by nature; fine for discovery but
  cannot drive a conversation.

Consequences, recorded deliberately:

- **Managed sessions must be spawned through the server.** A session running
  in the user's own terminal is unreachable by design; those appear only as
  read-only *external sessions* (codex via rollout JSONL tailing, cursor via
  list-level discovery — its ACP mode lacks `session/list`).
- **Interactive control (prompt, permission, cancel) is uniform** across
  providers because it is literally the same protocol forwarded over the wire.
- Adding a provider means finding its ACP entrypoint or writing an adapter —
  not adding a new wire protocol.
- Devin was considered and dropped from v1: it has no local process or ACP
  surface (sessions live in the cloud), so it would need a second substrate
  (REST API) bolted beside ACP rather than under the same model.
