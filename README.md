<p align="center">
  <img src="src-tauri/icons/icon.png" width="96" alt="llm-wiki" />
</p>

# llm-wiki

**Personal Knowledge Base powered by LLMs** — 100% Rust.

Compile documents into structured wiki pages with knowledge graph linking.
Cross-platform desktop app (macOS/Windows/Linux) with Codex-themed UI.

## Features

- **📄 Document Compilation** — LLM analyzes markdown, PDF, images, code → structured wiki pages
- **🔍 Hybrid Search** — BM25 + metadata + knowledge graph, fused with Reciprocal Rank Fusion
- **📊 Knowledge Graph** — Auto-extracted entities & relationships, interactive 3D visualization
- **🧮 Ledger/台账** — DuckDB-backed structured tables, NL→SQL queries, CSV/JSON/Excel import
- **💬 Chat** — Natural language Q&A over your wiki with source citations
- **⚡ liteparse** — Native Rust PDF text extraction (OCR optional)
- **🎨 Codex UI** — Dark theme, terminal green accents, JetBrains Mono, split panels

## Screenshots

| Graph View | Chat & Search | Settings |
|:---:|:---:|:---:|
| ![Graph](snapshot/graph-view.png) | ![Chat](snapshot/chat-view.png) | ![Settings](snapshot/settings-window.png) |

---

## Installation

Choose the package that matches your platform.

### Desktop App

#### macOS

| Arch | Download |
|------|----------|
| Apple Silicon (M1/M2/M3/M4) | `llm-wiki_*_aarch64.dmg` |
| Intel | `llm-wiki_*_x64.dmg` |

1. Download the `.dmg` for your architecture
2. Open the DMG and drag **llm-wiki.app** to **Applications**
3. On first launch, right-click → **Open** to bypass Gatekeeper (or allow in **System Settings → Privacy & Security**)

```bash
# Remove quarantine attribute if needed
xattr -d com.apple.quarantine /Applications/llm-wiki.app
```

**Requires:** macOS 10.15 (Catalina) or later.

#### Windows

Download `llm-wiki_*_x64-setup.exe` or `llm-wiki_*_x64.msi`.

1. Run the installer
2. If SmartScreen warns, click **More info → Run anyway**
3. Launch **llm-wiki** from the Start Menu

**Requires:** Windows 10 or later (x86_64).

#### Linux

| Format | Command |
|--------|---------|
| `.deb` | `sudo dpkg -i llm-wiki_*_amd64.deb` |
| `.AppImage` | `chmod +x llm-wiki_*_amd64.AppImage && ./llm-wiki_*_amd64.AppImage` |

The `.deb` installs a desktop entry — launch from your app launcher.
The `.AppImage` runs portably; move it anywhere and execute it.

**Requires:** glibc ≥ 2.31 (Ubuntu 20.04+, Debian 11+, Fedora 35+).

### CLI (`wiki`)

The CLI is a single compiled binary — no dependencies, no runtime required.

#### macOS / Linux

```bash
# Option 1: One-line install (downloads latest from releases)
curl -fsSL https://github.com/llm-wiki/llm-wiki-rust/releases/latest/download/wiki-$(uname -s)-$(uname -m) \
  -o ~/.local/bin/wiki && chmod +x ~/.local/bin/wiki

# Option 2: From a release package
bash scripts/install-cli.sh release/macos-arm64/cli/wiki

# Option 3: Manual install
mkdir -p ~/.local/bin
cp release/<platform>/cli/wiki ~/.local/bin/wiki
chmod +x ~/.local/bin/wiki
```

Ensure `~/.local/bin` is on PATH:

```bash
# Add to ~/.zshrc or ~/.bashrc
export PATH="$HOME/.local/bin:$PATH"
```

#### Windows

```powershell
# Option 1: From a release package
powershell -ExecutionPolicy Bypass -File scripts/install-cli.ps1 -Source "release\windows-x64\cli\wiki.exe"

# Option 2: Manual install
mkdir -Force "$env:LOCALAPPDATA\llm-wiki\bin"
cp release\windows-x64\cli\wiki.exe "$env:LOCALAPPDATA\llm-wiki\bin\"
[Environment]::SetEnvironmentVariable("PATH", "$env:PATH;$env:LOCALAPPDATA\llm-wiki\bin", "User")
```

#### Verify CLI

```bash
wiki --help
wiki config --check
```

If the command is not found, restart your terminal or re-open your shell.

### Claude Code Skill

The `llm-wiki-cli` skill lets Claude Code operate your wiki through the CLI.

**Prerequisite:** The `wiki` CLI must be installed first. Verify:

```bash
wiki --version
```

Then install the skill:

**macOS / Linux**

```bash
# From a release package
unzip release/<platform>/skill/llm-wiki-cli.zip -d ~/.claude/skills/llm-wiki-cli

# Or copy the raw directory
cp -r skills/llm-wiki-cli ~/.claude/skills/

# For development — symlink
ln -s "$(pwd)/skills/llm-wiki-cli" ~/.claude/skills/llm-wiki-cli
```

