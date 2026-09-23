# maplayer

Manage local Codex / Cursor agent sessions from your phone. A single daemon
on your dev machine owns the agent processes; Android and desktop apps are
remote controls that talk to it over [iroh](https://iroh.computer) (QUIC,
ALPN `maplayer/1`) — direct hole-punched connections where possible, the
official n0 relay as fallback.

The server spawns agents as [ACP](https://agentclientprotocol.com) child
processes and bridges their stdio JSON-RPC verbatim onto the wire, so
prompting, permissions, and cancellation work the same on every client.

## Topology

```
                    maplayer-server  (one per host — owns agents,
                         │            credentials, allowlist, profiles)
                         │ iroh QUIC, ALPN maplayer/1
            ┌────────────┴────────────┐
            │                         │
     maplayer-desktop          maplayer-android
      (Tauri client +             (Kotlin/Compose
       server launcher)            remote control)
```

Star topology: only the server touches agent CLIs and credentials. The
desktop app doubles as a launcher — it starts/stops `maplayer-server` as a
child process and displays the pairing ticket — but runs no agents of its
own.

## Components

| Component | What it is | Build & run |
| --- | --- | --- |
| `server/` | Rust host daemon (`maplayer-server`): spawns/owns agent sessions, serves the control plane and ACP passthrough over iroh. | `cargo build -p maplayer-server` (or `--release`), then `maplayer-server pair` / `serve` / `id` / `profiles` / `sessions` / `allow` |
| `desktop/` | Tauri 2 + Vite/TS desktop client; also launches the server binary and shows its pairing ticket. | `cd desktop && npm install && npm run tauri dev` — see `desktop/README.md` |
| `android/` | Kotlin + Jetpack Compose Android client using `computer.iroh:iroh-android` bindings. | Gradle/AGP 8.7.3, compileSdk 35 — open `android/` in Android Studio or `gradle :app:assembleDebug` |

The host needs the agent CLIs it will spawn: `npx` (for
`@agentclientprotocol/codex-acp`) and the Cursor `agent` CLI, plus `codex`
itself for profile logins.

## Quickstart

1. Build and start the server on the host with a pairing window open:

   ```sh
   cargo build -p maplayer-server
   ./target/debug/maplayer-server pair
   ```

   It prints the endpoint id, a 6-digit PIN, and a pairing ticket
   (`{addr, pin}` JSON — paste it, or render it as a QR code).

2. In the Android app, paste the ticket. The app calls
   `maplayer/pair_hello` with the PIN; the server records the app's
   endpoint ID in `~/.maplayer/allowlist.json` and the phone is paired
   permanently (the PIN dies with the `pair` process).

3. From then on the phone can list sessions, spawn managed sessions
   (`maplayer/session_new` with provider `codex` or `cursor`), attach to
   them over `acp` streams, and manage codex profiles — while the server
   keeps owning the agent processes and credentials.

   To run the daemon without pairing later: `maplayer-server serve`. To
   trust an endpoint without pairing: `maplayer-server allow <endpoint-id>`.

## Docs

- [`server/proto/PROTOCOL.md`](server/proto/PROTOCOL.md) — the wire
  contract: stream framing, control-plane methods, error codes, pairing
  ticket shape.
- [`CONTEXT.md`](CONTEXT.md) — domain vocabulary (host/server/client,
  managed vs external sessions, profiles, pairing).
- [`docs/adr/`](docs/adr/) — why ACP, why a star topology, why
  `CODEX_HOME` profiles, and the pairing/relay boundary.

## Limitations

- **External sessions are read-only.** Sessions you started in your own
  terminal are discovered (codex rollout `.jsonl`, cursor `store.db`) and
  listed, but cannot be prompted or controlled.
- **No Windows support yet.** Host targets Linux/macOS.
- **Relay fallback is the official n0 relay only** — no custom or
  self-hosted relays in v1.
- Managed sessions live in daemon memory; they do not survive a server
  restart.

## License

MIT — see [LICENSE](LICENSE).
