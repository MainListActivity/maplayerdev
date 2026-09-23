# maplayer-android

Android client for maplayer: pair with a `maplayer-server` on your dev machine and manage local Codex/Cursor agent sessions (spawn, prompt, answer permissions, cancel, kill) over iroh P2P.

## Stack

- Kotlin + Jetpack Compose (Material 3), navigation-compose
- [`iroh-ffi`](https://github.com/n0-computer/iroh-ffi) Android AAR: `computer.iroh:iroh-android` — per-ABI `libiroh_ffi.so` + `IrohAndroid.installAndroidContext`
- kotlinx-serialization for the maplayer wire protocol (see `server/proto/PROTOCOL.md`)

## Build

```bash
./gradlew assembleDebug      # debug APK
./gradlew assembleRelease    # release APK (unsigned/debug-signed in CI)
```

No gradle wrapper is committed yet; run `gradle wrapper` once or use the CI workflow, which provisions its own.

## Runtime notes

- `MaplayerApp` calls `IrohAndroid.installAndroidContext(this)` before any endpoint work (required for DNS/LinkProperties via JNI).
- The device's iroh `SecretKey` is generated once and stored in SharedPreferences — it IS the client identity the server allowlists.
- Pairing: paste the ticket JSON printed by `maplayer-server pair` (`{"addr":{...},"pin":"123456"}`); the app calls `maplayer/pair_hello` once, then persists the ticket.
- Terminal view shows raw ACP ndjson frames for bring-up; decoded chat UI is a later milestone.
