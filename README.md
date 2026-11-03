<p align="center">
  <img src="./assets/icon.png" alt="TerminalBuddy logo" width="220">
</p>

<h1 align="center">TerminalBuddy</h1>

<p align="center">
  <strong>Shared terminal over Delta Chat — one binary, mini app embedded.</strong>
</p>

<p align="center">
  <a href="https://github.com/omidz4t/terminal-buddy-webxdc/releases/latest">
    <img alt="Release" src="https://img.shields.io/github/v/release/omidz4t/terminal-buddy-webxdc?display_name=tag&sort=semver">
  </a>
  <a href="https://github.com/omidz4t/terminal-buddy-webxdc/actions/workflows/release.yml">
    <img alt="CI" src="https://github.com/omidz4t/terminal-buddy-webxdc/actions/workflows/release.yml/badge.svg">
  </a>
  <a href="./LICENCE">
    <img alt="Licence" src="https://img.shields.io/badge/licence-GPL--3.0--or--later-blue.svg">
  </a>
</p>

Host a real shell on a machine, ship a mini app into a chat, and let people type together from phones and desktops. Output streams from the host; cursors and reactions move peer-to-peer on the webxdc realtime mesh.

| | |
|---|---|
| **Version** | `0.1.1` (see [`VERSION`](./VERSION)) |
| **Repo** | [github.com/omidz4t/terminal-buddy-webxdc](https://github.com/omidz4t/terminal-buddy-webxdc) |
| **Host** | Rust agent + Ratatui TUI (`agent/`) |
| **Client** | Preact + xterm.js webxdc (`webxdc/`) |
| **Licence** | [GPL-3.0-or-later](./LICENCE) |

---


## One-line run (Linux x86_64)

Download the latest **release binary** (mini app is already embedded), make it executable, start the host:

```bash
wget -qO terminalbuddy https://github.com/omidz4t/terminal-buddy-webxdc/releases/latest/download/terminalbuddy-linux-x86_64 && chmod +x terminalbuddy && ./terminalbuddy serve
```

### Docker (GHCR · Debian 13)

Image is built in CI from the **same** release binary (no second Rust compile). Use **`-it`** so the TUI has a TTY:

```bash
# Pull + run TUI (interactive terminal)
docker run --rm -it \
  -v terminalbuddy-data:/data \
  ghcr.io/omidz4t/terminal-buddy-webxdc:latest \
  serve

# Invite + whitelist (recommended)
docker run --rm -it \
  -v terminalbuddy-data:/data \
  ghcr.io/omidz4t/terminal-buddy-webxdc:latest \
  serve \
    --invite 'https://i.delta.chat/#FINGERPRINT&v=3&x=…&i=…&s=…&a=user%40nine.testrun.org&n=2&g=MyGroup' \
    --allowed-commands ls,cat,pwd,whoami,uname,date,echo

# Headless (no TUI) — drop -t if you only need logs
docker run --rm -i \
  -v terminalbuddy-data:/data \
  ghcr.io/omidz4t/terminal-buddy-webxdc:0.1.1 \
  serve --no-tui --invite 'https://i.delta.chat/#…' --allowed-commands ls,pwd,whoami
```

- **`-it`** — required to see / use the Ratatui dashboard  
- **`-v terminalbuddy-data:/data`** — persists identity under `/data` (`.dc-agent`)  
- Tags: `latest`, `0.1.0` (or whatever version the release published)

```bash
./terminalbuddy --version

# Recommended: join a chat from invite + whitelist (not YOLO)
./terminalbuddy serve \
  --invite 'https://i.delta.chat/#FINGERPRINT&v=3&x=…&i=…&s=…&a=user%40nine.testrun.org&n=2&g=MyGroup' \
  --allowed-commands ls,cat,pwd,whoami,uname,date,echo

# Optional: force SecureJoin even if a group already exists
./terminalbuddy serve \
  --invite 'https://i.delta.chat/#…' \
  --force-join \
  --allowed-commands ls,cat,pwd,whoami
```

- **`--invite`** — SecureJoin the group/contact and send the embedded mini app (always **single-quote** the URL; `#` is a shell comment).
- **`--force-join`** / **`--no-force-join`** — Resolve the supplied SecureJoin invite. Neither option selects a group by chat-list order.
- **`--allowed-commands`** — **whitelist** only those commands (restricted shell). Without this, serve runs in **YOLO** (full shell) — avoid that when others can type.
- **`--allowed`** — Alias for `--allowed-commands`.
- **`--tmux`** / **`--no-tmux`** — YOLO + tmux, or force plain shell (`--tmux` ignored under whitelist).
- **`--no-tui`** — Headless host (logs to stdout, Ctrl-C to stop; pair with `--invite`).
- **`[app.xdc]`** — Optional package path; default is the **embedded** mini app.
- **`--help`** / **`--version`** — Help and version (embedded xdc info).
- Omit **`--invite`** to open the TUI only and send later from **tab 4 Share**.

Full list of commands, flags, and env vars: **[docs/cli-options.md](./docs/cli-options.md)**.

> Releases are published by [GitHub Actions](https://github.com/omidz4t/terminal-buddy-webxdc/actions) when you push a tag like `v0.1.0`.

---

## WARNING

**This program was written with Grok and heavy use of AI.**

Treat it as experimental software:

- **Know what it can do.** The host runs a real shell (or restricted commands) on *your* machine. Peers who can type may run commands as the host user, subject to the shell/access policies you set.
- **Understand the power and the risk.** Sharing a terminal is powerful and dangerous. Wrong access settings, YOLO mode, or an untrusted chat can lead to data loss, compromise, or unwanted activity on the host.
- **Use it with your own judgment.** Review the code, configure access carefully (whitelist / approve / allow-list), and only host for people you trust. You are responsible for how you run and share it.
- **Expect rough edges.** AI-assisted code can contain bugs, incomplete paths, or surprising behavior. Do not rely on this for production security or critical systems without your own review.

**Use at your own risk.** The authors and AI tools used to write this offer no warranty (see [LICENCE](./LICENCE)).

---

## How it works

```text
  You (host agent)                    Peers (Delta Chat)
  ─────────────────                   ──────────────────
  owns the PTY                        open the mini app
  sends O… / E  ──────────────────►   see the terminal
  receives I / R / P  ◄────────────   type, resize, ping
  observes presence   ◄── mesh ──►    cursors + reactions (P2P)
```

1. The **agent** signs in as **TerminalBuddy**, opens a dashboard, and (when you choose) sends `terminalbuddy.xdc` into a chat.
2. Everyone opens the **same** mini-app message in that chat.
3. **Only the host** owns the shell and publishes terminal bytes.
4. Cursors / reactions propagate on Iroh realtime **without** host relay.

More detail: [docs/architecture.md](./docs/architecture.md), [docs/protocol.md](./docs/protocol.md).

---

## Prerequisites

- **Rust** (agent)
- **Node.js + npm** (webxdc build)
- **`zip`** (packs the `.xdc`)
- Optional: **`tmux`** (off by default)
- A Delta Chat / chatmail account — created on first run (default server: `nine.testrun.org`; override with **`DC_AGENT_SERVER`**, see [Account server](./docs/getting-started.md#account-server-not-locked-to-ninetestrunorg))

The agent expects a local checkout of **deltachat-core-rust** at `context/deltachat-core-rust` (path dependency; gitignored clone).

---

## Quick start (from source)

If you prefer building locally instead of the [one-line release download](#one-line-run-linux-x86_64):

```bash
# 1) Optional env (server, shell flags). Invite only needed for run-agent-inv.
cp .env.example .env

# 2) Clone deltachat core next to the agent (path dep)
#    git clone https://github.com/chatmail/core.git context/deltachat-core-rust

# 3) One release binary with the mini app embedded
make build
./dist/terminalbuddy --version
./dist/terminalbuddy serve          # Share tab sends the embedded .xdc

# Dev loop (rebuild embed + cargo run)
make run-agent
```

In the TUI:

| Tab | What |
|-----|------|
| **1 Host** | Status, log, who is online |
| **2 Shell** | YOLO / whitelist / approve · tmux toggle |
| **3 Access** | Party mode vs who may type |
| **4 Share** | Paste invite or pick a chat/contact → send mini app |

Peers open the **newest** TerminalBuddy mini app in that chat.

### Bootstrap from invite (optional)

When you want the agent to join a group and send the `.xdc` automatically:

```bash
# .env — always single-quote invites (# is a shell comment otherwise!)
DC_INVITE='https://i.delta.chat/#...&g=...'
DC_FORCE_JOIN=1

make run-agent-inv
```

| Target | Invite behavior |
|--------|-----------------|
| `make run-agent` | **Ignores** `DC_INVITE` — send manually via **Share** |
| `make run-agent-inv` | Join + send using `DC_INVITE` from `.env` |

---

## Make targets

```bash
make help            # list targets
make build           # webxdc + release binary with .xdc embedded → dist/terminalbuddy
make build-webxdc    # npm install + vite build → webxdc/terminalbuddy.xdc
make run-agent       # embed + host TUI (manual Share)
make run-agent-inv   # embed + join/send from DC_INVITE
make clean           # dist, embed, webxdc artifacts, cargo clean
```

The release binary **embeds** `terminalbuddy.xdc` — no separate file needed to send into chat:

```bash
./dist/terminalbuddy --version     # also: -V, --verison
./dist/terminalbuddy serve
./dist/terminalbuddy serve --tmux
./dist/terminalbuddy serve --allowed-commands ls,cat,pwd,whoami
# optional override still works:
./dist/terminalbuddy serve /path/to/other.xdc
```

---

## Configuration (`.env`)

| Variable | Used by | Notes |
|----------|---------|--------|
| `DC_AGENT_SERVER` | both | First-time account host only (default `nine.testrun.org`). Use your chatmail host, e.g. `mail.example.org` — registers via `DCACCOUNT:` / that server’s `/new` |
| `DC_INVITE` | **run-agent-inv only** | SecureJoin URL — **single-quoted** |
| `DC_FORCE_JOIN` | run-agent-inv | `1` = re-join even if a group already exists |
| `DC_USE_TMUX` | both | `1` → start with tmux |
| `DC_ALLOWED_COMMANDS` | both | Comma list → restricted shell |
| `DC_AGENT_SHELL` | both | Override plain shell binary |
| `DC_TMUX_SESSION` | both | Default session name `terminalbuddy` |
| `DC_XDC` | both | Override path to the `.xdc` |

See [docs/makefile-and-env.md](./docs/makefile-and-env.md) and [`.env.example`](./.env.example).

---

## Shell & access

**Shell policy** (TUI tab 2, or CLI flags):

| Mode | Behavior |
|------|----------|
| **YOLO** | Full `$SHELL` (or tmux if enabled) |
| **Whitelist** | Only listed command names |
| **Approve** | Host presses `y` / `n` / `w` per command; peers wait |

**Who can type** (TUI tab 3):

- **Party mode** — everyone in the mesh may type  
- **Allow-list** — only selected contacts + host  

Watching the terminal only needs the mini app open; typing is gated separately.

Docs: [shell-policy](./docs/shell-policy.md), [access-control](./docs/access-control.md).

---

## Project layout

```text
.
├── VERSION                 # project version
├── Dockerfile              # Debian 13 runtime (COPY prebuilt binary only)
├── Makefile
├── .env.example
├── assets/                 # brand art + icon source
├── agent/                  # Rust host (terminal_buddy)
│   ├── assets/avatar.png   # Delta Chat profile picture
│   └── src/                # main, TUI, restricted shell
├── webxdc/                 # Preact mini app → terminalbuddy.xdc
│   └── public/icon.png     # webxdc icon
├── docs/                   # full documentation
└── context/                # local deltachat-core-rust (not committed)
```

Local identity lives under `agent/.dc-agent/` — **do not commit** it.

---

## Documentation

| Doc | Topic |
|-----|--------|
| [docs/README.md](./docs/README.md) | Doc index |
| [CLI options](./docs/cli-options.md) | All flags, env vars, examples |
| [Getting started](./docs/getting-started.md) | First run, troubleshooting |
| [Architecture](./docs/architecture.md) | Host vs peers |
| [Protocol](./docs/protocol.md) | Frames `I` `O` `R` `P` `E` + presence JSON |
| [Agent](./docs/agent.md) | TUI tabs & host details |
| [Webxdc client](./docs/webxdc.md) | UI, keyboard, cursors |

---

## Troubleshooting

| Symptom | What to try |
|---------|-------------|
| Wrong chat / not a member | Use **Share** tab, or `make run-agent-inv` with quoted `DC_INVITE` + `DC_FORCE_JOIN=1` |
| Peers see no output | Same webxdc **message** instance; host still running; network online |
| Cursors / reactions missing | Peers must be on the same instance; mesh needs connectivity |
| Invite truncated | Single-quote `DC_INVITE` — `#` starts a shell comment |
| Phone keyboard covers UI | Recent build uses `visualViewport` layout |

---

## Security notes

- The host machine runs real commands. Host only for trusted participants; whitelist and approve modes are not sandboxes.
- `DC_INVITE` and `.env` are secrets for private groups — keep them out of git (`.env` is gitignored).
- Profile name is set to **TerminalBuddy** with the project avatar on agent start.

---

## GitHub Actions / releases

Workflow: [`.github/workflows/release.yml`](./.github/workflows/release.yml)

| Event | What happens |
|-------|----------------|
| Push to `main` | Build → **GitHub Release** `v$(cat VERSION)` + GHCR image `:VERSION` and `:latest` |
| Push tag `v*` | Same (version taken from the tag) |
| Manual `workflow_dispatch` | Same |

Release notes include **TerminalBuddy version**, **git commit**, **Rust toolchain**, and the exact **chatmail/core (deltachat)** package version + commit used for the build.

**Docker reuses the CI binary** (`Dockerfile` only `COPY dist/terminalbuddy` onto Debian 13 — no second `cargo build`).

```bash
# bump VERSION, then push main (or tag) to publish
echo 0.1.1 > VERSION
git commit -am "release 0.1.1"
git push origin main
# optional explicit tag:
git tag v0.1.1 && git push origin v0.1.1
```

Assets:

- Binary: `https://github.com/omidz4t/terminal-buddy-webxdc/releases/latest/download/terminalbuddy-linux-x86_64`
- Image: `ghcr.io/omidz4t/terminal-buddy-webxdc:0.1.1`
---

## Licence

This project is licensed under the **GNU General Public License v3.0 or later** — see [`LICENCE`](./LICENCE).


### Security boundaries and review fixes

Realtime frames have no authenticated sender identity exposed to this app.
The input allow-list checks a self-declared address: a participant can impersonate
an allowed writer. Any participant can also forge terminal output or an exit frame
in other viewers. Treat the displayed terminal and presence as trusted-group data,
not authenticated host output. Party mode allows every participant to type.
Whitelist mode restricts executable names but leaves arguments free; it does not
isolate files, network access, or the host account. Approve mode requires the TUI;
headless startup uses YOLO or the configured whitelist.

Only app instances explicitly sent by this host are served. Sharing a replacement
stops previous shells, and remote advertisements or frames cannot start shells.
On startup a supplied invite determines the target, including with `--no-force-join`;
an accepted existing group is reused only when its group ID matches that invite. Removing a member is detected by the
15-second member refresh and stops hosting. Restart or share a fresh app to resume;
old app topics cannot be revoked cryptographically and retain previously shared data.
Changes to membership are not an instantaneous access revocation mechanism.

Remote terminal geometry is limited to 500 columns by 200 rows. Output is coalesced
into frames of at most 64 KiB with at most 50 broadcasts per second, retaining
bounded backpressure. Presence announces periodically without answering joins.
The host refreshes advertisements every 15 seconds; recovery after every kind of
network stall is not guaranteed.

The profile directory is set to mode 0700 on Unix and contains account secrets.
Embedded packages are extracted into private random temporary directories.
Use a dedicated unprivileged host account. `DC_TMUX_SESSION` selects the tmux
session (default `terminalbuddy`); an existing session of that name is reused.
Release CI pins core to the reviewed commit and actions to full commit hashes.
