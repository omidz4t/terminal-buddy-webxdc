# Getting started

## Prerequisites

- Rust (for the agent)
- Node.js + npm (for the webxdc build)
- `zip` (packs the `.xdc`)
- Optional: `tmux` (only if you enable it — **off by default**)
- A Delta Chat account on a server (default bootstrap: `nine.testrun.org`)
- A **group invite** for the chat you want to host in

## Configure `.env`

```bash
cp .env.example .env
```

**Always single-quote `DC_INVITE`.** `#` and `&` in invite URLs break unquoted shell parsing.

```bash
DC_AGENT_SERVER=nine.testrun.org

# Group SecureJoin invite — must be single-quoted
DC_INVITE='https://i.delta.chat/#...&g=MyGroup'

# Re-join that group on every serve (recommended after changing the invite)
DC_FORCE_JOIN=1

# Optional:
# DC_USE_TMUX=1
# DC_ALLOWED_COMMANDS=ls,cat,pwd,whoami
# DC_TMUX_SESSION=terminalbuddy
# DC_AGENT_SHELL=/bin/bash
# DC_XDC=/absolute/path/to/terminalbuddy.xdc
```

## Run the host

From the project root:

```bash
make run-agent
```

This will:

1. Build `webxdc/terminalbuddy.xdc`
2. Load `.env`
3. Start the agent with `cargo run -- serve <xdc>` (plus env-derived flags)

Bootstrap steps (console, before the TUI):

1. App package path/size  
2. Mail / identity  
3. Join group from invite (when forced or first time)  
4. Send mini app into the chat  
5. Announcement text  
6. Open Ratatui dashboard  

Open the **newest** mini app message in that group on every device that should participate.

## Useful make targets

| Target | Action |
|--------|--------|
| `make build-webxdc` | npm install + vite build + pack `.xdc` |
| `make run-agent` | build webxdc, then run agent serve |
| `make clean-webxdc` | remove `webxdc/dist` and `.xdc` |

## First-time agent identity

On first run the agent creates a profile under `agent/.dc-agent/` and configures an account via `DCACCOUNT:` + `DC_AGENT_SERVER`. That folder is local state; do not commit it.

## Changing the group

1. Put the new invite in `.env` as `DC_INVITE='...'`  
2. Keep `DC_FORCE_JOIN=1` for at least one successful join  
3. `make run-agent` again  

The agent should log the target chat name/id and send a new `.xdc` into **that** chat.

## Troubleshooting

| Symptom | Check |
|---------|--------|
| Invite ignored / wrong chat | Quote `DC_INVITE`; set `DC_FORCE_JOIN=1` |
| “Mini app not delivered” | SMTP timeout; agent logs; message state in chat |
| Peers don’t connect | Same webxdc **message** instance; both online; host TUI “waiting for peer” |
| Cursors missing | Peers on same instance; mesh up (host must be online for PTY, not for cursor relay) |
| Phone keyboard covers UI | Use a recent build (visualViewport layout) |

More detail: [Architecture](./architecture.md), [Agent](./agent.md), [Webxdc](./webxdc.md).
