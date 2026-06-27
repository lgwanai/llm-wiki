---
name: llm-wiki-cli
description: Operate the llm-wiki Rust CLI for personal knowledge base workflows. Use when Codex needs to configure llm-wiki, set OCR/local model options, initialize a wiki, compile files or directories, query the wiki, inspect status, run lint, or package the CLI/desktop app from this repository.
---

# LLM Wiki CLI

Use this skill to run the compiled `wiki` CLI and keep desktop and CLI configuration synchronized. Do not require users to install Rust or run `cargo` unless they are explicitly building from source.

When compiling sources from this skill, default to Agent-powered compile mode. Do not call `wiki compile` unless the source is structured table data or the user explicitly asks to use the CLI/app configured model. Agent-powered compile keeps skill usage independent from llm-wiki model configuration, while app and normal CLI usage still require a configured LLM. Structured table files (`.csv`, `.tsv`, `.json`, `.xlsx`, `.xls`) are the exception: `wiki compile` imports them directly into Tables and does not call an LLM.

Before Agent-powered compile, read and follow the bundled local standards:

- [COMPILE_SPEC.md](COMPILE_SPEC.md) — skill Agent compile workflow, output template, and retrieval requirements.
- [SCHEMA.md](SCHEMA.md) — wiki schema, entity types, relationship types, ingest rules, quality standards, and privacy rules.

The `wiki compile-prompt` output also embeds the active wiki schema. Treat these bundled files as the skill-side reference and the emitted prompt as the per-run source of truth.

## Prerequisites

Before using this skill, verify the `wiki` CLI is installed:

```bash
wiki --version
```

If not found, direct the user to install the CLI binary first. The skill
will not function without it. See the project README or
[INSTALL.md](INSTALL.md) for per-platform instructions.

## Command Location

Preferred user entrypoint:

```bash
wiki <command>
```

If the binary is not on PATH, use the release artifact directly:

```bash
./release/cli/wiki <command>
```

## Shared Configuration

The CLI and desktop app use the same config discovery order:

1. `LLM_WIKI_CONFIG`
2. nearest project `wiki_config.yaml`
3. `~/.config/llm-wiki/wiki_config.yaml`

Use dotted keys for scriptable changes:

```bash
wiki config --set liteparse.ocr_enabled=true
wiki config --set ocr.engine=paddleocr-vl --set ocr.model_root=/path/to/ocr-models --set ocr.model=PaddleOCR-VL-1.5-8bit --set ocr.device=auto
```

Validate after edits:

```bash
wiki config --check
```

## OCR Defaults

Use a local OCR model through a configured model root. The CLI and desktop app both read the same config file, so changes made with `wiki config --set ...` immediately apply to both.

PaddleOCR-VL uses the `Spotting:` task and parses `<|LOC_n|>` tokens into polygons and bounding boxes. Enable it with:

```bash
wiki config \
  --set liteparse.ocr_enabled=true \
  --set liteparse.ocr_language=chi_sim+eng \
  --set ocr.engine=paddleocr-vl \
  --set ocr.model_root=/path/to/ocr-models \
  --set ocr.model=PaddleOCR-VL-1.5-8bit \
  --set ocr.device=auto \
  --set ocr.auto_download=true
```

PaddleOCR PP-OCR remains available as `ocr.engine=paddleocr` for box-native OCR:

```bash
wiki config \
  --set liteparse.ocr_enabled=true \
  --set ocr.engine=paddleocr \
  --set ocr.model_root=/path/to/ocr-models \
  --set ocr.model=PP-OCRv5_server \
  --set ocr.device=auto \
  --set ocr.auto_download=true
```

MinerU is supported as a local document parser that returns boxes from `*_middle.json`:

```bash
wiki config \
  --set liteparse.ocr_enabled=true \
  --set ocr.engine=mineru \
  --set ocr.model_root=/path/to/ocr-models \
  --set ocr.model=MinerU2.5 \
  --set ocr.device=auto \
  --set ocr.auto_download=true
```

