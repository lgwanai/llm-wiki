#!/usr/bin/env bash
# llm-wiki CLI Cross-Compilation Script
#
# Builds the `wiki` CLI binary for one or more target platforms.
#
# Usage:
#   bash scripts/build-cli.sh [--target <tag>] [--release] [--all]
#
# Target tags:
#   macos-arm64        macOS Apple Silicon    (aarch64-apple-darwin)
#   macos-x64          macOS Intel            (x86_64-apple-darwin)
#   windows-x64        Windows x86_64         (x86_64-pc-windows-msvc)
#   linux-x64          Linux x86_64           (x86_64-unknown-linux-gnu)
#   linux-arm64        Linux ARM64            (aarch64-unknown-linux-gnu)
#
# Examples:
#   bash scripts/build-cli.sh                              # build for host only
#   bash scripts/build-cli.sh --all                        # build for all targets
#   bash scripts/build-cli.sh --target linux-x64
#   bash scripts/build-cli.sh --all --debug --out /tmp/release
# ---------------------------------------------------------------------------

set -eo pipefail
# NOTE: do NOT set -u — associative-array keys with hyphens trigger arithmetic
# evaluation in some bash versions, causing "unbound variable" on valid keys.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RELEASE_DIR="${RELEASE_DIR:-$ROOT_DIR/release}"

# ── Target lookup (case-based — avoids associative-array hyphen bug) ─────
rust_target() {
  case "${1:-}" in
    macos-arm64)   echo "aarch64-apple-darwin" ;;
    macos-x64)     echo "x86_64-apple-darwin" ;;
    windows-x64)   echo "x86_64-pc-windows-gnu" ;;
    linux-x64)     echo "x86_64-unknown-linux-gnu" ;;
    linux-arm64)   echo "aarch64-unknown-linux-gnu" ;;
    *)             echo "" ;;
  esac
}

cli_name() {
  case "${1:-}" in
    *windows*) echo "wiki.exe" ;;
    *)         echo "wiki" ;;
  esac
}

ALL_TARGETS=(macos-arm64 macos-x64 windows-x64 linux-x64 linux-arm64)

# ── Defaults ─────────────────────────────────────────────────────────────
BUILD_ALL=false
RELEASE_FLAG="--release"
EXTRA_CARGO_FLAGS=""
OUT_DIR=""
declare -a TARGETS=()

# ── Parse args ───────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      TARGETS+=("$2")
      shift 2
      ;;
    --all)
      BUILD_ALL=true
      shift
      ;;
    --release)
      RELEASE_FLAG="--release"
      shift
      ;;
    --debug)
      RELEASE_FLAG=""
      shift
      ;;
    --out)
      OUT_DIR="$2"
      shift 2
      ;;
    --features)
      EXTRA_CARGO_FLAGS="$EXTRA_CARGO_FLAGS --features $2"
      shift 2
      ;;
    --help|-h)
      sed -n '2,22p' "$0" | sed 's/^# //'
      exit 0
      ;;
    *)
      echo "Unknown flag: $1"
      exit 1
      ;;
  esac
done

# ── Resolve targets ──────────────────────────────────────────────────────
if $BUILD_ALL; then
  TARGETS=("${ALL_TARGETS[@]}")
elif [[ ${#TARGETS[@]} -eq 0 ]]; then
  # Auto-detect host
  HOST_TRIPLE="$(rustc -vV 2>/dev/null | awk '/host:/ {print $2}')"
  case "$HOST_TRIPLE" in
    aarch64-apple-darwin)      TARGETS=(macos-arm64) ;;
    x86_64-apple-darwin)       TARGETS=(macos-x64) ;;
    x86_64-pc-windows-msvc)    TARGETS=(windows-x64) ;;
    x86_64-unknown-linux-gnu)  TARGETS=(linux-x64) ;;
    aarch64-unknown-linux-gnu) TARGETS=(linux-arm64) ;;
    *)
      echo "Unknown host target: $HOST_TRIPLE"
      echo "Known targets: ${ALL_TARGETS[*]}"
      exit 1
      ;;
  esac
