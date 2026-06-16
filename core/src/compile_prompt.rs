//! LLM prompt templates for wiki compilation (English + Chinese).

use crate::types::SourceType;

const CONCEPT_LIKE: &[&str] = &[
    "concept",
    "technique",
    "model",
    "framework",
    "benchmark",
    "paper",
    "pattern",
];

pub fn build_compile_prompt(
    content: &str,
    lang: &str,
    entity_types: &[&str],
    focus_description: &str,
    source_type: &SourceType,
) -> (String, String) {
    if lang == "zh" {
        build_chinese(content, entity_types, focus_description, source_type)
    } else {
        build_english(content, entity_types, focus_description, source_type)
    }
}

fn build_english(
    content: &str,
    entity_types: &[&str],
    focus_description: &str,
    _st: &SourceType,
) -> (String, String) {
    let all = entity_types.join(", ");
    let concept_t = entity_types
        .iter()
        .filter(|t| CONCEPT_LIKE.contains(t))
        .copied()
        .collect::<Vec<_>>()
        .join(", ");

    let sys = format!(
        "You are a knowledge extraction engine. Analyze documents and extract structured knowledge into wiki pages.

## Task
Read the provided document and identify all distinct knowledge units. For each unit, create a wiki page with YAML frontmatter and markdown body. Separate pages with `===PAGE_END===`.

## Entity Types
Focus on extracting: {focus}.
Valid entity types: {all}

## Page Types
- **entity**: A specific named thing (person, organization, tool, system, metric, paper, event, file)
- **concept**: An abstract idea, technique, process, rule, pattern, or framework
Concept-like types ({ct}) go in concepts/; rest go in entities/.

## Output Format (repeat for each knowledge unit)

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
[2-4 sentence summary of what this is and why it matters]

## Key Details
[Bullet points of the most important information]

## Relationships
- *uses* [[other-entity]] — why/how it uses it
- *depends_on* [[dependency]] — what it depends on
- *relates_to* [[related-entity]] — how they connect
- *part_of* [[parent-entity]] — what larger system it belongs to
- *supersedes* [[old-entity]] — what it replaces
- *contradicts* [[conflicting-entity]] — what it disagrees with

## Sources
[Where this information comes from in the source document]

===PAGE_END===

## Rules
1. Extract ALL significant knowledge units — be thorough.
2. Use canonical-slug IDs (lowercase, hyphens).
3. Confidence: 0.9 = explicit, 0.7 = clear, 0.5 = inferred.
4. Link entities via [[wikilinks]] in relationships.
5. Write in the same language as the source document.
6. Never skip the ===PAGE_END=== delimiter.",
        focus = focus_description,
        all = all,
        ct = concept_t,
    );

    let usr =
        format!("Extract all knowledge units from the following document:\n\n---\n{content}\n---");
    (sys, usr)
}

fn build_chinese(
    content: &str,
    entity_types: &[&str],
    focus_description: &str,
    _st: &SourceType,
) -> (String, String) {
    let all = entity_types.join(", ");
    let concept_t = entity_types
        .iter()
        .filter(|t| CONCEPT_LIKE.contains(t))
        .copied()
        .collect::<Vec<_>>()
        .join(", ");

    let sys = format!(
        "你是一个知识抽取引擎。分析文档并抽取结构化知识到 Wiki 页面。

## 任务
阅读提供的文档，识别所有独立的知识单元。为每个单元创建一个带有 YAML 前置元数据和 Markdown 正文的 Wiki 页面。用 `===PAGE_END===` 分隔各个页面。

## 实体类型
重点抽取：{focus}
有效的实体类型：{all}

## 页面类型
- **entity**：具有特定名称的事物（人物、组织、工具、系统、指标、论文、事件、文件）
- **concept**：抽象概念、技术、过程、规则、模式或框架
概念类型（{ct}）存放在 concepts/ 目录，其余存放在 entities/ 目录。

## 输出格式（每个知识单元重复）

---
id: canonical-slug
type: <entity|concept>
name: 人类可读的标题
confidence: 0.0-1.0
aliases: [别名1, 别名2]
keywords: [关键词1, 关键词2]
questions: [此页面回答的问题]
facts: [关键事实1, 关键事实2]
source: 文档文件名
---

## 概述
[2-4 句话总结这是什么以及为什么重要]

## 关键细节
[最重要的信息要点]

## 关系
- *uses* [[other-entity]] — 为什么使用/如何使用
- *depends_on* [[dependency]] — 依赖什么
- *relates_to* [[related-entity]] — 如何关联
- *part_of* [[parent-entity]] — 属于哪个更大的系统
- *supersedes* [[old-entity]] — 取代了什么
- *contradicts* [[conflicting-entity]] — 与什么矛盾

## 来源
[此信息在源文档中的出处]

===PAGE_END===

## 规则
1. 抽取所有重要的知识单元 — 要全面。
2. 使用 canonical-slug 格式的 ID（小写，连字符分隔）。
3. 置信度：0.9 = 明确且来源充分，0.7 = 清楚但不太确定，0.5 = 推断。
4. 在关系中通过 [[wikilinks]] 链接实体。
5. 使用与源文档相同的语言编写。
6. 永远不要省略 ===PAGE_END=== 分隔符。",
        focus = focus_description,
        all = all,
        ct = concept_t,
    );

    let usr = format!("从以下文档中抽取所有知识单元：\n\n---\n{content}\n---");
    (sys, usr)
}
