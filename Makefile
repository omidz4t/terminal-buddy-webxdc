# TerminalBuddy — webxdc + Delta Chat agent
#
#   make build           webxdc + release binary with .xdc embedded → dist/terminalbuddy
#   make build-webxdc    build webxdc/terminalbuddy.xdc only
#   make run-agent       build embed + run agent (manual Share tab; ignores DC_INVITE)
#   make run-agent-inv   build embed + run agent, join/send using DC_INVITE from .env
#
# .env: quote DC_INVITE with single quotes — '#' is a shell comment otherwise!

ROOT     := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
AGENT    := $(ROOT)/agent
WEBXDC   := $(ROOT)/webxdc
XDC_OUT  := $(WEBXDC)/terminalbuddy.xdc
XDC_EMBED:= $(AGENT)/assets/terminalbuddy.xdc
DIST     := $(ROOT)/dist
BIN_NAME := terminalbuddy
ENV_FILE := $(ROOT)/.env

.PHONY: help build build-webxdc embed-xdc run-agent run-agent-inv clean-webxdc clean

help:
	@echo "TerminalBuddy"
	@echo "  make build           webxdc + release binary (embedded .xdc) → dist/$(BIN_NAME)"
	@echo "  make build-webxdc    npm build → webxdc/terminalbuddy.xdc"
	@echo "  make run-agent       start host TUI (Share tab to send; no .env invite)"
	@echo "  make run-agent-inv   start host + join/send from DC_INVITE in .env"
	@echo "  make clean           remove dist, embed, webxdc build artifacts"
	@echo ""
	@echo "Binary:  dist/$(BIN_NAME) --version"
	@echo "         dist/$(BIN_NAME) serve"
	@echo ""
	@echo "Env (.env) — only used by run-agent-inv for invite:"
	@echo "  DC_INVITE='https://i.delta.chat/#...&g=...'"
	@echo "  DC_FORCE_JOIN=1"
	@echo "  DC_AGENT_SERVER=nine.testrun.org"

build-webxdc:
	@echo "==> building webxdc"
	cd $(WEBXDC) && npm install
	cd $(WEBXDC) && npm run build
	@test -f $(XDC_OUT) || (echo "missing $(XDC_OUT)" && exit 1)
	@ls -la $(XDC_OUT)

# Copy packed mini app where the agent build.rs can embed it.
embed-xdc: build-webxdc
	@mkdir -p $(AGENT)/assets
	cp -f $(XDC_OUT) $(XDC_EMBED)
	@echo "==> embedded source: $(XDC_EMBED) ($$(wc -c < "$(XDC_EMBED)") bytes)"

# One release binary with webxdc inside — no separate .xdc required at runtime.
build: embed-xdc
	@echo "==> building release agent (embeds webxdc)"
	cd $(AGENT) && cargo build --release
	@mkdir -p $(DIST)
	cp -f $(AGENT)/target/release/terminal_buddy $(DIST)/$(BIN_NAME)
	@chmod +x $(DIST)/$(BIN_NAME)
	@echo "==> $(DIST)/$(BIN_NAME)"
	@$(DIST)/$(BIN_NAME) --version
	@ls -la $(DIST)/$(BIN_NAME)

# Default: do NOT bootstrap from DC_INVITE — user sends via TUI Share tab.
# Rebuilds embed so `cargo run` has the latest mini app inside the binary.
run-agent: embed-xdc
	@bash -c 'set -euo pipefail; \
	  if [ -f "$(ENV_FILE)" ]; then set -a; source "$(ENV_FILE)"; set +a; fi; \
	  unset DC_INVITE DC_FORCE_JOIN DC_BOOTSTRAP_INVITE DC_XDC; \
	  export DC_AGENT_SERVER="$${DC_AGENT_SERVER:-nine.testrun.org}"; \
	  export DC_BOOTSTRAP_INVITE=0; \
	  echo "==> agent (dev) — manual Share tab · embedded xdc"; \
	  echo "    server=$$DC_AGENT_SERVER"; \
	  echo "    invite=manual (tab 4 Share)"; \
	  if [ -n "$${DC_ALLOWED_COMMANDS:-}" ]; then echo "    allowed=$$DC_ALLOWED_COMMANDS"; fi; \
	  EXTRA=(); \
	  if [ "$${DC_USE_TMUX:-0}" = "1" ] || [ "$${DC_USE_TMUX:-}" = "true" ]; then EXTRA+=(--tmux); fi; \
	  if [ -n "$${DC_ALLOWED_COMMANDS:-}" ]; then EXTRA+=(--allowed-commands "$$DC_ALLOWED_COMMANDS"); fi; \
	  cd "$(AGENT)" && cargo run -- serve "$${EXTRA[@]}"'

# Join + send using DC_INVITE from .env (quoted!).
run-agent-inv: embed-xdc
	@bash -c 'set -euo pipefail; \
	  if [ -f "$(ENV_FILE)" ]; then set -a; source "$(ENV_FILE)"; set +a; fi; \
	  unset DC_XDC; \
	  export DC_AGENT_SERVER="$${DC_AGENT_SERVER:-nine.testrun.org}"; \
	  export DC_BOOTSTRAP_INVITE=1; \
	  export DC_FORCE_JOIN="$${DC_FORCE_JOIN:-1}"; \
	  echo "==> agent (dev) — bootstrap from DC_INVITE · embedded xdc"; \
	  echo "    server=$$DC_AGENT_SERVER"; \
	  echo "    force_join=$$DC_FORCE_JOIN"; \
	  if [ -n "$${DC_INVITE:-}" ]; then \
	    echo "    invite=$${DC_INVITE:0:56}…"; \
	  else \
	    echo "    invite=(MISSING — set DC_INVITE in .env)"; \
	    exit 1; \
	  fi; \
	  if [ -n "$${DC_ALLOWED_COMMANDS:-}" ]; then echo "    allowed=$$DC_ALLOWED_COMMANDS"; fi; \
	  EXTRA=(); \
	  if [ "$${DC_USE_TMUX:-0}" = "1" ] || [ "$${DC_USE_TMUX:-}" = "true" ]; then EXTRA+=(--tmux); fi; \
	  if [ -n "$${DC_ALLOWED_COMMANDS:-}" ]; then EXTRA+=(--allowed-commands "$$DC_ALLOWED_COMMANDS"); fi; \
	  cd "$(AGENT)" && cargo run -- serve "$${EXTRA[@]}"'

clean-webxdc:
	rm -rf $(WEBXDC)/dist $(WEBXDC)/terminalbuddy.xdc $(WEBXDC)/remote-xterm.xdc

clean: clean-webxdc
	rm -f $(XDC_EMBED)
	rm -rf $(DIST)
	cd $(AGENT) && cargo clean
