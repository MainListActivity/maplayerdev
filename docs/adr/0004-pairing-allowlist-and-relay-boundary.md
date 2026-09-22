# Pairing, allowlist, and the relay boundary

This system exposes an agent command channel — authorization had to exist
from day one rather than riding on iroh's transport encryption alone.

Model: the server keeps a persistent **allowlist** of client endpoint IDs.
Every incoming connection is checked; an un-authorized endpoint can reach
only `maplayer/pair_hello`, and only while a **pairing window** is open.
Pairing is initiated on the host (`maplayer-server pair`, or the desktop's
"start with pairing" button), which mints a short-lived PIN and displays a
ticket `{addr, pin}` — the client presents the PIN, the host records its
endpoint ID, the window closes.

Why this shape: endpoint IDs are the identity iroh already authenticates at
the QUIC layer, so an allowlist of them is authorization with zero extra
crypto. The PIN proves only "you can see the host's screen right now",
keeping pairing ceremony-free without trusting LAN membership or scanning
certificates.

Network boundary, per requirement: connections prefer direct hole-punched
paths; the only fallback relays are the official n0 relays (`presets::N0`
defaults). No custom or self-hosted relay in v1 — less surface to run and to
secure. Relay remains a transport fallback only; authorization is unchanged
whichever path a connection arrives on.
