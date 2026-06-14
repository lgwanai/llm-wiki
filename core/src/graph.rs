//! Knowledge graph management: entities.json + edges.json CRUD, BFS path finding.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use crate::compile_parse::ParsedPage;
use crate::config::{get_edges_path, get_entities_path};
use crate::error::WikiResult;
use crate::types::{Edge, EdgeCollection, Entity, EntityGraph};

pub fn load_entities() -> EntityGraph {
    let path = get_entities_path();
    if !path.exists() { return HashMap::new(); }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_entities(entities: &EntityGraph) -> WikiResult<()> {
    let path = get_entities_path();
    if let Some(p) = path.parent() { fs::create_dir_all(p)?; }
    fs::write(&path, serde_json::to_string_pretty(entities)?)?;
    Ok(())
}

pub fn load_edges() -> Vec<Edge> {
    let path = get_edges_path();
    if !path.exists() { return Vec::new(); }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<EdgeCollection>(&s).ok())
        .map(|c| c.edges)
        .unwrap_or_default()
}

pub fn save_edges(edges: &[Edge]) -> WikiResult<()> {
    let path = get_edges_path();
    if let Some(p) = path.parent() { fs::create_dir_all(p)?; }
    fs::write(&path, serde_json::to_string_pretty(&EdgeCollection { edges: edges.to_vec() })?)?;
    Ok(())
}

