//! Knowledge graph management: entities.json + edges.json CRUD, BFS path finding.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use crate::compile_parse::ParsedPage;
use crate::config::{get_edges_path, get_entities_path};
use crate::error::WikiResult;
use crate::types::{Edge, EdgeCollection, Entity, EntityGraph};

pub fn load_entities() -> EntityGraph {
    let path = get_entities_path();
    if !path.exists() {
        return HashMap::new();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_entities(entities: &EntityGraph) -> WikiResult<()> {
    let path = get_entities_path();
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(&path, serde_json::to_string_pretty(entities)?)?;
    Ok(())
}

pub fn load_edges() -> Vec<Edge> {
    let path = get_edges_path();
    if !path.exists() {
        return Vec::new();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<EdgeCollection>(&s).ok())
        .map(|c| c.edges)
        .unwrap_or_default()
}

pub fn save_edges(edges: &[Edge]) -> WikiResult<()> {
    let path = get_edges_path();
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(
        &path,
        serde_json::to_string_pretty(&EdgeCollection {
            edges: edges.to_vec(),
        })?,
    )?;
    Ok(())
}

/// Connect all entities pairwise (guaranteed edges).
pub fn connect_entities(ids: &[String], label: &str) -> WikiResult<()> {
    if ids.len() < 2 {
        return Ok(());
    }
    let mut edges = load_edges();
    let mut set: HashMap<(String, String, String), bool> = edges
        .iter()
        .map(|e| {
            (
                (e.source.clone(), e.target.clone(), e.rel_type.clone()),
                true,
            )
        })
        .collect();
    let mut cnt = edges.len() + 1;
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let (a, b) = if ids[i] < ids[j] {
                (&ids[i], &ids[j])
            } else {
                (&ids[j], &ids[i])
            };
            let key = (a.clone(), b.clone(), label.to_string());
            if !set.contains_key(&key) {
                set.insert(key.clone(), true);
                edges.push(Edge {
                    id: Some(format!("edge-{cnt:04}")),
                    source: a.clone(),
                    target: b.clone(),
                    rel_type: label.to_string(),
                    description: None,
                    confidence: Some(0.5),
                    sources: Some(vec![a.clone()]),
                    weight: None,
                    created_at: Some(chrono::Utc::now().to_rfc3339()),
                });
                cnt += 1;
            }
        }
    }
    save_edges(&edges)
}

/// Merge pages with similar names (e.g., "AI Agent" ≈ "Agent").
/// Returns number of merges performed.
pub fn merge_similar_pages(wiki_dir: &Path) -> WikiResult<usize> {
    let pages_dir = wiki_dir.join("pages");
    let mut all_pages: Vec<(String, String, PathBuf, String)> = Vec::new(); // (id, name, path, type_dir)

    for subdir in &["concepts", "entities"] {
        let dir = pages_dir.join(subdir);
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let id = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(&path).unwrap_or_default();
                let name = crate::compile::extract_frontmatter_field(&content, "name")
                    .unwrap_or_else(|| id.clone());
                all_pages.push((id, name.to_lowercase(), path.clone(), subdir.to_string()));
            }
        }
    }

    let mut merged = 0usize;
    let mut skip = std::collections::HashSet::new();

    for i in 0..all_pages.len() {
        if skip.contains(&i) {
            continue;
        }
        let (id_a, name_a, path_a, _dir_a) = &all_pages[i];

        for j in (i + 1)..all_pages.len() {
            if skip.contains(&j) {
                continue;
            }
            let (id_b, name_b, path_b, _dir_b) = &all_pages[j];

            if names_are_similar(name_a, name_b) {
                // Merge b into a (keep the one with longer name/content)
                let (keep_id, keep_path, merge_id, merge_path) = if name_a.len() >= name_b.len() {
                    (id_a, path_a, id_b, path_b)
                } else {
                    (id_b, path_b, id_a, path_a)
                };

                let merge_content = fs::read_to_string(merge_path).unwrap_or_default();
                let keep_content = fs::read_to_string(keep_path).unwrap_or_default();
                let keep_content = merge_page_content(&keep_content, &merge_content, merge_id);
                fs::write(keep_path, &keep_content)?;

                // Redirect edges: update all edges pointing to merge_id
                let mut edges = load_edges();
                for edge in &mut edges {
                    if edge.source == *merge_id {
                        edge.source = keep_id.clone();
                    }
                    if edge.target == *merge_id {
                        edge.target = keep_id.clone();
                    }
                }
                // Remove self-edges
                edges.retain(|e| e.source != e.target);
                save_edges(&edges)?;

                // Remove merged page
                fs::remove_file(merge_path)?;
                // Also remove from entities if exists
                let mut entities = load_entities();
                entities.remove(merge_id);
                save_entities(&entities)?;

                merged += 1;
                skip.insert(j);
                eprintln!(
                    "[merge] '{merge_id}' → '{keep_id}' (similar names: '{name_a}' ≈ '{name_b}')"
                );
            }
        }
    }

    Ok(merged)
}

