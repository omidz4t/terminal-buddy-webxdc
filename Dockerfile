# TerminalBuddy — runtime image (Debian 13).
#
# The host binary is built in CI (`make build`) and copied in here.
# Do NOT recompile Rust in this image — that is intentionally heavy and already done.
#
# Build (after `make build` or with CI artifact in dist/):
#   docker build -t terminalbuddy:local .
#
# Run TUI:
#   docker run --rm -it -v tb-data:/data ghcr.io/omidz4t/terminal-buddy-webxdc:latest serve
#
# Headless:
#   docker run --rm -v tb-data:/data … serve --no-tui --invite '…' --allowed-commands ls,pwd

FROM debian:13-slim

LABEL org.opencontainers.image.title="TerminalBuddy" \
      org.opencontainers.image.description="Shared terminal over Delta Chat (embedded webxdc)" \
      org.opencontainers.image.source="https://github.com/omidz4t/terminal-buddy-webxdc" \
      org.opencontainers.image.licenses="GPL-3.0-or-later"

# Runtime deps for the agent binary + optional tmux YOLO mode.
# Debian 13 (trixie): prefer libssl3t64, fall back to libssl3.
RUN set -eux; \
    apt-get update; \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates bash tmux zlib1g libgcc-s1 libstdc++6; \
    if apt-cache show libssl3t64 >/dev/null 2>&1; then \
      DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends libssl3t64; \
    else \
      DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends libssl3; \
    fi; \
    rm -rf /var/lib/apt/lists/*

# Persistent agent identity / SQLite live under /data (.dc-agent when cwd=/data).
WORKDIR /data
VOLUME ["/data"]

# Pre-built release binary (with embedded terminalbuddy.xdc) from CI / `make build`.
# Expected path in build context: dist/terminalbuddy
COPY dist/terminalbuddy /usr/local/bin/terminalbuddy
RUN chmod 755 /usr/local/bin/terminalbuddy \
 && /usr/local/bin/terminalbuddy --version

ENTRYPOINT ["/usr/local/bin/terminalbuddy"]
# Default keeps a TTY-friendly serve; pass --no-tui in CI/servers without -t.
CMD ["serve"]
