# Realtime protocol

Transport: **webxdc `joinRealtimeChannel()`** → Delta Chat Iroh gossip for that app message.

Two families of frames share the same channel:

1. **Binary terminal** — first byte is a kind letter  
2. **Presence JSON** — first byte is `{` (0x7b)

## Binary frames

| Kind | Byte | Direction | Payload |
|------|------|-----------|---------|
| Input | `I` (0x49) | Peer → host | See [Input framing](#input-framing) |
| Output | `O` (0x4f) | Host → peers | Raw PTY bytes |
| Resize | `R` (0x52) | Peer → host | 4 bytes LE: `cols_u16`, `rows_u16` |
| Ping | `P` (0x50) | Peer → host | empty; host may pong in the terminal stream |
| Exit | `E` (0x45) | Host → peers | empty; shell ended |

Host **ignores** peer-originated `O` / `E`. Only the host may publish terminal content.

### Input framing

Versioned format (required for access control):

```text
I | 0x01 | u16le addr_len | addr UTF-8 | key payload…
```

- Version byte `0x01` avoids collisions with raw keystrokes.  
- `addr` is `webxdc.selfAddr` (email **or** hashed identity — may be long, no `@`).  
- Host checks write permission for that addr, then writes **only** the key payload to the PTY.  
- Legacy bare `I`+keys (no version): empty addr; allowed only in party mode.

Example (conceptual):

```text
I  0x01  len=14  user@example.com  h e l l o
```

## Presence JSON

UTF-8 JSON object; first character `{`.

Common fields:

| Field | Meaning |
|-------|---------|
| `t` | Type: `join` \| `leave` \| `pos` \| `writer` \| `in` \| `rx` |
| `a` | Peer address (`selfAddr`) |
| `n` | Display name |
| `c` / `cl` | Color / light color |

### Types

| `t` | Extra fields | Purpose |
|-----|--------------|---------|
| `join` | — | Announce presence; peers answer with their own join |
| `leave` | — | Leaving the app |
| `pos` | `x`, `y`, `b` (`up`/`down`) | Shared cursor (stage-local px) |
| `writer` | `at` | Typing into the terminal |
| `in` | — | Input activity signal (alongside binary `I`) |
| `rx` | `e` emoji, `x`, `y`, `i` id | Ephemeral reaction burst |

Cursors and reactions are **peer-to-peer**. The agent may log them for the TUI but does not re-relay them.

## Client send path (webxdc)

- Desktop: `RealtimeListener.send(Uint8Array)` → IPC as `number[]` → core.  
- Receive: normalize to `Uint8Array` (array / TypedArray / Uint8Array).  

## Host send path (agent)

- PTY read loop → `O` + bytes → `send_webxdc_realtime_data`  
- Access-controlled `I` → `write_pty`  
- Resize → `portable_pty` resize  

## Versioning notes

- Adding new binary kinds: use unused ASCII capital letters and ignore unknown kinds.  
- Presence: unknown `t` values should be ignored by clients.  


Realtime sender identity is not exposed to TerminalBuddy. The address in an input
or presence frame is self-declared; allow-list checks do not authenticate it.
Clients also cannot authenticate output or exit frames. All participants must be
trusted. Only explicitly hosted app instances accept input or resize frames;
geometry is capped at 500 by 200 and output is batched up to 64 KiB at 50 frames/s.
Presence joins are periodic and never answered with another join.


`C | requester-address-utf8` (1–512 address bytes) explicitly requests the latest
server tmux buffer from an actively hosted instance. The host responds with JSON
`{"t":"clipboard_reply","a":"requester","text":"…"}` or an `error` field.
Only the requesting address displays the copy card. Responses are shared-channel
data, not private or authenticated. The host bounds reads to 16 KiB and three
seconds, preserves UTF-8 text and trailing newlines, and never types it into the shell.