fi

# Validate all targets
for TAG in "${TARGETS[@]}"; do
  if [[ -z "$(rust_target "$TAG")" ]]; then
    echo "Unknown target tag: $TAG"
    echo "Valid tags: ${ALL_TARGETS[*]}"
    exit 1
  fi
done

# ── Helpers ──────────────────────────────────────────────────────────────
green() { printf "\033[32m%s\033[0m\n" "$1"; }
yellow() { printf "\033[33m%s\033[0m\n" "$1"; }
red() { printf "\033[31m%s\033[0m\n" "$1" >&2; }

section() {
  printf "\n%.0s" {1..60}
  printf "\n  %s\n" "$1"
  printf "%.0s" {1..60}
  printf "\n\n"
}

has_target() {
  rustup target list --installed 2>/dev/null | grep -qF "$1"
}

install_target() {
  if has_target "$1"; then
    echo "  Rust target $1 already installed"
  else
    echo "  Installing Rust target: $1"
    rustup target add "$1"
  fi
}

# ── Main ─────────────────────────────────────────────────────────────────
section "llm-wiki CLI Build"
echo "  Targets: ${TARGETS[*]}"
echo "  Release: ${RELEASE_FLAG:-debug}"
echo "  Output:  ${OUT_DIR:-$RELEASE_DIR/<target>/cli/}"
echo ""

for TAG in "${TARGETS[@]}"; do
  RUST_TARGET="$(rust_target "$TAG")"
  BIN_NAME="$(cli_name "$RUST_TARGET")"

  section "Building CLI: $TAG ($RUST_TARGET)"

  # Install Rust target if cross-compiling
  install_target "$RUST_TARGET"

  # Build with cross-compilation env vars if needed
  if [[ "$RUST_TARGET" == *windows* ]]; then
    export USERPROFILE="${USERPROFILE:-/tmp}"
  fi

  # Use cargo-zigbuild for Linux targets (needs zig as cross-linker)
  if [[ "$RUST_TARGET" == *linux* ]]; then
    green "  cargo zigbuild $RELEASE_FLAG -p llm-wiki-cli --target $RUST_TARGET $EXTRA_CARGO_FLAGS"
    cargo zigbuild $RELEASE_FLAG -p llm-wiki-cli --target "$RUST_TARGET" $EXTRA_CARGO_FLAGS
  else
    green "  cargo build $RELEASE_FLAG -p llm-wiki-cli --target $RUST_TARGET $EXTRA_CARGO_FLAGS"
    cargo build $RELEASE_FLAG -p llm-wiki-cli --target "$RUST_TARGET" $EXTRA_CARGO_FLAGS
  fi

  # Determine source and destination
  PROFILE_DIR="release"
  if [[ -z "$RELEASE_FLAG" ]]; then
    PROFILE_DIR="debug"
  fi

  SRC="$ROOT_DIR/target/$RUST_TARGET/$PROFILE_DIR/$BIN_NAME"
  if [[ ! -f "$SRC" ]]; then
    red "  ERROR: Binary not found at $SRC"
    continue
  fi

  if [[ -n "$OUT_DIR" ]]; then
    DEST_DIR="$OUT_DIR"
  else
    DEST_DIR="$RELEASE_DIR/$TAG/cli"
  fi
  mkdir -p "$DEST_DIR"

  cp "$SRC" "$DEST_DIR/$BIN_NAME"
  if [[ "$BIN_NAME" != "wiki.exe" ]]; then
    chmod +x "$DEST_DIR/$BIN_NAME"
  fi

  green "  → $DEST_DIR/$BIN_NAME"
  green "  Size: $(du -h "$DEST_DIR/$BIN_NAME" | cut -f1)"
done

echo ""
section "Done"
echo "  Built ${#TARGETS[@]} target(s)"
echo "  Artifacts under: ${OUT_DIR:-$RELEASE_DIR}"
echo ""
