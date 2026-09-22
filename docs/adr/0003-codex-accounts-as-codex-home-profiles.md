# Codex account switching via per-profile CODEX_HOME

A *profile* is a name bound to an isolated `CODEX_HOME` directory under
`~/.maplayer/profiles/<name>/`, each holding its own `auth.json` and
`config.toml`. The server passes `CODEX_HOME=<profile dir>` when spawning the
codex ACP adapter, so account switching needs no codex-side support.

Why this mechanism: upstream codex assumes a single account under `~/.codex`;
multiple accounts are only supported today by relocating the whole home
(`CODEX_HOME` is the documented knob — the same approach community tools like
`aisw` use). Swapping a directory per spawn is atomic, side-effect-free for
the user's own `~/.codex`, and keeps each account's session files separate.

Consequences:

- **Login happens on the host.** `codex login` (ChatGPT OAuth) or
  `codex login --with-api-key` must run where the browser/credential lives —
  the desktop can drive it; Android only picks the profile. Creating a
  profile returns a login *instruction*, not a credential prompt.
- **Switching is per-spawn, not global.** Running sessions keep their
  account; the profile affects only new spawns (with a default for when the
  field is omitted).
- Cursor has no equivalent need in v1 (single account via `cursor_login`).
