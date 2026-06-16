#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN_NAME="wiki"
SRC="${1:-$ROOT_DIR/release/cli/$BIN_NAME}"
DEST_DIR="${LLM_WIKI_BIN_DIR:-$HOME/.local/bin}"

if [[ ! -f "$SRC" ]]; then
  if [[ -f "$ROOT_DIR/target/release/$BIN_NAME" ]]; then
    SRC="$ROOT_DIR/target/release/$BIN_NAME"
  else
    echo "CLI binary not found. Expected $SRC" >&2
    echo "Build/package first, or pass the binary path as the first argument." >&2
    exit 1
  fi
fi

mkdir -p "$DEST_DIR"
cp "$SRC" "$DEST_DIR/wiki"
chmod +x "$DEST_DIR/wiki"

echo "Installed wiki CLI to $DEST_DIR/wiki"
if ! command -v wiki >/dev/null 2>&1; then
  echo "Add this directory to PATH if needed:"
  echo "  export PATH=\"$DEST_DIR:\$PATH\""
fi
