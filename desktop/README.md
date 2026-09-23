# maplayer-desktop

Tauri 2 + Vite/TypeScript desktop client for maplayer. It is also the
server **launcher**: it spawns `maplayer-server` as a child process, scrapes
its stdout for the endpoint id / pairing ticket, and speaks to it over the
same iroh wire protocol as the Android client (see
[`../server/proto/PROTOCOL.md`](../server/proto/PROTOCOL.md)).

## Dev

```sh
npm install
npm run tauri dev        # vite frontend + tauri shell
```

Build the server first from the repo root so the app has a binary to launch:

```sh
cargo build -p maplayer-server
```

## Finding the server binary

The app resolves the `maplayer-server` executable in this order:

1. `MAPLAYER_SERVER_BIN` environment variable (set this for packaged builds
   or non-standard layouts),
2. `../target/debug/maplayer-server` and `../target/release/maplayer-server`
   relative to the working directory,
3. `maplayer-server` on `PATH`.

```sh
MAPLAYER_SERVER_BIN=/path/to/maplayer-server npm run tauri dev
```

In the UI, **Start with pairing** runs `maplayer-server pair` and shows the
ticket to paste into the Android app; **Start** runs `serve` (allowlist
only). The app connects to its own server automatically once the addr is
known — no ticket paste needed on desktop.
