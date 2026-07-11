# TerminalBuddy — documentation

Shared terminal over **Delta Chat webxdc realtime**: people open the mini app in a chat; a host agent on a machine runs the real shell (or a restricted command list) and streams the screen. Cursors and reactions move peer-to-peer on the same mesh.

## Docs index

| Doc | What it covers |
|-----|----------------|
| [Getting started](./getting-started.md) | Clone layout, `.env`, custom account server (`DC_AGENT_SERVER` / `/new`), `make run-agent`, first join |
| [CLI options](./cli-options.md) | All binary commands, flags, env vars, examples |
| [Architecture](./architecture.md) | Host vs peers, who sends what |
| [Protocol](./protocol.md) | Binary frames + presence JSON |
| [Agent](./agent.md) | TUI tabs, shell/tmux, restricted commands |
| [Access control](./access-control.md) | Party mode vs allow-list (who may type) |
| [Shell policy](./shell-policy.md) | YOLO / whitelist / approve (which binaries run) |
| [Webxdc client](./webxdc.md) | Preact UI, terminal, cursors, reactions, phone keyboard |
| [Makefile and env](./makefile-and-env.md) | Make targets and `.env` |

## Quick map of the repo

```text
terminalbuddy.xdc/
  Makefile              # build-webxdc, run-agent
  .env / .env.example   # invite, server, optional flags
  agent/                # Rust host (deltachat core + PTY + Ratatui)
  webxdc/               # Preact + xterm.js mini app → terminalbuddy.xdc
  docs/                 # this documentation
  context/              # local deltachat-core-rust (gitignored clone)
```

## One-line flow

1. Agent joins a group (from `DC_INVITE`), sends the `.xdc`, hosts a PTY.
2. People open **the same** webxdc message in Delta Chat.
3. Peers send keystrokes / resize / presence; **only the host** sends terminal output.
4. Cursors and reactions propagate on the realtime mesh without host relay.
