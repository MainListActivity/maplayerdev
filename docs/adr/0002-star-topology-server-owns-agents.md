# Star topology: the server is the only agent host

`server` (host daemon), `desktop` (Tauri), and `android` (Kotlin) form a star:
only the server spawns or touches agents; desktop and android are pure clients
over the same iroh protocol. The desktop app additionally plays the role of
*launcher*: it starts/stops the server binary as a child process and surfaces
its pairing ticket and logs, but it is not a second session host.

Rejected alternatives:

- **Desktop as host too** (agents could run wherever the UI is): doubles the
  lifecycle surface and makes "which machine owns this session" a user-facing
  question we didn't want in v1.
- **Server as pure relay**: pushed the hard problems (auth, credential
  custody, where agents run) back onto each client.

Consequence: Android never needs agent CLIs or credentials; a phone is a
remote control, not a compute node. If a second host ever appears, it runs
another `maplayer-server` and pairs the same way — the model extends without
bending.
