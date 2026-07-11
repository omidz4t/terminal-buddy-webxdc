# Architecture

## Roles

| Role | Where | Responsibility |
|------|--------|----------------|
| **Host** | `agent/` on a machine | Owns the PTY (shell / tmux / restricted shell). Joins the chat, sends the webxdc, streams terminal output. Enforces who may type. |
| **Peers** | Delta Chat webxdc on phones/desktops | Open the app, show terminal, send keys/resize/presence. |

There is no central app server beyond Delta Chat mail + **Iroh realtime** (webxdc peer channels).

## What each side propagates

```text
  Peer A ── I (input), R (resize), P (ping) ──► Host PTY
  Peer A ── { pos, rx, join, … } ──► mesh ──► Peer B   (natural P2P)
  Host   ── O (output), E (exit) ────────────► all peers
```

### Peers publish

- **Intent for the host:** key input, resize, ping  
- **Social presence for everyone:** cursor position, reactions, join/leave/writer  

### Host alone publishes terminal content

- **`O`…** PTY stdout/stderr bytes  
- **`E`** shell/session ended  

The host **observes** presence JSON for its TUI (who is live / writing) but **does not rebroadcast** cursors or reactions. Those ride the Iroh gossip mesh between webxdc clients.

## Components

### Agent (`agent/`)

- **deltachat-core** (local path `context/deltachat-core-rust`)  
- Join group (`DC_INVITE`), send `.xdc`, host realtime for that message id  
- PTY via `portable-pty` (default: plain `$SHELL`)  
- Ratatui TUI: Host / Shell / Access tabs  
- Access policy: party mode or write allow-list  
- Optional restricted shell (`--allowed-commands`)  

### Webxdc (`webxdc/`)

- Preact UI + xterm.js + FitAddon  
- `joinRealtimeChannel()` for binary + JSON frames  
- Shared cursors, Alt+move reaction pie  
- Mobile: layout pinned to `visualViewport` when the keyboard opens  

## Session identity

Realtime is scoped per **webxdc message instance** (topic id on the original app message). Everyone must open the **same** mini app message the host is serving—not an older copy from a previous send.

## Access model (high level)

- **Watch:** anyone who can open the webxdc sees terminal output.  
- **Type:** party mode (default) = everyone; or allow-list = only granted people + host.  

See [Access control](./access-control.md).

## Process lifecycle

| Event | Effect |
|-------|--------|
| Close webxdc on phone | Client leaves; host and PTY keep running |
| Quit agent (`q`) | Stop IO, kill PTY child; if tmux mode was on, kill named tmux session |
| Toggle tmux on Shell tab | Restart PTY; leaving tmux kills the named session |

## Related docs

- [Protocol](./protocol.md) — frame formats  
- [Agent](./agent.md) — CLI and TUI  
- [Webxdc](./webxdc.md) — client behavior  
