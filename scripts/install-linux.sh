#!/usr/bin/env bash
# One-shot installer: download the latest Linux release binary and run `serve`.
# Usage:
#   wget -qO- https://raw.githubusercontent.com/omidz4t/terminal-buddy-webxdc/main/scripts/install-linux.sh | bash
set -euo pipefail

REPO="${TERMINALBUDDY_REPO:-omidz4t/terminal-buddy-webxdc}"
ASSET="${TERMINALBUDDY_ASSET:-terminalbuddy-linux-x86_64}"
URL="${TERMINALBUDDY_URL:-https://github.com/${REPO}/releases/latest/download/${ASSET}}"
OUT="${TERMINALBUDDY_BIN:-$PWD/terminalbuddy}"
RUN_SERVE="${TERMINALBUDDY_RUN:-1}"

echo "==> TerminalBuddy install"
echo "    url=$URL"
echo "    out=$OUT"

if ! command -v wget >/dev/null 2>&1; then
  echo "need wget" >&2
  exit 1
fi

wget -qO "$OUT" "$URL"
chmod +x "$OUT"
"$OUT" --version || true

if [ "$RUN_SERVE" = "1" ]; then
  echo "==> starting: $OUT serve"
  exec "$OUT" serve "$@"
else
  echo "==> installed: $OUT"
  echo "    run: $OUT serve"
fi
