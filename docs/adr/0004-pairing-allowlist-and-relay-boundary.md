# Pairing, allowlist, and the relay boundary

This system exposes an agent command channel — authorization had to exist
from day one rather than riding on iroh's transport encryption alone.

Model: the server keeps a persistent **allowlist** of client endpoint IDs.
Authorization is checked per stream: an un-authorized endpoint can reach
only `maplayer/pair_hello`, and only with a valid credential. Pairing is
initiated on the host (`maplayer-server pair`, or the desktop's
"start with pairing" button), which mints a PIN and displays a ticket
`{addr, pin}` — the client presents the PIN, the host records its endpoint
ID. **The window admits exactly one client**: it closes on the first
successful PIN pair, or when that `pair` daemon exits.

The desktop launcher is a client too, and self-pairing through the PIN
would race the phone for the single window. The server therefore also
accepts `~/.maplayer/local_token` (0600, same-user-only) as the `pin` —
possession proves same-host, same-user access and does not consume the
window. This is the spec's "implicitly trusted launcher" made concrete.

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

*Amended 2026-09-22 after code review: one-shot pairing window and the
local launcher token replace the earlier "window open for the life of the
daemon" wording.*
