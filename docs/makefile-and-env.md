# Makefile and environment

## Makefile targets

| Target | Description |
|--------|-------------|
| `make help` | List targets |
| `make build-webxdc` | `npm install` + `npm run build` → `webxdc/terminalbuddy.xdc` |
| `make run-agent` | Always builds webxdc first, sources `.env`, runs agent `serve` |
| `make clean-webxdc` | Remove dist + `.xdc` |

`run-agent` passes through:

- `DC_USE_TMUX=1` → `--tmux`  
- `DC_ALLOWED_COMMANDS=…` → `--allowed-commands …`  

## `.env` variables

| Variable | Required | Notes |
|----------|----------|--------|
| `DC_INVITE` | for group host | **Single-quoted** invite URL |
| `DC_FORCE_JOIN` | recommended `1` when changing groups | SecureJoin on serve |
| `DC_AGENT_SERVER` | no | Default `nine.testrun.org` |
| `DC_USE_TMUX` | no | `1` / `true` / `yes` enable tmux |
| `DC_TMUX_SESSION` | no | Default `terminalbuddy` |
| `DC_AGENT_SHELL` | no | Plain shell binary |
| `DC_ALLOWED_COMMANDS` | no | Comma/space list of command names |
| `DC_XDC` | no | Override path to `.xdc` |

Copy from `.env.example`. Never commit real `.env` invites if the repo is shared.

## Quote rule for invites

```bash
# Wrong — shell treats # as comment
DC_INVITE=https://i.delta.chat/#DEADBEEF&v=3&...

# Right
DC_INVITE='https://i.delta.chat/#DEADBEEF&v=3&...'
```
