#!/usr/bin/env bash
# llm-wiki Claude Code Skill Packager
#
# Packages the skills/llm-wiki-cli/ directory into a distributable zip
# and copies the raw directory for direct installation.
#
# Usage:
#   bash scripts/package-skill.sh [--out <dir>]
#
# Output:
#   <out>/llm-wiki-cli.zip     — compressed skill package
#   <out>/llm-wiki-cli/        — raw skill directory for direct use
# ---------------------------------------------------------------------------

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
SKILL_SRC="$ROOT_DIR/skills/llm-wiki-cli"
RELEASE_DIR="${RELEASE_DIR:-$ROOT_DIR/release}"
OUT_DIR=""

# ── Parse args ───────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --out)
      OUT_DIR="$2"
      shift 2
      ;;
    --help|-h)
      sed -n '2,14p' "$0" | sed 's/^# //'
      exit 0
      ;;
    *)
      echo "Unknown flag: $1"
      exit 1
      ;;
  esac
done

if [[ -z "$OUT_DIR" ]]; then
  # Determine current platform tag for default output
  case "$(uname -s)" in
    Darwin)
      ARCH="$(uname -m)"
      if [[ "$ARCH" == "arm64" ]]; then
        TAG="macos-arm64"
      else
        TAG="macos-x64"
      fi
      ;;
    Linux)
      ARCH="$(uname -m)"
      if [[ "$ARCH" == "aarch64" ]]; then
        TAG="linux-arm64"
      else
        TAG="linux-x64"
      fi
      ;;
    MINGW*|MSYS*|CYGWIN*)
      TAG="windows-x64"
      ;;
    *)
      TAG="unknown"
      ;;
  esac
  OUT_DIR="$RELEASE_DIR/$TAG/skill"
fi

# ── Verify skill source ──────────────────────────────────────────────────
if [[ ! -f "$SKILL_SRC/SKILL.md" ]]; then
  echo "ERROR: Skill source not found at $SKILL_SRC" >&2
  echo "Expected: $SKILL_SRC/SKILL.md" >&2
  exit 1
fi

# ── Package ──────────────────────────────────────────────────────────────
mkdir -p "$OUT_DIR"

echo "Packaging skill from: $SKILL_SRC"
echo "Output:              $OUT_DIR"

# Create zip
ZIP_FILE="$OUT_DIR/llm-wiki-cli.zip"
rm -f "$ZIP_FILE"
(cd "$SKILL_SRC" && zip -r -q "$ZIP_FILE" . -x '*.DS_Store' -x '__MACOSX/*')
echo "  → $ZIP_FILE"

# Also copy raw directory
RAW_DIR="$OUT_DIR/llm-wiki-cli"
rm -rf "$RAW_DIR"
cp -r "$SKILL_SRC" "$RAW_DIR"
echo "  → $RAW_DIR (raw copy)"

echo ""
echo "Done. Skill packaged for distribution."
echo ""
echo "Users can install with:"
echo "  unzip llm-wiki-cli.zip -d ~/.claude/skills/llm-wiki-cli"
echo "  # or"
echo "  cp -r llm-wiki-cli ~/.claude/skills/"
echo ""
