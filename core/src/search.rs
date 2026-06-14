//! Multi-stream hybrid search: BM25 + metadata + graph + ledger.
//! Uses Reciprocal Rank Fusion (RRF) to merge results across streams.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use crate::config::get_pages_dir;
use crate::graph;
use crate::search_tokenize;
use crate::types::{SearchResult, SearchStream};

const BM25_K1: f64 = 1.5;
const BM25_B: f64 = 0.75;
const RRF_K: f64 = 60.0;

/// All page subdirectories to search.
const PAGE_SUBDIRS: &[&str] = &[
    "concepts", "entities", "models", "techniques", "frameworks",
    "benchmarks", "papers", "decisions", "sessions", "patterns",
];

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn read_page_without_frontmatter(path: &PathBuf) -> String {
    let content = fs::read_to_string(path).unwrap_or_default();
    if content.starts_with("---") {
        if let Some(end) = content[4..].find("\n---") {
            return content[4 + end + 4..].to_string();
        }
    }
    content
}

fn read_page_parts(path: &PathBuf) -> (HashMap<String, serde_yaml::Value>, String) {
    let content = fs::read_to_string(path).unwrap_or_default();
    if !content.starts_with("---") {
        return (HashMap::new(), content);
    }
    if let Some(end) = content[4..].find("\n---") {
        let fm_str = &content[4..4 + end];
        let body = content[4 + end + 4..].to_string();
        let fm = serde_yaml::from_str(fm_str).unwrap_or_default();
        return (fm, body);
    }
    (HashMap::new(), content)
}

fn known_page_paths() -> Vec<PathBuf> {
    let pages_dir = get_pages_dir();
    let mut paths = Vec::new();
    for subdir in PAGE_SUBDIRS {
        let dir = pages_dir.join(subdir);
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("md") {
                        paths.push(path);
                    }
                }
            }
        }
    }
    paths
}

// ═══════════════════════════════════════════════════════════════════════════
// Stream 1: BM25 keyword search
// ═══════════════════════════════════════════════════════════════════════════

/// Build a BM25 index over all wiki pages. Returns map of path -> (tokens, freqs, length).
pub fn build_bm25_index() -> HashMap<String, Bm25Doc> {
    let mut index = HashMap::new();
    for path in known_page_paths() {
        let content = read_page_without_frontmatter(&path);
        let tokens: Vec<String> = search_tokenize::tokenize(&content)
            .into_iter()
            .map(|t| search_tokenize::stem(&t))
            .collect();
        if tokens.is_empty() {
            continue;
        }
        let length = tokens.len();
        let mut freqs: HashMap<String, usize> = HashMap::new();
        for t in &tokens {
            *freqs.entry(t.clone()).or_insert(0) += 1;
        }
        index.insert(
            path.to_string_lossy().to_string(),
            Bm25Doc { path: path.to_string_lossy().to_string(), tokens, freqs, length },
        );
    }
    index
}

#[derive(Debug, Clone)]
struct Bm25Doc {
    path: String,
    tokens: Vec<String>,
    freqs: HashMap<String, usize>,
    length: usize,
}