fn merge_page_content(keep_content: &str, merge_content: &str, merge_id: &str) -> String {
    let (mut keep_fm, keep_body) = split_markdown_page(keep_content);
    let (merge_fm, merge_body) = split_markdown_page(merge_content);
    merge_frontmatter(&mut keep_fm, &merge_fm, merge_id);
    let body = merge_markdown_bodies(&keep_body, &merge_body);
    if keep_fm.is_empty() {
        body
    } else {
        let fm = serde_yaml::to_string(&keep_fm).unwrap_or_default();
        format!("---\n{}---\n\n{}", fm, body)
    }
}

fn split_markdown_page(content: &str) -> (serde_yaml::Mapping, String) {
    if !content.starts_with("---\n") {
        return (serde_yaml::Mapping::new(), clean_merge_markers(content));
    }
    let Some(end) = content[4..].find("\n---") else {
        return (serde_yaml::Mapping::new(), clean_merge_markers(content));
    };
    let fm = content[4..4 + end].trim();
    let body = content[4 + end + 4..].trim();
    let mapping = serde_yaml::from_str::<serde_yaml::Mapping>(fm).unwrap_or_default();
    (mapping, clean_merge_markers(body))
}

fn merge_frontmatter(
    target: &mut serde_yaml::Mapping,
    source: &serde_yaml::Mapping,
    merge_id: &str,
) {
    for key in ["aliases", "keywords", "facts", "questions", "source_files"] {
        let values = yaml_list_strings(source, key);
        add_yaml_list_values(target, key, &values, 32);
    }
    add_yaml_list_values(
        target,
        "facts",
        &[format!(
            "Merged duplicate concept [[{merge_id}]] during compile deduplication"
        )],
        32,
    );
    let key = serde_yaml::Value::String("confidence".into());
    let old_c = target.get(&key).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let new_c = source.get(&key).and_then(|v| v.as_f64()).unwrap_or(0.0);
    if new_c > old_c {
        target.insert(
            key,
            serde_yaml::Value::Number(serde_yaml::Number::from(new_c)),
        );
    }
}

fn merge_markdown_bodies(left: &str, right: &str) -> String {
    let mut merged = split_sections(left);
    for (heading, lines) in split_sections(right) {
        let entry = merged.entry(heading).or_default();
        for line in lines {
            insert_fused_line(entry, line);
        }
    }
    render_sections(merged)
}

fn split_sections(body: &str) -> BTreeMap<String, Vec<String>> {
    let mut sections: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current = "概述".to_string();
    for raw in clean_merge_markers(body).lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(heading) = canonical_heading(line) {
            current = heading;
            sections.entry(current.clone()).or_default();
            continue;
        }
        insert_fused_line(
            sections.entry(current.clone()).or_default(),
            line.to_string(),
        );
    }
    sections
}

fn canonical_heading(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_start_matches('#').trim();
    match trimmed {
        "概述" | "Overview" | "Summary" => Some("概述".into()),
        "关键细节" | "Details" | "Key Details" => Some("关键细节".into()),
        "关系" | "Relationships" => Some("关系".into()),
        "来源" | "Source" | "Sources" => Some("来源".into()),
        _ if line.starts_with('#') => Some(trimmed.to_string()),
        _ => None,
    }
}