**Windows**

```powershell
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.claude\skills"
Expand-Archive -Path release\windows-x64\skill\llm-wiki-cli.zip -DestinationPath "$env:USERPROFILE\.claude\skills\llm-wiki-cli"
```

After installing, restart Claude Code. The skill will appear as `/llm-wiki-cli`.

See [skills/llm-wiki-cli/INSTALL.md](skills/llm-wiki-cli/INSTALL.md) for troubleshooting.

---

## Configuration

Create `~/.config/llm-wiki/wiki_config.yaml`:

```yaml
model:
  provider: deepseek
  api_key: "sk-your-key"
  model: deepseek-v4-flash
  temperature: 0.3

liteparse:
  ocr_server_url: ""     # Optional OCR endpoint
  ocr_language: chi_sim+eng
  ocr_enabled: false

query:
  max_results: 5
  llm_synthesis: true
```

The CLI and desktop app use the same config discovery order:

1. `LLM_WIKI_CONFIG` environment variable (path to YAML file)
2. Nearest project `wiki_config.yaml` (searched upward from CWD)
3. `~/.config/llm-wiki/wiki_config.yaml`

Set values from the command line:

```bash
wiki config --set model.provider=openai
wiki config --set liteparse.ocr_enabled=true
wiki config --check
```

Or use the GUI: **⌘,** → Settings window.

See [wiki_config.yaml.example](wiki_config.yaml.example) for all options.

---

## Quick Start

```bash
# 1. Initialize a wiki
wiki init

# 2. Import a document
wiki compile document.md

# 3. Compile a PDF with OCR
wiki config --set liteparse.ocr_enabled=true --set liteparse.ocr_language=chi_sim+eng
wiki compile report.pdf --source-type doc

# 4. Query your knowledge base
wiki query "What is DeepSeek?"

# 5. Check wiki health
wiki status
wiki lint
```

---

## Architecture

```
llm-wiki-rust/
├── core/          # Shared library (config, search, compile, graph, ledger, llm)
├── cli/           # CLI binary (wiki)
├── src-tauri/     # Tauri desktop app (menu, commands, window management)
├── src/           # React frontend (Codex UI, chat, graph, markdown viewer)
├── skills/        # Claude Code skill packaging
├── scripts/       # Build, install, and packaging scripts
└── Cargo.toml     # Workspace root
```

| Layer | Tech |
|-------|------|
| Desktop | Tauri 2 (Rust) |
| Frontend | React 19 + TypeScript + Vite |
| CSS | Tailwind v4 + custom Codex theme |
| Database | DuckDB (bundled) |
| PDF | liteparse (native) + OCR API (optional) |
| Search | BM25 + jieba-rs + RRF fusion |
| Graph | Canvas 2D force-directed |

## Commands

| Command | Description |
|---------|-------------|
| `wiki init` | Initialize wiki directory structure |
| `wiki compile <file>` | Compile source → wiki pages |
| `wiki query "..."` | Search + LLM synthesis |
| `wiki search doctor` | Diagnose search index |
| `wiki lint` | Health check + auto-heal |
| `wiki ledger create/insert/show` | DuckDB table management |
| `wiki config` | Show configuration |
| `wiki status` | Wiki dashboard |

---

## Development

### Prerequisites

