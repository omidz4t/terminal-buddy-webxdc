# Agent (host)

Rust binary in `agent/`. Depends on local **deltachat-core** under `context/deltachat-core-rust`.

## Commands

```bash
cd agent

# Help
cargo run -- help

# Create identity + join invite + “Hi” (optional standalone)
cargo run -- join 'https://i.delta.chat/#...'

# Host: TUI + PTY + send webxdc (usual path)
cargo run -- serve ../webxdc/terminalbuddy.xdc

# Send a file once
cargo run -- send-file ../webxdc/terminalbuddy.xdc
```

From project root, preferred:

```bash
make run-agent   # builds .xdc then cargo run -- serve …
```

## `serve` options

Full flag and env reference: **[CLI options](./cli-options.md)**.

```text
serve [app.xdc] [--invite URL] [--force-join] [--allowed-commands a,b,c] [--tmux] [--no-tui]
```

| Flag / env | Default | Meaning |
|------------|---------|---------|
| path to `.xdc` | **embedded** (or `DC_XDC`) | Package to send / host |
| `--invite` | — | SecureJoin + send mini app |
| `--force-join` / `--no-force-join` | force on by default when bootstrapping | Re-join vs reuse group |
| `--allowed-commands` / `DC_ALLOWED_COMMANDS` | empty (**YOLO**) | Whitelist restricted shell |
| `--tmux` / `DC_USE_TMUX=1` | **off** | YOLO tmux session |
| `--no-tmux` | — | Force plain shell |
| `--no-tui` / `--headless` / `DC_NO_TUI=1` | **off** | Headless (no Ratatui; Ctrl-C to stop) |
| `DC_TMUX_SESSION` | `terminalbuddy` | tmux session name |
| `DC_AGENT_SHELL` | `$SHELL` | Plain shell when not tmux/restricted |
| `DC_INVITE` + `DC_BOOTSTRAP_INVITE=1` | — | Env bootstrap (e.g. `make run-agent-inv`) |
| `DC_AGENT_SERVER` | `nine.testrun.org` | Chatmail host for **first-time** account creation only (`DCACCOUNT:` → server `/new`). Set to e.g. `mail.example.org` to avoid nine.testrun.org. Ignored if already configured. |

### Shell modes

| Mode | When | Process on PTY |
|------|------|----------------|
| **Plain shell** | Default | `$SHELL` or `DC_AGENT_SHELL` |
| **tmux** | `--tmux` or TUI tab 2 | `tmux new-session -A -s <session>` |
| **Restricted** | `--allowed-commands` non-empty | This binary as `restricted-shell` (tmux ignored) |

Restricted shell rules: see [Restricted shell](#restricted-shell) below.

## TUI tabs

| Tab | Keys | Purpose |
|-----|------|---------|
| **1 Host** | `↑↓` log | Account, people, log |
| **2 Shell** | `y`/`w`/`a` policy, `+`/`−` whitelist, `Y`/`n` approve | YOLO / whitelist / approve + tmux |
| **3 Access** | `p` party, `Space` grant, `a`/`n` all/none | Who may type |
| **4 Share** | invite field + chat/contact list | Send webxdc to a chat |

### Share tab (send the mini app)

| Key | Action |
|-----|--------|
| `i` / `e` | Edit invite URL field |
| type / paste | Invite (Ctrl-u clears) |
| `Enter` | If field looks like an invite → join + send; else send to selected chat/contact |
| `s` | Always send to the **selected** chat or contact |
| `↑` / `↓` | Move selection in chat/contact list |
| `Esc` | Cancel invite edit |

- **Invite link:** SecureJoin into the group, send `.xdc`, host it.  
- **Chat:** send into an existing group or 1:1.  
- **Contact:** create/open 1:1, then send.  

Invite pre-filled from `DC_INVITE` in `.env` when present.

Global: `Tab` / `1`–`4` switch tabs · `q` quit agent.

**Live in app now** shows display names only (no addresses).  
**Access** tab is documented in [access-control.md](./access-control.md).

## Restricted shell

When `--allowed-commands` is set, the PTY runs:

```text
<agent-binary> restricted-shell --allowed ls,cat,pwd
```

- One command per line  
- First token must be on the allow-list  
- No pipes, redirects, `&&`, `$()`, etc.  
- Builtins: `help`, `exit`, `clear`  

Examples:

```bash
cargo run -- serve app.xdc --allowed-commands ls,cat,pwd,whoami,uname

# .env
DC_ALLOWED_COMMANDS=ls,cat,pwd
```

## Quit behavior

On `q` / shutdown:

1. Flag shutdown for background tasks  
2. Kill PTY child processes  
3. If **tmux mode** was active, `tmux kill-session -t $DC_TMUX_SESSION`  
4. `stop_io` (bounded timeout)  
5. Process exit  

Closing the webxdc on a phone does **not** stop the agent.

## Data directory

- `agent/.dc-agent/` — SQLite profile, blobs, identity  
- Not for git  

## Related

- [Getting started](./getting-started.md)  
- [Access control](./access-control.md)  
- [Protocol](./protocol.md)  
