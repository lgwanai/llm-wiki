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

## Quick Start

### Install

```bash
# Download the release package for your OS, then install the desktop app.
# CLI binaries are included under release/cli/ as wiki or wiki.exe.
wiki config --check
```

Maintainers can build platform packages with `npm run package:all`.

### CLI

```bash
# Download/extract a release package, then install the compiled binary
scripts/install-cli.sh

# Or copy release/cli/wiki to any directory on PATH
cp release/cli/wiki ~/.local/bin/wiki
chmod +x ~/.local/bin/wiki

# Initialize a wiki
wiki init

# Compile a document
wiki compile document.md

# Query your knowledge base
wiki query "What is DeepSeek?"

# Full list of commands
wiki --help
```

Users do not need Rust or Cargo to use the CLI. The CLI is a single compiled
`wiki` binary and reads configuration from `LLM_WIKI_CONFIG`, a project
`wiki_config.yaml`, or `~/.config/llm-wiki/wiki_config.yaml`.

### Configuration

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

Or use the GUI: **⌘,** → Settings window.

## Architecture

```
llm-wiki-rust/
├── core/          # Shared library (config, search, compile, graph, ledger, llm)
├── cli/           # CLI binary (wiki)
├── src-tauri/     # Tauri desktop app (menu, commands, window management)
├── src/           # React frontend (Codex UI, chat, graph, markdown viewer)
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

## Development

```bash
# Run tests
cargo test --workspace

# Dev mode (hot reload)
npm run tauri dev

# Release build
npm run package:all
```

## License

Apache 2.0