fn insert_fused_line(lines: &mut Vec<String>, candidate: String) {
    let candidate_norm = normalize_line(&candidate);
    if candidate_norm.is_empty() {
        return;
    }
    for existing in lines.iter_mut() {
        let existing_norm = normalize_line(existing);
        if existing_norm == candidate_norm || similar_line(&existing_norm, &candidate_norm) {
            if candidate.chars().count() > existing.chars().count() {
                *existing = candidate;
            }
            return;
        }
    }
    lines.push(candidate);
}

fn render_sections(sections: BTreeMap<String, Vec<String>>) -> String {
    let order = ["概述", "关键细节", "关系", "来源"];
    let mut out = String::new();
    let mut rendered = HashSet::new();
    for heading in order {
        if let Some(lines) = sections.get(heading) {
            push_section(&mut out, heading, lines);
            rendered.insert(heading.to_string());
        }
    }
    for (heading, lines) in sections {
        if !rendered.contains(&heading) {
            push_section(&mut out, &heading, &lines);
        }
    }
    out.trim().to_string()
}

fn push_section(out: &mut String, heading: &str, lines: &[String]) {
    let lines: Vec<_> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(heading);
    out.push_str("\n\n");
    out.push_str(
        &lines
            .into_iter()
            .map(|l| l.trim().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

fn clean_merge_markers(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("<!-- merged")
                && !trimmed.starts_with("<!-- dream auto-merged")
                && !trimmed.starts_with("<!-- source-ref:")
                && !trimmed.starts_with("_Source:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn yaml_list_strings(map: &serde_yaml::Mapping, key: &str) -> Vec<String> {
    match map.get(serde_yaml::Value::String(key.to_string())) {
        Some(serde_yaml::Value::Sequence(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(serde_yaml::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn add_yaml_list_values(
    map: &mut serde_yaml::Mapping,
    key: &str,
    values: &[String],
    max_len: usize,
) {
    let yaml_key = serde_yaml::Value::String(key.to_string());
    let mut existing = yaml_list_strings(map, key);
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if !existing
            .iter()
            .any(|e| similar_line(&normalize_line(e), &normalize_line(value)))
        {
            existing.push(value.to_string());
        }
        if existing.len() >= max_len {
            break;
        }
    }
    map.insert(
        yaml_key,
        serde_yaml::Value::Sequence(
            existing
                .into_iter()
                .take(max_len)
                .map(serde_yaml::Value::String)
                .collect(),
        ),
    );
}

fn normalize_line(line: &str) -> String {
    line.to_lowercase()
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && !c.is_ascii_punctuation()
                && !"，。；：、“”‘’（）【】《》—".contains(*c)
        })
        .collect()
}

fn similar_line(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.contains(b) || b.contains(a) {
        return true;
    }
    let grams_a = bigrams(a);
    let grams_b = bigrams(b);
    if grams_a.is_empty() || grams_b.is_empty() {
        return false;
    }
    let intersection = grams_a.intersection(&grams_b).count() as f64;
    let union = grams_a.union(&grams_b).count() as f64;
    intersection / union >= 0.42
}

fn bigrams(text: &str) -> HashSet<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return chars.into_iter().map(|c| c.to_string()).collect();
    }
    chars
        .windows(2)
        .map(|w| w.iter().collect::<String>())
        .collect()
}

/// Check if two names are similar enough to merge.
fn names_are_similar(a: &str, b: &str) -> bool {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();
    if a == b {
        return true;
    }
    // Substring match (require min 3 chars to avoid "e" matching "entity")
    if (a.len() >= 3 && b.len() >= 3) && (a.contains(&b) || b.contains(&a)) {
        return true;
    }
    // After removing common prefixes
    for prefix in &["ai ", "the ", "a ", "an "] {
        let ca = a.strip_prefix(prefix).unwrap_or(&a);
        let cb = b.strip_prefix(prefix).unwrap_or(&b);
        if ca.len() >= 3 && cb.len() >= 3 && (ca == cb || ca.contains(cb) || cb.contains(ca)) {
            return true;
        }
    }
    // Edit distance for short names
    if a.len().max(b.len()) < 20 {
        let dist = levenshtein(&a, &b);
        if dist <= 2 && a.len().min(b.len()) >= 3 {
            return true;
        }
    }
    false
}

/// Simple Levenshtein distance.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..=n {
        dp[i][0] = i;
    }
    for j in 0..=m {
        dp[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[n][m]
}

pub fn add_entity_from_page(page: &ParsedPage, page_path: &Path) -> WikiResult<()> {
    let mut entities = load_entities();
    let entity = Entity {
        id: page.id.clone(),
        entity_type: page
            .frontmatter
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("entity")
            .into(),
        name: page
            .frontmatter
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&page.id)
            .into(),
        confidence: page
            .frontmatter
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5),
        attributes: page.frontmatter.clone(),
        sources: vec![],
        page: Some(page_path.to_string_lossy().to_string()),
        aliases: None,
        last_confirmed: Some(chrono::Utc::now().to_rfc3339()),
        status: Some("active".into()),
    };
    entities.insert(page.id.clone(), entity);
    save_entities(&entities)
}

pub fn find_path(source: &str, target: &str) -> Option<Vec<Edge>> {
    let edges = load_edges();
    let mut adj: HashMap<&str, Vec<&Edge>> = HashMap::new();
    for e in &edges {
        adj.entry(&e.source).or_default().push(e);
    }
    let mut queue = VecDeque::new();
    let mut visited = std::collections::HashSet::new();
    queue.push_back((source, vec![]));
    visited.insert(source);
    while let Some((current, path)) = queue.pop_front() {
        if current == target {
            return Some(path);
        }
        if let Some(nbrs) = adj.get(current) {
            for edge in nbrs {
                if !visited.contains(edge.target.as_str()) {
                    visited.insert(&edge.target);
                    let mut np = path.clone();
                    np.push((*edge).clone());
                    queue.push_back((&edge.target, np));
                }
            }
        }
    }
    None
}

/// Extract edges from page content (wikilinks + relationships) and also create
/// fallback edges between entities compiled from the same source to ensure graph connectivity.
pub fn extract_edges_from_pages(wiki_dir: &Path) -> WikiResult<()> {
    let mut existing = load_edges();
    let mut existing_set: HashMap<(String, String, String), bool> = existing
        .iter()
        .map(|e| {
            (
                (e.source.clone(), e.target.clone(), e.rel_type.clone()),
                true,
            )
        })
        .collect();

    let pages_dir = wiki_dir.join("pages");
    let mut edge_id = existing.len() + 1;
    let re_wikilink = regex::Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").unwrap();
    let re_rel_line = regex::Regex::new(r"-\s+\*?(\w+)\*?\s+\[\[([^\]]+)\]\]").unwrap();

    let entities = load_entities();
    let valid_ids: std::collections::HashSet<String> = entities.keys().cloned().collect();

    // Group pages by source file to create fallback edges
    let mut pages_by_source: HashMap<String, Vec<String>> = HashMap::new();

    for subdir in &["concepts", "entities"] {
        let dir = pages_dir.join(subdir);
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let source_id = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if source_id.is_empty() {
                    continue;
                }
                let content = fs::read_to_string(&path).unwrap_or_default();

                let body = if content.starts_with("---") {
                    content[4..]
                        .find("\n---")
                        .map(|e| &content[4 + e + 4..])
                        .unwrap_or(&content)
                } else {
                    &content
                };

                // 1. Wikilinks
                for cap in re_wikilink.captures_iter(body) {
                    let target = cap[1].trim().to_string();
                    if target.is_empty() || target == source_id {
                        continue;
                    }
                    if valid_ids.contains(&target) {
                        add_edge(
                            &mut existing,
                            &mut existing_set,
                            &mut edge_id,
                            &source_id,
                            &target,
                            "relates_to",
                            0.7,
                        );
                    }
                }

                // 2. Structured relationships
                for cap in re_rel_line.captures_iter(body) {
                    let rel_type = cap[1].to_lowercase();
                    let target = cap[2].trim().to_string();
                    if target.is_empty() || target == source_id {
                        continue;
                    }
                    if valid_ids.contains(&target) {
                        add_edge(
                            &mut existing,
                            &mut existing_set,
                            &mut edge_id,
                            &source_id,
                            &target,
                            &rel_type,
                            0.8,
                        );
                    }
                }

                // Group by source file
                let source_file = crate::compile::extract_frontmatter_field(&content, "source")
                    .unwrap_or_else(|| "unknown".to_string());
                pages_by_source
                    .entry(source_file)
                    .or_default()
                    .push(source_id);
            }
        }
    }

    // 3. FALLBACK: Connect entities from the same source document
    for (_source, page_ids) in &pages_by_source {
        if page_ids.len() < 2 {
            continue;
        }
        for i in 0..page_ids.len() {
            for j in (i + 1)..page_ids.len() {
                add_edge(
                    &mut existing,
                    &mut existing_set,
                    &mut edge_id,
                    &page_ids[i],
                    &page_ids[j],
                    "related_to",
                    0.5,
                );
            }
        }
    }

    eprintln!("[graph] Total edges: {}", existing.len());
    save_edges(&existing)
}

