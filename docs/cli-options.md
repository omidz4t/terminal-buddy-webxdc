# CLI options

Full reference for the `terminalbuddy` / `terminal_buddy` binary.  
Also: `./terminalbuddy --help` and `./terminalbuddy serve --help`.

---

## Global

```bash
terminalbuddy <command> [options]
terminalbuddy --help
terminalbuddy --version
```

| Option | Meaning |
|--------|---------|
| **`--help`**, **`-h`**, **`help`** | Print full help and exit |
| **`--version`**, **`-V`**, **`version`** | Print version + whether the mini app is embedded (also accepts typo **`--verison`**) |

---

## Commands

| Command | Meaning |
|---------|---------|
| **`serve [app.xdc] [options]`** | Host the TUI + PTY; optional path to a `.xdc` (default: **embedded** package) |
| **`join [invite-url]`** | Create identity if needed, SecureJoin the invite, send a short Hi |
| **`send-file <path> [chat-id]`** | Send a file once into a chat (debug helper) |
| **`help`** | Same as `--help` |
| **`version`** | Same as `--version` |
| **`restricted-shell …`** | Internal PTY helper spawned by `serve` — do not call directly |

---

## `serve` options

### Chat / invite bootstrap

- **`--invite URL`** — SecureJoin the group/contact and send the embedded mini app into that chat. Always **single-quote** the URL; `#` is a shell comment otherwise.  
  Example: `--invite 'https://i.delta.chat/#FINGERPRINT&v=3&…'`
- **`--invite=URL`** — Same as `--invite URL` (equals form).
- **`--force-join`** — Force SecureJoin even if a group chat already exists (default behaviour when bootstrapping unless you pass `--no-force-join`).
- **`--no-force-join`** — Do not re-join; reuse the first existing group if one is present.

Omit **`--invite`** (and do not set bootstrap env) to open the TUI only and send later from **tab 4 Share**.

### Shell policy (what peers can run)

- **`--allowed-commands a,b,c`** — **Whitelist** only those command names (restricted shell). **Not YOLO.** Preferred when others can type.  
  Example: `--allowed-commands ls,cat,pwd,whoami,uname,date,echo`
- **`--allowed a,b,c`** — Alias for `--allowed-commands`.
- **`--allowed-commands=a,b,c`** / **`--allowed=a,b,c`** — Equals forms of the same.

Without any allowed-commands list, `serve` starts in **YOLO** (full `$SHELL` / optional tmux) — avoid that when untrusted people can type. You can still switch policy live on TUI **tab 2 Shell** (`y` / `w` / `a`).

### Tmux / plain shell (YOLO only)

- **`--tmux`** — Host PTY in a tmux session (YOLO). **Ignored** when `--allowed-commands` is set.
- **`--no-tmux`** — Force plain shell (`$SHELL` or `DC_AGENT_SHELL`).

### UI

- **`--no-tui`** — Headless host: no Ratatui dashboard. Logs stream to stdout; **Ctrl-C** stops the process.  
  Alias: **`--headless`**. Env: **`DC_NO_TUI=1`**.  
  Use with **`--invite`** (there is no Share tab). Prefer **`--allowed-commands`** — approve mode has no keys without the TUI.

### Package path

- **`[app.xdc]`** (positional) — Optional path to a webxdc package to send/host.  
  If omitted: use **`DC_XDC`**, then the **embedded** mini app from `make build`, then common on-disk fallbacks.

### Meta

- **`-h`**, **`--help`** — Serve-specific help.
- **`-V`**, **`--version`** — Version (same as global).

---

## Examples

```bash
# Version + embedded mini-app info
./terminalbuddy --version

# Recommended: invite + whitelist (not YOLO)
./terminalbuddy serve \
  --invite 'https://i.delta.chat/#FINGERPRINT&v=3&x=…&i=…&s=…&a=user%40nine.testrun.org&n=2&g=MyGroup' \
  --allowed-commands ls,cat,pwd,whoami,uname,date,echo

# Force re-join + whitelist
./terminalbuddy serve \
  --invite 'https://i.delta.chat/#…' \
  --force-join \
  --allowed-commands ls,cat,pwd,whoami

# TUI only — send later from Share tab
./terminalbuddy serve --allowed-commands ls,cat,pwd

# Headless (no dashboard) + invite + whitelist
./terminalbuddy serve \
  --no-tui \
  --invite 'https://i.delta.chat/#…' \
  --allowed-commands ls,cat,pwd,whoami

# YOLO full shell (dangerous if others can type)
./terminalbuddy serve --tmux

# Override embedded package with a path
./terminalbuddy serve /path/to/other.xdc --allowed-commands ls,pwd
```

---

## Environment variables

These work with the binary and with `make run-agent` / `make run-agent-inv` (from `.env`).

| Variable | Meaning |
|----------|---------|
| **`DC_AGENT_SERVER`** | Chatmail / account bootstrap server (default `nine.testrun.org`) |
| **`DC_AGENT_SHELL`** | Plain shell binary when not using tmux / restricted shell |
| **`DC_USE_TMUX=1`** | Same as `--tmux` |
| **`DC_TMUX_SESSION`** | tmux session name (default `terminalbuddy`) |
| **`DC_ALLOWED_COMMANDS`** | Same as `--allowed-commands` (comma/space list) |
| **`DC_INVITE`** | SecureJoin invite URL — **single-quote in `.env`** |
| **`DC_BOOTSTRAP_INVITE=1`** | Auto join/send from `DC_INVITE` (used by `make run-agent-inv`) |
| **`DC_FORCE_JOIN`** | `1` = force SecureJoin; `0` = may reuse existing group |
| **`DC_XDC`** | Override path to `.xdc` (else embedded / CLI path) |
| **`DC_NO_TUI=1`** | Same as `--no-tui` (headless host) |

### Quote rule for invites

```bash
# Wrong — shell treats # as comment
DC_INVITE=https://i.delta.chat/#DEADBEEF&v=3&...

# Right
DC_INVITE='https://i.delta.chat/#DEADBEEF&v=3&...'

./terminalbuddy serve --invite 'https://i.delta.chat/#DEADBEEF&v=3&...'
```

---

## Docker (GHCR)

Runtime image is **Debian 13**; the binary is built in CI and copied into the image (no Rust rebuild).

```bash
# TUI needs a TTY: -it
docker run --rm -it \
  -v terminalbuddy-data:/data \
  ghcr.io/omidz4t/terminal-buddy-webxdc:latest \
  serve --invite 'https://i.delta.chat/#…' --allowed-commands ls,cat,pwd,whoami
```

See root [README](../README.md#docker-ghcr--debian-13) for more.

## Related

- [Agent](./agent.md) — TUI tabs, Share, restricted shell internals  
- [Shell policy](./shell-policy.md) — YOLO / whitelist / approve  
- [Access control](./access-control.md) — who may type  
- [Makefile and env](./makefile-and-env.md) — make targets and `.env`