/// Connect all entities pairwise (guaranteed edges).
pub fn connect_entities(ids: &[String], label: &str) -> WikiResult<()> {
    if ids.len() < 2 { return Ok(()); }
    let mut edges = load_edges();
    let mut set: HashMap<(String, String, String), bool> = edges.iter()
        .map(|e| ((e.source.clone(), e.target.clone(), e.rel_type.clone()), true)).collect();
    let mut cnt = edges.len() + 1;
    for i in 0..ids.len() {
        for j in (i+1)..ids.len() {
            let (a, b) = if ids[i] < ids[j] { (&ids[i], &ids[j]) } else { (&ids[j], &ids[i]) };
            let key = (a.clone(), b.clone(), label.to_string());
            if !set.contains_key(&key) {
                set.insert(key.clone(), true);
                edges.push(Edge {
                    id: Some(format!("edge-{cnt:04}")), source: a.clone(), target: b.clone(),
                    rel_type: label.to_string(), description: None, confidence: Some(0.5),
                    sources: Some(vec![a.clone()]), weight: None,
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
        if !dir.exists() { continue; }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }
                let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
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
        if skip.contains(&i) { continue; }
        let (id_a, name_a, path_a, _dir_a) = &all_pages[i];

        for j in (i + 1)..all_pages.len() {
            if skip.contains(&j) { continue; }
            let (id_b, name_b, path_b, _dir_b) = &all_pages[j];

            if names_are_similar(name_a, name_b) {
                // Merge b into a (keep the one with longer name/content)
                let (keep_id, keep_path, merge_id, merge_path) = if name_a.len() >= name_b.len() {
                    (id_a, path_a, id_b, path_b)
                } else {
                    (id_b, path_b, id_a, path_a)
                };

                // Append merge page content to keep page
                let merge_content = fs::read_to_string(merge_path).unwrap_or_default();
                let mut keep_content = fs::read_to_string(keep_path).unwrap_or_default();
                let merge_body = if merge_content.starts_with("---") {
                    merge_content[4..].find("\n---")
                        .map(|e| merge_content[4 + e + 4..].trim().to_string())
                        .unwrap_or(merge_content)
                } else { merge_content };
                keep_content.push_str("\n\n<!-- merged from ");
                keep_content.push_str(merge_id);
                keep_content.push_str(" -->\n\n");
                keep_content.push_str(&merge_body);
                fs::write(keep_path, &keep_content)?;

                // Redirect edges: update all edges pointing to merge_id
                let mut edges = load_edges();
                for edge in &mut edges {
                    if edge.source == *merge_id { edge.source = keep_id.clone(); }
                    if edge.target == *merge_id { edge.target = keep_id.clone(); }
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
                eprintln!("[merge] '{merge_id}' → '{keep_id}' (similar names: '{name_a}' ≈ '{name_b}')");
            }
        }
    }

    Ok(merged)
}

/// Check if two names are similar enough to merge.
fn names_are_similar(a: &str, b: &str) -> bool {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();
    if a == b { return true; }
    // Substring match (require min 3 chars to avoid "e" matching "entity")
    if (a.len() >= 3 && b.len() >= 3) && (a.contains(&b) || b.contains(&a)) { return true; }
    // After removing common prefixes
    for prefix in &["ai ", "the ", "a ", "an "] {
        let ca = a.strip_prefix(prefix).unwrap_or(&a);
        let cb = b.strip_prefix(prefix).unwrap_or(&b);
        if ca.len() >= 3 && cb.len() >= 3 && (ca == cb || ca.contains(cb) || cb.contains(ca)) { return true; }
    }
    // Edit distance for short names
    if a.len().max(b.len()) < 20 {
        let dist = levenshtein(&a, &b);
        if dist <= 2 && a.len().min(b.len()) >= 3 { return true; }
    }
    false
}

/// Simple Levenshtein distance.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len(); let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..=n { dp[i][0] = i; }
    for j in 0..=m { dp[0][j] = j; }
    for i in 1..=n {
        for j in 1..=m {
            let cost = if a[i-1] == b[j-1] { 0 } else { 1 };
            dp[i][j] = (dp[i-1][j] + 1).min(dp[i][j-1] + 1).min(dp[i-1][j-1] + cost);
        }
    }
    dp[n][m]
}

pub fn add_entity_from_page(page: &ParsedPage, page_path: &Path) -> WikiResult<()> {
    let mut entities = load_entities();
    let entity = Entity {
        id: page.id.clone(),
        entity_type: page.frontmatter.get("type").and_then(|v| v.as_str()).unwrap_or("entity").into(),
        name: page.frontmatter.get("name").and_then(|v| v.as_str()).unwrap_or(&page.id).into(),
        confidence: page.frontmatter.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.5),
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
    for e in &edges { adj.entry(&e.source).or_default().push(e); }
    let mut queue = VecDeque::new();
    let mut visited = std::collections::HashSet::new();
    queue.push_back((source, vec![]));
    visited.insert(source);
    while let Some((current, path)) = queue.pop_front() {
        if current == target { return Some(path); }
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
    let mut existing_set: HashMap<(String, String, String), bool> = existing.iter()
        .map(|e| ((e.source.clone(), e.target.clone(), e.rel_type.clone()), true))
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
        if !dir.exists() { continue; }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }
                let source_id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if source_id.is_empty() { continue; }
                let content = fs::read_to_string(&path).unwrap_or_default();

                let body = if content.starts_with("---") {
                    content[4..].find("\n---").map(|e| &content[4 + e + 4..]).unwrap_or(&content)
                } else { &content };

                // 1. Wikilinks
                for cap in re_wikilink.captures_iter(body) {
                    let target = cap[1].trim().to_string();
                    if target.is_empty() || target == source_id { continue; }
                    if valid_ids.contains(&target) {
                        add_edge(&mut existing, &mut existing_set, &mut edge_id, &source_id, &target, "relates_to", 0.7);
                    }
                }

                // 2. Structured relationships
                for cap in re_rel_line.captures_iter(body) {
                    let rel_type = cap[1].to_lowercase();
                    let target = cap[2].trim().to_string();
                    if target.is_empty() || target == source_id { continue; }
                    if valid_ids.contains(&target) {
                        add_edge(&mut existing, &mut existing_set, &mut edge_id, &source_id, &target, &rel_type, 0.8);
                    }
                }

                // Group by source file
                let source_file = crate::compile::extract_frontmatter_field(&content, "source")
                    .unwrap_or_else(|| "unknown".to_string());
                pages_by_source.entry(source_file).or_default().push(source_id);
            }
        }
    }

    // 3. FALLBACK: Connect entities from the same source document
    for (_source, page_ids) in &pages_by_source {
        if page_ids.len() < 2 { continue; }
        for i in 0..page_ids.len() {
            for j in (i + 1)..page_ids.len() {
                add_edge(&mut existing, &mut existing_set, &mut edge_id, &page_ids[i], &page_ids[j], "related_to", 0.5);
            }
        }
    }

    eprintln!("[graph] Total edges: {}", existing.len());
    save_edges(&existing)
}

fn add_edge(
    existing: &mut Vec<Edge>, set: &mut HashMap<(String, String, String), bool>,
    edge_id: &mut usize, source: &str, target: &str, rel_type: &str, confidence: f64,
) {
    let key = (source.to_string(), target.to_string(), rel_type.to_string());
    if set.contains_key(&key) { return; }
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
    load_edges().into_iter()
        .filter(|e| e.target == entity_id && (e.rel_type == "uses" || e.rel_type == "depends_on"))
        .collect()
}

pub fn graph_stats() -> crate::types::GraphStats {
    let entities = load_entities();
    let edges = load_edges();
    let mut edge_types = HashMap::new();
    for e in &edges { *edge_types.entry(e.rel_type.clone()).or_insert(0) += 1; }
    let avg = if entities.is_empty() { 0.0 } else { edges.len() as f64 / entities.len() as f64 };
    let mut has_edge: std::collections::HashSet<String> = std::collections::HashSet::new();
    for e in &edges { has_edge.insert(e.source.clone()); has_edge.insert(e.target.clone()); }
    let orphan_count = entities.keys().filter(|k| !has_edge.contains(*k)).count();
    crate::types::GraphStats { entity_count: entities.len(), edge_count: edges.len(), edge_types, avg_edges_per_entity: (avg * 100.0).round() / 100.0, orphan_count }
}