DeepSeek-OCR is supported as a local grounding model that returns boxes from saved grounding outputs:

```bash
wiki config \
  --set liteparse.ocr_enabled=true \
  --set ocr.engine=deepseek-ocr \
  --set ocr.model_root=/path/to/ocr-models \
  --set ocr.model=DeepSeek-OCR-2 \
  --set ocr.device=auto \
  --set ocr.auto_download=true
```

Use `LLM_WIKI_PYTHON=/path/to/python` when a machine needs a specific Python environment for MinerU or DeepSeek-OCR. On Windows, use `wiki.exe` or the PowerShell install script from the release package; do not ask end users to run `cargo`.

## Common Workflows

Initialize:

```bash
wiki init
```

Compile a source file with the default skill Agent mode:

```bash
LLM_WIKI_SKILL_AGENT=1 wiki compile-prompt /path/to/file.pdf --source-type doc > /tmp/llm-wiki-prompt.txt
```

Use the Agent's built-in language model to answer the generated system/user prompts. Follow `COMPILE_SPEC.md`, `SCHEMA.md`, and the generated prompt exactly. The generated prompt includes the same compile output template and active wiki schema used by configured-model compilation. The response must keep the normal llm-wiki compile format, including YAML frontmatter and `===PAGE_END===` delimiters.

```bash
LLM_WIKI_SKILL_AGENT=1 wiki compile-ingest /path/to/file.pdf --source-type doc --response /tmp/llm-wiki-response.md --lang en
```

Use the language value printed under `---METADATA---` from `compile-prompt` for `--lang`.

Compile structured table files in skill mode without Agent generation:

```bash
wiki compile /path/to/file.csv --source-type doc
wiki compile /path/to/file.xlsx --source-type doc
```

These commands import table-like data directly into the DuckDB-backed Tables store. Supported formats are CSV, TSV, JSON, XLSX, and XLS. Markdown sources compiled through `compile-prompt` also extract embedded Markdown tables into Tables before the Agent prompt is generated, replacing large table blocks with `[[table:...]]` links.

Compile a directory in skill Agent mode by enumerating supported files and applying the same prompt/Agent/ingest flow to each file. Keep temporary prompt/response filenames unique per source file.

Configured-model compile is available only when explicitly requested:

```bash
wiki compile /path/to/file.pdf --source-type doc
```

Query:

```bash
wiki query "问题或检索内容"
```

Status and health:

```bash
wiki status
wiki lint
```

Dream consolidation:

```bash
wiki dream
```

`dream` is manual only; never start it automatically. It returns immediately and runs a background worker. Any `wiki query`, app query, `wiki compile`, app compile, `compile-prompt`, or `compile-ingest` cancels the active dream worker before continuing. Use `wiki dream --foreground` only for debugging.

The Rust CLI follows the `llm-wiki-skill` dream model:

- Phase 1 Light Sleep directly updates existing page metadata from today's queries (`questions`, `keywords`, `facts`, dream touch counters).
- Phase 2 Audit aggregates 7-day query logs and writes an Agent semantic-query-analysis task.
- Phase 3 Purify directly applies deterministic duplicate cleanup: merge duplicate body paragraphs into the survivor page, mark duplicate pages as redirects, and update graph edges.
- Phase 4 Enrich directly enriches low-density high-frequency pages with query-derived metadata and a `Dream Maintenance` body section, then writes deerflow/deep-research tasks to `dream/research-queue.jsonl`.

Dream is unattended content maintenance, not report-only. It initialises an internal git repository inside `.wiki`, creates snapshots before/after content-changing phases, evaluates search quality after each phase, keeps stable/improved changes, rolls back significant retrieval degradation, and records lessons in `dream/experience.md`. Each new dream run writes `dream/YYYYMMDD-context.md` with prior experience so past failures remain in context.

Package:

```bash
npm run package:all
```

`npm run package:all` is a maintainer/developer command. End users should receive the compiled `release/cli/wiki` binary or platform release package.
