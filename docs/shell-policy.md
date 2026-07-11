# Shell command policy (Tab 2)

Controls **which binaries** the shared terminal may run. This is separate from [who may type](./access-control.md).

## Modes

| Key | Mode | Behavior |
|-----|------|----------|
| **`y`** | **YOLO** (default) | Full `$SHELL` (or tmux). No command filtering. |
| **`w`** | **Whitelist** | Gated shell: only programs you list may run. |
| **`a`** | **Approve** | Gated shell: every command waits for host **Y/n**. Peers see a wait overlay. |

Changing mode **restarts the PTY**.

## Whitelist editing

| Key | Action |
|-----|--------|
| `+` / `=` | Add program (type name, Enter) |
| `-` / `d` | Remove selected program |
| `↑` / `↓` | Move selection |

CLI / env still work:

```bash
cargo run -- serve app.xdc --allowed-commands ls,cat,pwd
# or
DC_ALLOWED_COMMANDS=ls,cat,pwd
```

That starts the agent already in **whitelist** mode.

## Approve flow

1. Someone types a command in the webxdc (shared gated shell).  
2. Terminal prints `waiting for host approval`.  
3. All clients get `{ "t": "cmd_wait", "line": "…" }` → **wait overlay**.  
4. Host on **Shell tab** chooses:
   - **`y`** — run **once**  
   - **`w`** — run **and add the binary to the whitelist** (next time auto-runs without asking)  
   - **`n`** — deny  
5. Clients get `cmd_done` → overlay clears; command runs or is refused.  

Whitelisted binaries skip the prompt while still in approve mode (`[whitelisted] running…`).

Only **one command line** at a time (no pipes/redirection in gated modes).

### Tab completion (whitelist / approve)

Gated modes use **rustyline**:

| What | Completes |
|------|-----------|
| First word | Whitelist names (whitelist mode) or PATH commands (approve) + builtins |
| Later words | File / directory paths |

Arrow keys, history, and Ctrl-C (cancel line) also work.

## tmux

- Only in **YOLO** mode (`t` toggles tmux).  
- Whitelist / Approve use the gated shell process, not free bash/tmux.  

## Related

- [Agent TUI](./agent.md)  
- [Access control (who types)](./access-control.md)  
