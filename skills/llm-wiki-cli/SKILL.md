---
name: llm-wiki-cli
description: Operate the llm-wiki Rust CLI for personal knowledge base workflows. Use when Codex needs to configure llm-wiki, set OCR/local model options, initialize a wiki, compile files or directories, query the wiki, inspect status, run lint, or package the CLI/desktop app from this repository.
---

# LLM Wiki CLI

Use this skill to run the compiled `wiki` CLI and keep desktop and CLI configuration synchronized. Do not require users to install Rust or run `cargo` unless they are explicitly building from source.

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

Compile a source file:

```bash
wiki compile /path/to/file.pdf --source-type doc
```

Compile a directory:

```bash
wiki compile /path/to/docs --source-type doc --depth 3 -j 2
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

Package:

```bash
npm run package:all
```

`npm run package:all` is a maintainer/developer command. End users should receive the compiled `release/cli/wiki` binary or platform release package.