- **Rust** 1.80+ ([rustup](https://rustup.rs))
- **Node.js** 20+ and npm
- **Tauri CLI**: `cargo install tauri-cli --version "^2"`
- **Platform toolkits**:
  - macOS: Xcode Command Line Tools (`xcode-select --install`)
  - Windows: Microsoft Visual Studio C++ Build Tools
  - Linux: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev`

#### Cross-compilation toolchains (optional)

| Target | Toolchain | Install |
|--------|-----------|---------|
| Windows x64 | `mingw-w64` | `brew install mingw-w64` (macOS) / `apt install mingw-w64` (Linux) |
| Linux x64/arm64 | `zig` + `cargo-zigbuild` | `brew install zig && cargo install cargo-zigbuild` |
| macOS x64 (from arm64) | Built-in | `rustup target add x86_64-apple-darwin` |

### Setup

```bash
git clone https://github.com/llm-wiki/llm-wiki-rust.git
cd llm-wiki-rust
npm install
```

### Development Loop

```bash
# Run tests
cargo test --workspace

# Dev mode (hot reload)
npm run tauri dev

# CLI-only dev
cargo run -- -p llm-wiki-cli -- status
```

### Building from Source

```bash
# ── CLI only ──────────────────────────
# Host platform
cargo build --release -p llm-wiki-cli
# Cross-compile (requires rustup target)
bash scripts/build-cli.sh --target linux-x64

# ── CLI + Desktop + Skill (host platform) ──
npm run package:all

# ── Desktop only ──
npm run package:desktop

# ── Skill only ──
npm run package:skill

# ── Everything, all targets ──
npm run package:release
```

### Build Script Reference

| Script | Purpose |
|--------|---------|
| `scripts/package.mjs` | Full multi-target orchestrator (CLI + desktop + skill) |
| `scripts/build-cli.sh` | Cross-compile CLI for any target |
| `scripts/package-skill.sh` | Package Claude Code skill as `.zip` |
| `scripts/install-cli.sh` | Install CLI binary on macOS/Linux |
| `scripts/install-cli.ps1` | Install CLI binary on Windows |

```bash
# Package a single target
node scripts/package.mjs --target macos-arm64

# Package all targets (installs cross-compilation targets)
node scripts/package.mjs --target all --setup

# Dry-run — see what would happen
node scripts/package.mjs --target all --dry-run

# CLI-only, all targets
bash scripts/build-cli.sh --all

# Cross-compile CLI for Linux
bash scripts/build-cli.sh --target linux-x64 --out /tmp/wiki-release
```

### Cross-Platform Build Notes

| Build | From macOS | From Windows | From Linux |
|-------|-----------|--------------|------------|
| CLI macOS arm64/x64 | ✅ Both via `--target` | ❌ (needs macOS SDK) | ❌ |
| CLI Windows x64 | ✅ via `mingw-w64` | ✅ Native | ✅ via `mingw-w64` |
| CLI Linux x64/arm64 | ✅ via `cargo-zigbuild` | ✅ via `cargo-zigbuild` | ✅ Native |
| Desktop (Tauri) | ✅ macOS only | ✅ Windows only | ✅ Linux only |

**CLI cross-compilation works from any platform** with the right toolchain. Desktop apps require the native OS for bundling — use **GitHub Actions** for full multi-platform desktop releases (template below).

<details>
<summary><b>GitHub Actions CI template</b></summary>

```yaml
# .github/workflows/release.yml
name: Release
on:
  push:
    tags: ["v*"]

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: macos-latest
            target: macos-arm64
            rust-target: aarch64-apple-darwin
          - os: macos-13
            target: macos-x64
            rust-target: x86_64-apple-darwin
          - os: windows-latest
            target: windows-x64
            rust-target: x86_64-pc-windows-msvc
          - os: ubuntu-latest
            target: linux-x64
            rust-target: x86_64-unknown-linux-gnu

    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: "22" }
      - run: npm ci
      - run: rustup target add ${{ matrix.rust-target }}
      - run: node scripts/package.mjs --target ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.target }}
          path: release/${{ matrix.target }}/
```
</details>

---

## Upgrading

### Desktop App

Download the latest release for your platform and replace the existing install.

- **macOS**: Drag new `.app` to Applications (replaces old version)
- **Windows**: Run the new installer (upgrades in-place)
- **Linux**: `sudo dpkg -i llm-wiki_*_amd64.deb` (upgrades automatically)

### CLI

```bash
# Download the latest binary
curl -fsSL https://github.com/llm-wiki/llm-wiki-rust/releases/latest/download/wiki-$(uname -s)-$(uname -m) \
  -o ~/.local/bin/wiki && chmod +x ~/.local/bin/wiki

# Verify version
wiki --version
```

### Claude Code Skill

```bash
rm -rf ~/.claude/skills/llm-wiki-cli
unzip llm-wiki-cli.zip -d ~/.claude/skills/llm-wiki-cli
# Restart Claude Code
```

Configuration files are forward-compatible — no migration needed between minor versions.

---

## Troubleshooting

### `wiki: command not found`

Ensure the install directory is on PATH. Add to your shell profile and restart the terminal:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

### Desktop app won't open (macOS)

The app may be quarantined. Remove the quarantine attribute:

```bash
xattr -d com.apple.quarantine /Applications/llm-wiki.app
```

Or allow it in **System Settings → Privacy & Security**.

### Desktop app won't open (Windows)

SmartScreen may block unsigned binaries. Click **More info → Run anyway**.

### Configuration not found

The config file must be at one of:

- `LLM_WIKI_CONFIG` env var (pointing to a YAML file)
- `wiki_config.yaml` in the current or parent directory
- `~/.config/llm-wiki/wiki_config.yaml`

Run `wiki config --check` to validate.

### OCR not working

Ensure OCR is enabled and a local engine is configured:

```bash
wiki config --set liteparse.ocr_enabled=true --set liteparse.ocr_language=chi_sim+eng
wiki config --check | grep ocr
```

For local OCR, configure the engine and model root (see [wiki_config.yaml.example](wiki_config.yaml.example)).

### Search returns no results

Run a search health check:

```bash
wiki search doctor
```

This re-indexes all pages and reports index status.

### `cargo: command not found`

Install Rust via [rustup](https://rustup.rs):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

---

## License

Apache 2.0