/// Search BM25 index and return scored results.
pub fn bm25_search(query: &str, index: &HashMap<String, Bm25Doc>, limit: usize) -> Vec<SearchResult> {
    let query_terms: Vec<String> = search_tokenize::tokenize(query)
        .into_iter()
        .map(|t| search_tokenize::stem(&t))
        .collect();

    if query_terms.is_empty() || index.is_empty() {
        return vec![];
    }

    let num_docs = index.len() as f64;
    let total_len: usize = index.values().map(|d| d.length).sum();
    let avg_dl = if num_docs > 0.0 { total_len as f64 / num_docs } else { 1.0 };

    // Document frequency per term
    let mut doc_freq: HashMap<&str, usize> = HashMap::new();
    for doc in index.values() {
        for term in doc.freqs.keys() {
            *doc_freq.entry(term.as_str()).or_insert(0) += 1;
        }
    }

    let mut scores: Vec<(String, f64)> = Vec::new();
    for (path, doc) in index {
        let mut score = 0.0;
        for term in &query_terms {
            let f = *doc.freqs.get(term.as_str()).unwrap_or(&0) as f64;
            if f == 0.0 {
                continue;
            }
            let df = *doc_freq.get(term.as_str()).unwrap_or(&1) as f64;
            let idf = ((num_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
            score += idf * (f * (BM25_K1 + 1.0))
                / (f + BM25_K1 * (1.0 - BM25_B + BM25_B * doc.length as f64 / avg_dl));
        }
        if score > 0.0 {
            scores.push((path.clone(), score));
        }
    }

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores.truncate(limit);

    scores
        .into_iter()
        .map(|(path, score)| SearchResult {
            id: path_to_id(&path),
            path: PathBuf::from(&path),
            score: (score * 1000.0).round() / 1000.0,
            stream: SearchStream::Bm25,
            rrf_score: None,
            title: None,
            summary: None,
            entity_type: None,
            stream_ranks: HashMap::new(),
            stream_scores: HashMap::new(),
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// Stream 2: Metadata search (frontmatter fields)
// ═══════════════════════════════════════════════════════════════════════════

/// Build a metadata index from page frontmatter.
pub fn build_metadata_index() -> Vec<MetadataEntry> {
    let mut items = Vec::new();
    for path in known_page_paths() {
        let (fm, body) = read_page_parts(&path);
        let title = body.lines().find(|l| l.starts_with("# "))
            .map(|l| l[2..].trim().to_string())
            .or_else(|| fm.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .unwrap_or_default();

        let file_id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let entry = MetadataEntry {
            id: fm.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or(&file_id).to_string(),
            name: title,
            entity_type: fm.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            aliases: list_from_yaml(&fm, "aliases"),
            keywords: list_from_yaml(&fm, "keywords"),
            summary: fm.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            questions: list_from_yaml(&fm, "questions"),
            facts: list_from_yaml(&fm, "facts"),
            path: path.to_string_lossy().to_string(),
        };
        items.push(entry);
    }
    items
}

#[derive(Debug, Clone)]
pub struct MetadataEntry {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
    pub keywords: Vec<String>,
    pub summary: String,
    pub questions: Vec<String>,
    pub facts: Vec<String>,
    pub path: String,
}

fn list_from_yaml(fm: &HashMap<String, serde_yaml::Value>, key: &str) -> Vec<String> {
    fm.get(key)
        .map(|v| match v {
            serde_yaml::Value::Sequence(s) => s.iter()
                .filter_map(|i| i.as_str().map(|s| s.to_string()))
                .collect(),
            serde_yaml::Value::String(s) => vec![s.clone()],
            _ => vec![],
        })
        .unwrap_or_default()
}

/// Search the metadata index and return scored results.
pub fn metadata_search(query: &str, index: &[MetadataEntry], limit: usize) -> Vec<SearchResult> {
    let q = query.to_lowercase();
    let q_terms: Vec<&str> = q.split_whitespace().collect();
    let mut scored: Vec<(usize, f64)> = Vec::new();

    for (i, entry) in index.iter().enumerate() {
        let mut score = 0.0f64;

        // Exact ID match
        if entry.id.to_lowercase() == q {
            score += 10.0;
        }
        // Name match
        if entry.name.to_lowercase().contains(&q) {
            score += 3.0;
        } else {
            for term in &q_terms {
                if entry.name.to_lowercase().contains(term) {
                    score += 1.5;
                }
            }
        }
        // Alias match
        for alias in &entry.aliases {
            if alias.to_lowercase().contains(&q) {
                score += 4.0;
            }
        }
        // Keyword match
        for kw in &entry.keywords {
            if kw.to_lowercase() == q || q.contains(&kw.to_lowercase()) {
                score += 2.0;
            }
        }
        // Question match
        for question in &entry.questions {
            let ql = question.to_lowercase();
            if ql.contains(&q) || q.contains(&ql) {
                score += 2.5;
            }
        }

        if score > 0.0 {
            scored.push((i, score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    scored
        .into_iter()
        .map(|(i, score)| {
            let entry = &index[i];
            SearchResult {
                id: entry.id.clone(),
                path: PathBuf::from(&entry.path),
                score: (score * 1000.0).round() / 1000.0,
                stream: SearchStream::Metadata,
                rrf_score: None,
                title: Some(entry.name.clone()),
                summary: if entry.summary.is_empty() { None } else { Some(entry.summary.clone()) },
                entity_type: if entry.entity_type.is_empty() { None } else { Some(entry.entity_type.clone()) },
                stream_ranks: HashMap::new(),
                stream_scores: HashMap::new(),
            }
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// Stream 3: Graph-based search
// ═══════════════════════════════════════════════════════════════════════════

/// Search knowledge graph for matching entities.
pub fn graph_search(query: &str, limit: usize) -> Vec<SearchResult> {
    let entities = graph::load_entities();
    let q = query.to_lowercase();
    let mut scored: Vec<(String, f64, String)> = Vec::new(); // (id, score, name)

    for (eid, entity) in &entities {
        let mut score = 0.0f64;
        let name = entity.name.to_lowercase();

        if name == q || eid.to_lowercase() == q {
            score += 15.0;
        } else if name.contains(&q) || q.contains(&name) {
            score += 5.0;
        } else {
            let match_count = q.split_whitespace()
                .filter(|t| name.contains(t) || eid.contains(t))
                .count();
            score += match_count as f64 * 2.0;
        }

        if score > 0.0 {
            scored.push((eid.clone(), score, entity.name.clone()));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    scored
        .into_iter()
        .map(|(id, score, name)| SearchResult {
            id: id.clone(),
            path: PathBuf::new(),
            score: (score * 1000.0).round() / 1000.0,
            stream: SearchStream::Graph,
            rrf_score: None,
            title: Some(name),
            summary: None,
            entity_type: Some("graph-entity".into()),
            stream_ranks: HashMap::new(),
            stream_scores: HashMap::new(),
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// Reciprocal Rank Fusion
// ═══════════════════════════════════════════════════════════════════════════

/// Fuse multiple ranked result lists using RRF.
pub fn reciprocal_rank_fusion(
    stream_results: Vec<(SearchStream, Vec<SearchResult>)>,
    limit: usize,
) -> Vec<SearchResult> {
    // Track rank and score per doc per stream
    let _doc_ranks: HashMap<String, usize> = HashMap::new();
    let mut doc_scores: HashMap<String, f64> = HashMap::new();
    let mut doc_data: HashMap<String, SearchResult> = HashMap::new();

    for (stream, results) in &stream_results {
        for (rank, result) in results.iter().enumerate() {
            let key = result.id.clone();
            let rrf_contrib = 1.0 / (RRF_K + (rank + 1) as f64);

            *doc_scores.entry(key.clone()).or_insert(0.0) += rrf_contrib;

            let entry = doc_data.entry(key.clone()).or_insert_with(|| result.clone());
            entry.stream_ranks.insert(stream.to_string(), rank + 1);
            entry.stream_scores.insert(stream.to_string(), result.score);
        }
    }

    let mut fused: Vec<SearchResult> = doc_data.into_values().collect();
    fused.sort_by(|a, b| {
        let sa = doc_scores.get(&a.id).unwrap_or(&0.0);
        let sb = doc_scores.get(&b.id).unwrap_or(&0.0);
        sb.partial_cmp(sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    fused.truncate(limit);

    // Set RRF scores
    for result in &mut fused {
        result.rrf_score = Some(
            (doc_scores.get(&result.id).copied().unwrap_or(0.0) * 1000.0).round() / 1000.0,
        );
    }

    fused
}

// ═══════════════════════════════════════════════════════════════════════════
// Unified search
// ═══════════════════════════════════════════════════════════════════════════


static BM25_CACHE: std::sync::Mutex<Option<(u64, HashMap<String, Bm25Doc>)>> = std::sync::Mutex::new(None);
static META_CACHE: std::sync::Mutex<Option<(u64, Vec<MetadataEntry>)>> = std::sync::Mutex::new(None);

/// Get a hash of all page paths + mtimes to detect changes
fn page_hash() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut paths = known_page_paths();
    paths.sort(); // Ensure deterministic hash order
    for path in &paths {
        path.hash(&mut h);
        if let Ok(m) = std::fs::metadata(&path) {
            if let Ok(t) = m.modified() {
                t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs().hash(&mut h);
            }
        }
    }
    h.finish()
}

fn get_cached_bm25() -> HashMap<String, Bm25Doc> {
    let hash = page_hash();
    if let Ok(cache) = BM25_CACHE.lock() {
        if let Some((cached_hash, ref idx)) = *cache {
            if cached_hash == hash { return idx.clone(); }
        }
    }
    let idx = build_bm25_index();
    if let Ok(mut cache) = BM25_CACHE.lock() { *cache = Some((hash, idx.clone())); }
    idx
}

fn get_cached_metadata() -> Vec<MetadataEntry> {
    let hash = page_hash();
    if let Ok(cache) = META_CACHE.lock() {
        if let Some((cached_hash, ref idx)) = *cache {
            if cached_hash == hash { return idx.clone(); }
        }
    }
    let idx = build_metadata_index();
    if let Ok(mut cache) = META_CACHE.lock() { *cache = Some((hash, idx.clone())); }
    idx
}

pub fn search(
    query: &str,
    enabled_streams: &HashSet<String>,
    limit: usize,
) -> Vec<SearchResult> {
    let mut stream_results: Vec<(SearchStream, Vec<SearchResult>)> = Vec::new();

    if enabled_streams.contains("bm25") {
        let index = get_cached_bm25();
        let results = bm25_search(query, &index, limit);
        stream_results.push((SearchStream::Bm25, results));
    }

    if enabled_streams.contains("metadata") {
        let meta_index = get_cached_metadata();
        let results = metadata_search(query, &meta_index, limit);
        stream_results.push((SearchStream::Metadata, results));
    }

    if enabled_streams.contains("graph") {
        let results = graph_search(query, limit);
        stream_results.push((SearchStream::Graph, results));
    }

    reciprocal_rank_fusion(stream_results, limit)
}

/// Search doctor: diagnose index health.
pub fn search_doctor() -> serde_json::Value {
    let _pages_dir = get_pages_dir();
    let page_count = known_page_paths().len();
    let entities = graph::load_entities();
    let graph_stats = graph::graph_stats();

    serde_json::json!({
        "page_count": page_count,
        "entity_count": entities.len(),
        "graph": {
            "entities": graph_stats.entity_count,
            "edges": graph_stats.edge_count,
            "orphans": graph_stats.orphan_count,
        },
        "index_health": if page_count > 0 { "ok" } else { "empty" },
    })
}

fn path_to_id(path: &str) -> String {
    PathBuf::from(path)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}
