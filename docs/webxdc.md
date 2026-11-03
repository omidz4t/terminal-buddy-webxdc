# Webxdc client

Source: `webxdc/`. Built with **Vite + Preact + xterm.js**, packed to `terminalbuddy.xdc`.

## Build

```bash
make build-webxdc
# or
cd webxdc && npm install && npm run build
```

Output: `webxdc/terminalbuddy.xdc` (IIFE bundle, `base: ./`, no module `crossorigin`—friendly to webxdc webviews).

Requires Delta Chat’s injected `webxdc.js` (`window.webxdc.joinRealtimeChannel`).

## UI structure

```text
src/
  main.jsx              # render App
  App.jsx               # terminal, realtime, gestures
  presence.js           # peer state, pos/rx protocol
  components/
    Titlebar.jsx
    TerminalHost.jsx    # never re-renders (xterm owns DOM)
    PeerCursors.jsx     # imperative cursor paint
    ReactionBursts.jsx
    ReactionPie.jsx
  hooks/
    useWebxdc.js
    useVisualViewport.js  # phone keyboard layout
```

## Features

### Terminal

- xterm.js + FitAddon  
- Full key passthrough (tmux prefix `Ctrl-b` if host uses tmux)  
- Resize: window, `visualViewport`, `ResizeObserver` on stage → `R` frame to host  
- Input: framed with `selfAddr` for access control (see [protocol](./protocol.md))  

### Shared cursors

- Always on pointer move (stage-local coordinates)  
- Not painted for self; peers only  
- High-rate updates coalesced; cursor layer avoids thrashing xterm’s DOM  

### Reactions

- **Alt + mouse move** (threshold) opens a fixed pie (does not follow the pointer after open)  
- Click / pointerup on emoji → `rx` presence + local burst  
- Pie stays usable after Alt release until click outside or Esc  

### Phone keyboard

- `useVisualViewport` sets CSS vars so `#app` fills the **visible** viewport  
- Viewport meta: `interactive-widget=resizes-content`  
- Title bar compact while `html.keyboard-open`  
- FitAddon re-runs when the visual viewport resizes  

## Status bar

Shows connection state: waiting for host → P2P connected → errors.

## Development notes

- **Do not put Preact children inside the xterm host** after open—Preact would wipe xterm’s DOM (flicker). `TerminalHost` is `memo(..., () => true)`.  
- Reactivity is for status, peers list, bursts, pie—not per-mousemove.  

## Related

- [Architecture](./architecture.md)  
- [Protocol](./protocol.md)  


### Copying from terminal tools

Tools that emit OSC 52 clipboard requests now show a **Terminal requested a copy**
card. Review the text and tap **Copy to clipboard** on your device. If the webview
blocks clipboard APIs, the text remains selectable for your device’s Copy action.
Requests never read your clipboard and never silently overwrite other viewers’
clipboards. The tmux host forwards these requests too. Tools that only run remote
`xclip`, `wl-copy`, or `pbcopy` must be configured to emit OSC 52 instead.

To check the path from the shell:

```sh
printf '\033]52;c;SGVsbG8gZnJvbSBUZXJtaW5hbEJ1ZGR5\007'
```

A copy card should offer “Hello from TerminalBuddy”. Existing app messages must be
replaced with a newly built/shared app to receive this client update.
