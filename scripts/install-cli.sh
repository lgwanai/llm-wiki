#!/usr/bin/env bash
# llm-wiki CLI Installer (macOS / Linux)
#
# Installs the compiled `wiki` binary to a directory on PATH.
#
# Usage:
#   bash scripts/install-cli.sh [<binary-path>] [--dest <dir>]
#
# Examples:
#   bash scripts/install-cli.sh                           # auto-detect binary
#   bash scripts/install-cli.sh release/macos-arm64/cli/wiki
#   bash scripts/install-cli.sh --dest /usr/local/bin
#   bash scripts/install-cli.sh --version 2.0.0
# ---------------------------------------------------------------------------

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN_NAME="wiki"
DEST_DIR="${LLM_WIKI_BIN_DIR:-$HOME/.local/bin}"
SRC=""
VERSION=""

# ── Helpers ──────────────────────────────────────────────────────────────
green() { printf "\033[32m%s\033[0m\n" "$1"; }
yellow() { printf "\033[33m%s\033[0m\n" "$1"; }
red() { printf "\033[31m%s\033[0m\n" "$1" >&2; }

# ── Parse args ───────────────────────────────────────────────────────────
PASSED_SRC=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dest)
      DEST_DIR="$2"
      shift 2
      ;;
    --version|-v)
      VERSION="$2"
      shift 2
      ;;
    --help|-h)
      sed -n '2,14p' "$0" | sed 's/^# //'
      exit 0
      ;;
    -*)
      echo "Unknown flag: $1"
      exit 1
      ;;
    *)
      PASSED_SRC="$1"
      shift
      ;;
  esac
done

# ── Locate binary ────────────────────────────────────────────────────────
if [[ -n "$PASSED_SRC" ]]; then
  SRC="$PASSED_SRC"
elif [[ -n "$VERSION" ]]; then
  # Look for a specific version in release dir
  for tag in macos-arm64 macos-x64 linux-x64 linux-arm64; do
    candidate="$ROOT_DIR/release/$tag/cli/$BIN_NAME"
    if [[ -f "$candidate" ]]; then
      SRC="$candidate"
      break
    fi
  done
fi

# Fallback: search common locations
if [[ -z "$SRC" ]]; then
  if [[ -f "$ROOT_DIR/release/cli/$BIN_NAME" ]]; then
    SRC="$ROOT_DIR/release/cli/$BIN_NAME"
  elif [[ -f "$ROOT_DIR/target/release/$BIN_NAME" ]]; then
    SRC="$ROOT_DIR/target/release/$BIN_NAME"
  else
    # Try platform-specific release directory
    case "$(uname -s)" in
      Darwin)
        ARCH="$(uname -m)"
        [[ "$ARCH" == "arm64" ]] && TAG="macos-arm64" || TAG="macos-x64"
        ;;
      Linux)
        ARCH="$(uname -m)"
        [[ "$ARCH" == "aarch64" ]] && TAG="linux-arm64" || TAG="linux-x64"
        ;;
      *) TAG="" ;;
    esac
    if [[ -n "$TAG" ]]; then
      candidate="$ROOT_DIR/release/$TAG/cli/$BIN_NAME"
      [[ -f "$candidate" ]] && SRC="$candidate"
    fi
  fi
fi

if [[ -z "$SRC" ]] || [[ ! -f "$SRC" ]]; then
  red "CLI binary not found."
  red ""
  red "Build the CLI first:"
  red "  cargo build --release -p llm-wiki-cli"
  red "  # or"
  red "  bash scripts/build-cli.sh"
  red ""
  red "Or pass the binary path:"
  red "  bash scripts/install-cli.sh /path/to/wiki"
  exit 1
fi

# ── Show what we found ──────────────────────────────────────────────────
echo "──────────────────────────────────────────────────────────────"
echo "  llm-wiki CLI Installer"
echo "──────────────────────────────────────────────────────────────"
echo "  Binary:     $SRC"
echo "  Size:       $(du -h "$SRC" | cut -f1)"
echo "  Destination: $DEST_DIR/$BIN_NAME"
echo ""

# ── Install ──────────────────────────────────────────────────────────────
mkdir -p "$DEST_DIR"

if [[ -f "$DEST_DIR/$BIN_NAME" ]]; then
  echo "  Existing binary found — backing up to $DEST_DIR/$BIN_NAME.bak"
  cp "$DEST_DIR/$BIN_NAME" "$DEST_DIR/$BIN_NAME.bak"
fi

cp "$SRC" "$DEST_DIR/$BIN_NAME"
chmod +x "$DEST_DIR/$BIN_NAME"

green "  ✓ Installed wiki CLI to $DEST_DIR/$BIN_NAME"

# ── Verify ────────────────────────────────────────────────────────────────
if command -v "$BIN_NAME" >/dev/null 2>&1; then
  green "  ✓ wiki is on PATH ($(command -v wiki))"
else
  yellow "  ⚠ wiki is not on PATH."
  echo ""
  echo "  Add the following to your shell profile (~/.zshrc, ~/.bashrc, etc.):"
  echo ""
  echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
  echo ""
  echo "  Then reload:"
  echo "    source ~/.zshrc   # or ~/.bashrc"
  echo ""
  echo "  Or add a symlink to a directory already on PATH:"
  echo "    sudo ln -sf $DEST_DIR/$BIN_NAME /usr/local/bin/$BIN_NAME"
fi

# ── Quick test ───────────────────────────────────────────────────────────
echo ""
echo "  Run 'wiki --help' to verify the installation."
echo ""

# ── Configuration reminder ───────────────────────────────────────────────
CONFIG_FILE="$HOME/.config/llm-wiki/wiki_config.yaml"
if [[ ! -f "$CONFIG_FILE" ]]; then
  yellow "  ⚠ No configuration file found at $CONFIG_FILE"
  echo "  Create one from the example:"
  echo "    mkdir -p ~/.config/llm-wiki"
  echo "    cp wiki_config.yaml.example ~/.config/llm-wiki/wiki_config.yaml"
  echo "    # Then edit with your API keys"
  echo ""
fi