fn add_edge(
    existing: &mut Vec<Edge>,
    set: &mut HashMap<(String, String, String), bool>,
    edge_id: &mut usize,
    source: &str,
    target: &str,
    rel_type: &str,
    confidence: f64,
) {
    let key = (source.to_string(), target.to_string(), rel_type.to_string());
    if set.contains_key(&key) {
        return;
    }
    set.insert(key.clone(), true);
    existing.push(Edge {
        id: Some(format!("edge-{:04}", edge_id)),
        source: source.to_string(),
        target: target.to_string(),
        rel_type: rel_type.to_string(),
        description: None,
        confidence: Some(confidence),
        sources: Some(vec![source.to_string()]),
        weight: None,
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    });
    *edge_id += 1;
}

pub fn impact_analysis(entity_id: &str) -> Vec<Edge> {
    load_edges()
        .into_iter()
        .filter(|e| e.target == entity_id && (e.rel_type == "uses" || e.rel_type == "depends_on"))
        .collect()
}

pub fn graph_stats() -> crate::types::GraphStats {
    let entities = load_entities();
    let edges = load_edges();
    let mut edge_types = HashMap::new();
    for e in &edges {
        *edge_types.entry(e.rel_type.clone()).or_insert(0) += 1;
    }
    let avg = if entities.is_empty() {
        0.0
    } else {
        edges.len() as f64 / entities.len() as f64
    };
    let mut has_edge: std::collections::HashSet<String> = std::collections::HashSet::new();
    for e in &edges {
        has_edge.insert(e.source.clone());
        has_edge.insert(e.target.clone());
    }
    let orphan_count = entities.keys().filter(|k| !has_edge.contains(*k)).count();
    crate::types::GraphStats {
        entity_count: entities.len(),
        edge_count: edges.len(),
        edge_types,
        avg_edges_per_entity: (avg * 100.0).round() / 100.0,
        orphan_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_page_content_fuses_sections_without_copying_blocks() {
        let keep = r#"---
id: ai-best-practices
type: concept
name: AI 最佳实践
aliases: [AI 实践指南]
keywords: [AI, 合规]
facts: [AI 最佳实践强调人工审核。]
confidence: 0.7
---

概述

AI 最佳实践强调 prompt 要素、人工审核、合规检查等。

关键细节

人工审核：所有 AI 产出物必须经过人工审核。
"#;
        let merge = r#"---
id: ai-best-practice
type: concept
name: AI 最佳实践
aliases: [AI 最佳实践原则]
keywords: [AI, 质量把控]
facts: [AI 最佳实践强调合规检查。]
confidence: 0.8
---

概述

AI 最佳实践强调 prompt 设计、人工审核、合规检查和质量把控。

关键细节

合规检查：建立合规检查清单。
"#;
        let merged = merge_page_content(keep, merge, "ai-best-practice");
        assert!(merged.contains("质量把控"));
        assert!(merged.contains("合规检查：建立合规检查清单。"));
        assert!(merged.contains("AI 最佳实践原则"));
        assert!(!merged.contains("<!-- merged"));
        assert_eq!(merged.matches("概述").count(), 1);
    }
}
