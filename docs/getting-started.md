# Getting started

## Prerequisites

- Rust (for the agent)
- Node.js + npm (for the webxdc build)
- `zip` (packs the `.xdc`)
- Optional: `tmux` (only if you enable it — **off by default**)
- A Delta Chat / chatmail account (created automatically on first run)
- Optional: a **group invite** for the chat you want to host in

## Configure `.env`

```bash
cp .env.example .env
```

**Always single-quote `DC_INVITE`.** `#` and `&` in invite URLs break unquoted shell parsing.

```bash
# Account bootstrap server (chatmail). Default is nine.testrun.org.
# Use your own host to register there instead (see “Account server” below).
DC_AGENT_SERVER=nine.testrun.org
# DC_AGENT_SERVER=mail.example.org

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

On first run the agent creates a profile under `agent/.dc-agent/` and configures an account if none exists yet. That folder is local state; do not commit it.

### Account server (not locked to nine.testrun.org)

By default the host registers on **`nine.testrun.org`**. You can use **any chatmail-compatible server** instead — the same flow Delta Chat uses for that server’s **`/new`** account creation.

Set **`DC_AGENT_SERVER`** before the first configure (env, `.env`, or export):

```bash
# hostname only (usual form)
DC_AGENT_SERVER=mail.example.org

# also accepted — stripped down to the host by the agent
DC_AGENT_SERVER=https://mail.example.org
DC_AGENT_SERVER=DCACCOUNT:mail.example.org
```

```bash
export DC_AGENT_SERVER=mail.example.org
./terminalbuddy serve --allowed-commands ls,cat,pwd
# or from source:
make run-agent
```

What happens under the hood:

1. If `agent/.dc-agent/` is not already configured, the agent builds a QR of the form **`DCACCOUNT:<server>`**.
2. Delta Chat core uses that to create a throwaway account on the server (chatmail **`/new`** bootstrap).
3. Later runs **reuse** the saved account in `.dc-agent/` and do **not** re-register, even if you change `DC_AGENT_SERVER`.

To switch servers, remove the old profile (or point the process at a fresh data dir) **before** starting again with the new `DC_AGENT_SERVER`. Interactive `join` (without the env var) can also prompt: `server [default nine.testrun.org]`.

The server only decides **where TerminalBuddy’s address lives**. Peers can still be on other chatmail servers; SecureJoin / invites work across them as usual.

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
