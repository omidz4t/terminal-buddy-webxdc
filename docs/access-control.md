# Access control

Controls **who may type** into the shared terminal. Anyone who can open the webxdc can still **watch** the stream.

Configured live in the agent TUI **Tab 3 · Access** (no restart required for policy changes).

## Modes

### Party mode (default)

- **Everyone** may type.  
- Status line: `access: party`.  
- Toggle with **`p`** on the Access tab.

### Allow-list mode

- Party mode **off**.  
- Only people you **grant** may type.  
- **Host (you)** always may type.  
- Others can open the app and see output but keystrokes are dropped on the host.

## TUI (tab 3)

| Key | Action |
|-----|--------|
| `p` | Toggle party mode |
| `↑` / `↓` or `k` / `j` | Move selection |
| `Space` / `Enter` | Grant or revoke write for the selected person (turns party off if needed) |
| `a` | Grant all chat members (party off) |
| `n` | Revoke everyone except host (party off) |
| `1` `2` `3` / `Tab` | Switch tabs |
| `q` | Quit agent |

List marks:

- Party on: `[all]` / `[host]`  
- Party off: `[x]` granted · `[ ]` denied  
- Online / writing indicators when known  

## How enforcement works

1. Webxdc sends input as:

   ```text
   I | u8 addr_len | selfAddr | key bytes
   ```

2. Host parses `addr` and calls `is_write_allowed(addr)`.  
3. If denied, payload is **not** written to the PTY (occasional log: `access denied for …`).  

Presence (`pos`, `rx`) is **not** gated by write access—cursors/reactions remain social.

## Policy vs chat membership

| Concept | Meaning |
|---------|---------|
| Chat member | In the Delta Chat group; shown on Host / Access lists |
| Live in app | Currently on the realtime channel |
| `can_write` | Party **or** (allow-list contains addr) **or** is host |

## Defaults

- Party mode: **on** at agent start  
- Allow-list: empty until you grant people  

## Related

- [Protocol — input framing](./protocol.md#input-framing)  
- [Agent TUI](./agent.md#tui-tabs)  
