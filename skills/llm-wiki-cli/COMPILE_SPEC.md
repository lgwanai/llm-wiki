# Skill Agent Compile Specification

This document is the local compile contract for the `llm-wiki-cli` skill. Skill Agent compile must follow this contract and `SCHEMA.md`.

## Default Mode

When this skill compiles a source file, use Agent-powered compile by default:

1. Run `LLM_WIKI_SKILL_AGENT=1 wiki compile-prompt <source> --source-type <type>` and save the prompt.
2. Use the host Agent model to answer the generated system and user prompts.
3. Save the Agent response as markdown.
4. Run `LLM_WIKI_SKILL_AGENT=1 wiki compile-ingest <source> --source-type <type> --response <response.md> --lang <language>`.

Do not call `wiki compile` unless the source is structured table data or the user explicitly asks to use the CLI/app configured model.

Structured table files are an exception to Agent-powered compile. For `.csv`, `.tsv`, `.json`, `.xlsx`, and `.xls`, run `wiki compile <source> --source-type <type>` so the CLI imports the file directly into Tables without LLM generation. If `compile-prompt` or `compile-ingest` is accidentally called for one of these formats, the CLI will import it as a table and print `skip_agent: true`.

Markdown table data must also be preserved as structured tables. `compile-prompt` extracts embedded Markdown tables into Tables before prompt generation and replaces table blocks with `[[table:...]]` links in the prompt content. Do not recreate those extracted rows as prose unless the surrounding document contains separate knowledge claims.

## Required Prompt Inputs

The Agent must use all of these sources of instruction:

- The system prompt emitted by `wiki compile-prompt`.
- The user prompt emitted by `wiki compile-prompt`.
- The local compile template in this file.
- The wiki schema and quality standards in `SCHEMA.md`.

If these conflict, prefer the emitted `wiki compile-prompt` contract, then `SCHEMA.md`, then this file.

## Output Format

Repeat this format once for each extracted knowledge unit:

```markdown
---
id: canonical-slug
type: <entity|concept>
name: Human-Readable Title
confidence: 0.0-1.0
aliases: [alias1, alias2]
keywords: [keyword1, keyword2]
questions: [question this page answers]
facts: [key fact 1, key fact 2]
source: document-filename
---

## Overview
[2-4 sentence stable summary of the concept/entity and why it matters]

## Shared Understanding
[Facts, definitions, mechanisms, or claims that are broadly reusable and likely to remain true across sources]

## Source Perspective
[What this source specifically says, emphasizes, customizes, disputes, or adds]

## Variations and Conditions
[Important differences, constraints, exceptions, versions, interpretations, use cases, or open disagreements]

## Relationships
- *uses* [[other-entity]] - why/how it uses it
- *depends_on* [[dependency]] - what it depends on
- *relates_to* [[related-entity]] - how they connect
- *part_of* [[parent-entity]] - what larger system it belongs to
- *supersedes* [[old-entity]] - what it replaces
- *contradicts* [[conflicting-entity]] - what it disagrees with

## Sources
[Where this information comes from in the source document]

===PAGE_END===
```

## Compile Rules

1. Extract all significant knowledge units. Do not summarize the whole file into one page when it contains multiple independent concepts or entities.
2. Use stable canonical slugs for `id`, lowercase with hyphens. Reuse IDs for the same concept across related files.
3. Use `type: concept` for abstract ideas, processes, rules, models, frameworks, techniques, patterns, and decisions. Use `type: entity` for named concrete things.
4. Keep universal facts in `Shared Understanding`; keep source-specific claims in `Source Perspective` or `Variations and Conditions`.
5. Include relationship lines with `[[canonical-id]]` wikilinks whenever the source implies relationships.
6. Preserve source language. Chinese sources should produce Chinese pages; English sources should produce English pages.
7. Keep YAML valid. Arrays should be bracket arrays or valid YAML lists.
8. Never omit `===PAGE_END===` after a page.
9. Never invent source citations, dates, owners, versions, or relationships not supported by the source.
10. Redact secrets, credentials, private keys, passwords, and sensitive personal data.

## Retrieval Requirements

Agent-generated pages must remain searchable by all retrieval streams:

- BM25: put important terms in page body sections, not only frontmatter.
- Metadata: fill `name`, `aliases`, `keywords`, `questions`, and `facts`.
- Graph: use stable `id` values and relationship wikilinks that point to other canonical IDs.
- Provenance: keep a meaningful `source` value. `compile-ingest` will also add `source_name`, `source_type`, `source_files`, and `source_refs`.

## Directory Compile

For a directory, enumerate supported source files. Use direct `wiki compile` for structured table files (`.csv`, `.tsv`, `.json`, `.xlsx`, `.xls`) and the single-file Agent compile flow for other supported documents. Use unique temporary prompt and response paths per source.
