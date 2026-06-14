//! Core compilation engine.
//!
//! Main pipeline: source file → read → strip sensitive → detect language →
//! generate LLM prompt → call LLM → parse response → write wiki pages →
//! update graph → update index → write audit trail.
//!
//! Uses liteparse for native PDF text extraction (no OCR needed for PDFs).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::compile_parse::parse_compile_response;
use crate::compile_prompt::build_compile_prompt;
use crate::config::get_wiki_dir;
use crate::error::{WikiError, WikiResult};
use crate::graph;
use crate::llm;
use crate::types::{CompileResult, PageType, SourceType};

const TEXT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "rst", "adoc", "csv", "tsv", "json", "jsonl",
    "yaml", "yml", "html", "htm", "xml", "svg", "py", "js", "ts", "jsx",
    "tsx", "go", "rs", "java", "c", "cc", "cpp", "h", "hpp", "cs", "php",
    "rb", "sh", "bash", "zsh", "sql", "toml", "ini", "cfg",
];

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff", "tif", "avif", "heic", "heif",
];

const PDF_EXTENSIONS: &[&str] = &["pdf"];

const SKIP_DIR_NAMES: &[&str] = &[
    ".wiki", ".git", "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache",
    "node_modules", "target",
];

const SENSITIVE_PATTERNS: &[(&str, &str)] = &[
    (r"(?i)(?:sk|pk|rk)-(?:[a-zA-Z0-9]{20,})", "[REDACTED: API key]"),
    (r"(?:ghp|gho|ghu|ghs|ghr)_[a-zA-Z0-9]{36,}", "[REDACTED: GitHub token]"),
    (r"(?s)-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----.*?-----END (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
     "[REDACTED: Private key]"),
    (r"(?i)password\s*[=:]\s*\S+", "password=[REDACTED]"),
    (r"[\w\.-]+@[\w\.-]+\.\w{2,}", "[REDACTED: Email]"),
];

pub fn is_text_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| TEXT_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn is_image_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn is_pdf_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| PDF_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn is_supported_source(path: &Path) -> bool {
    path.is_file() && (is_text_source(path) || is_image_source(path) || is_pdf_source(path))
}

pub fn iter_source_files(root: &Path, max_depth: Option<usize>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let walker = walkdir::WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && !SKIP_DIR_NAMES.contains(&name.as_ref())
        });
    for entry in walker {
        if let Ok(entry) = entry {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(max_d) = max_depth {
                if entry.depth() > max_d + 1 {
                    continue;
                }
            }
            let path = entry.into_path();
            if is_supported_source(&path) {
                files.push(path);
            }
        }
    }
    files
}

pub fn read_source_content(path: &Path) -> WikiResult<String> {
    if is_text_source(path) {
        fs::read_to_string(path).map_err(|e| e.into())
    } else if is_pdf_source(path) {
        read_pdf_source(path)
    } else if is_image_source(path) {
        read_image_source(path)
    } else {
        Err(WikiError::Parse(format!(
            "Unsupported file type: {}", path.display()
        )))
    }
}

fn read_pdf_source(path: &Path) -> WikiResult<String> {
    let path_str = path.to_string_lossy().to_string();
    let lp_cfg = crate::config::get_liteparse_config();

    // Try liteparse first
    let config = liteparse::LiteParseConfig {
        ocr_language: lp_cfg.ocr_language,
        ocr_enabled: false, // Force OCR off for reliability
        ocr_server_url: if lp_cfg.ocr_server_url.is_empty() { None } else { Some(lp_cfg.ocr_server_url) },
        dpi: lp_cfg.dpi,
        max_pages: lp_cfg.max_pages,
        num_workers: 1,
        ..Default::default()
    };

    let result = match pollster::block_on(liteparse::LiteParse::new(config).parse(&path_str)) {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("[pdf] liteparse failed: {e:?}, trying raw read...");
            None
        }
    };

    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let size = path.metadata().map(|m| m.len() / 1024).unwrap_or(0);
    let mut sections = vec![
        format!("# PDF Source: {file_name}"),
        String::new(),
        format!("> **Size**: {size} KB"),
        String::new(),
    ];

    if let Some(result) = result {
        sections.push(format!("> **Format**: PDF (text extracted via liteparse)"));
        sections.push(String::new());
        for page in &result.pages {
            if !page.text.trim().is_empty() {
                sections.push(format!("## Page {}", page.page_number));
                sections.push(String::new());
                sections.push(page.text.clone());
                sections.push(String::new());
            }
        }
    } else {
        // Fallback: extract raw text strings from PDF binary
        sections.push(format!("> **Format**: PDF (raw text extraction — liteparse failed)"));
        sections.push(String::new());
        let bytes = std::fs::read(path)?;
        let raw = String::from_utf8_lossy(&bytes);
        // Extract readable text between stream/endstream markers
        let re = regex::Regex::new(r"(?s)stream\s+(.*?)endstream").unwrap();
        let mut found = 0;
        for cap in re.captures_iter(&raw) {
            let content = &cap[1];
            // Filter printable ASCII
            let cleaned: String = content.chars().filter(|c| c.is_ascii_graphic() || c.is_ascii_whitespace()).collect();
            if cleaned.len() > 50 {
                sections.push(format!("## Extracted Block {}\n\n```\n{}\n```\n", found + 1, cleaned.trim()));
                found += 1;
            }
        }
        if found == 0 {
            return Err(WikiError::Parse(format!("Cannot extract text from PDF: {}", path.display())));
        }
    }
    Ok(sections.join("\n"))
}

fn read_image_source(path: &Path) -> WikiResult<String> {
    let image_config = crate::config::get_image_analysis_config();
    let mut analysis = String::new();
    let mut ocr_text = String::new();

    if image_config.enabled && !image_config.api_model.is_empty() {
        analysis = crate::ocr_api::analyze_image(path, &image_config)?;
    }
    if !image_config.enabled || analysis.is_empty() {
        if image_config.ocr_fallback {
            ocr_text = crate::ocr_api::ocr_image(path)?;
        }
    }
    if analysis.is_empty() && ocr_text.is_empty() {
        return Err(WikiError::Ocr(
            "Image compile requires image_analysis.enabled or OCR backend".into(),
        ));
    }

    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let size = path.metadata().map(|m| m.len() / 1024).unwrap_or(0);
    let mut sections = vec![
        format!("# Image Source: {file_name}"),
        String::new(),
        format!("> **Size**: {size} KB"),
        String::new(),
    ];
    if !analysis.is_empty() {
        sections.push("## Visual Analysis".into());
        sections.push(String::new());
        sections.push(analysis.trim().to_string());
        sections.push(String::new());
    }
    if !ocr_text.is_empty() {
        sections.push("## OCR Text".into());
        sections.push(String::new());
        sections.push(ocr_text.trim().to_string());
    }
    Ok(sections.join("\n"))
}

pub fn strip_sensitive(content: &str) -> String {
    let mut result = content.to_string();
    for (pattern, replacement) in SENSITIVE_PATTERNS {
        if let Ok(re) = regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .dot_matches_new_line(true)
            .build()
        {
            result = re.replace_all(&result, *replacement).to_string();
        }
    }
    result
}

pub fn ingest_rules(source_type: &SourceType) -> (Vec<&'static str>, &'static str) {
    match source_type {
        SourceType::Doc => (
            vec!["entity", "concept", "process", "rule", "role", "event"],
            "core concepts, named entities, processes, roles, rules, and events",
        ),
        SourceType::Article => (
            vec!["concept", "entity", "model", "technique", "benchmark", "paper"],
            "claims, concepts, models, techniques, benchmarks, and cited work",
        ),
        SourceType::Code => (
            vec!["entity", "concept", "framework", "tool", "file", "library", "decision"],
            "source files, libraries, tools, architectural decisions, and implementation patterns",
        ),
        SourceType::Conversation => (
            vec!["decision", "concept", "entity", "process", "rule"],
            "decisions, findings, open questions, rules, and follow-up actions",
        ),
    }
}

pub fn compile_source(
    source: &Path,
    source_type: &SourceType,
    _force: bool,
    dry_run: bool,
) -> WikiResult<CompileResult> {
    let wiki_dir = get_wiki_dir();
    let relative = source.to_string_lossy().to_string();
    let mut result = CompileResult {
        source: relative.clone(),
        pages_created: 0,
        pages_updated: 0,
        entities_added: 0,
        edges_added: 0,
        errors: Vec::new(),
    };

    let raw_content = match read_source_content(source) {
        Ok(c) => c,
        Err(e) => {
            result.errors.push(format!("{e}"));
            return Ok(result);
        }
    };

    let content = strip_sensitive(&raw_content);
    let lang = llm::detect_language(&content);
    let (entity_types, focus_description) = ingest_rules(source_type);
    let (system_prompt, user_prompt) =
        build_compile_prompt(&content, lang, &entity_types, focus_description, source_type);

    if dry_run {
        println!("[DRY-RUN] Would compile: {}", source.display());
        println!("  Length: {} chars, Language: {lang}", content.len());
        return Ok(result);
    }

    let response = llm::call_llm_default(&system_prompt, &user_prompt)?;
    let pages = parse_compile_response(&response, lang)?;

    // Dedup: remove pages with duplicate IDs (LLM sometimes outputs same entity twice)
    let mut seen_ids = std::collections::HashSet::new();
    let unique_pages: Vec<&crate::compile_parse::ParsedPage> = pages.iter()
        .filter(|p| seen_ids.insert(p.id.clone()))
        .collect();
    if unique_pages.len() < pages.len() {
        eprintln!("Dedup: {} duplicates removed", pages.len() - unique_pages.len());
    }

    let mut page_ids: Vec<String> = Vec::new();
    for page in &unique_pages {
        let page_path = write_wiki_page(page, &wiki_dir)?;
        result.pages_created += 1;
        page_ids.push(page.id.clone());
        if let Err(e) = graph::add_entity_from_page(page, &page_path) {
            result.errors.push(format!("Graph entity: {e}"));
        }
    }

    // DIRECT edge creation: connect all entities from this source
    if page_ids.len() >= 2 {
        match graph::connect_entities(&page_ids, "same_source") {
            Ok(_) => result.edges_added = (page_ids.len() * (page_ids.len() - 1)) / 2,
            Err(e) => result.errors.push(format!("Graph edges: {e}")),
        }
    }

    // Merge similar concepts (e.g., "AI Agent" and "Agent")
    if let Ok(merged) = graph::merge_similar_pages(&wiki_dir) {
        if merged > 0 {
            eprintln!("Merged {} similar pages", merged);
        }
    }

    if let Err(e) = update_index(&wiki_dir) {
        result.errors.push(format!("Index: {e}"));
    }
    if let Err(e) = write_audit(&wiki_dir, &relative, result.pages_created) {
        result.errors.push(format!("Audit: {e}"));
    }
    Ok(result)
}

fn write_wiki_page(page: &crate::compile_parse::ParsedPage, wiki_dir: &Path) -> WikiResult<PathBuf> {
    let subdir = match page.page_type {
        PageType::Entity => "entities",
        PageType::Concept => "concepts",
    };
    let dir = wiki_dir.join("pages").join(subdir);
    fs::create_dir_all(&dir)?;
    let filename = format!("{}.md", page.id);
    let filepath = dir.join(&filename);

    if filepath.exists() {
        // MERGE: update existing page instead of overwriting
        let existing = fs::read_to_string(&filepath)?;
        let (old_fm, old_body) = split_page(&existing);

        // Merge frontmatter: new values override, confidence takes max
        let mut merged_fm = old_fm;
        for (k, v) in &page.frontmatter {
            if k == "confidence" {
                let old_c = merged_fm.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
                let new_c = v.as_f64().unwrap_or(0.0);
                if new_c > old_c {
                    merged_fm.insert(k.clone(), serde_json::json!(new_c));
                }
            } else if k == "aliases" || k == "keywords" || k == "facts" {
                // Merge lists
                let mut set: std::collections::HashSet<String> = as_string_set(merged_fm.get(k));
                for item in as_string_set(Some(v)) { set.insert(item); }
                let merged: Vec<serde_json::Value> = set.into_iter().map(serde_json::Value::String).collect();
                merged_fm.insert(k.clone(), serde_json::Value::Array(merged));
            } else if k != "id" {
                merged_fm.insert(k.clone(), v.clone());
            }
        }

        let merged_fm_str = serde_yaml::to_string(&merged_fm).unwrap_or_default();
        let merged_body = merge_bodies(&old_body, &page.body);
        let output = format!("---\n{}---\n\n{}", merged_fm_str, merged_body);
        fs::write(&filepath, &output)?;
    } else {
        // NEW page
        let mut output = String::from("---\n");
        if let Ok(fm_str) = serde_yaml::to_string(&page.frontmatter) {
            output.push_str(&fm_str);
        }
        output.push_str("---\n\n");
        output.push_str(&page.body);
        fs::write(&filepath, &output)?;
    }
    Ok(filepath)
}

/// Split a page into (frontmatter_json, body_string).
fn split_page(content: &str) -> (HashMap<String, serde_json::Value>, String) {
    if content.len() < 8 || !content.starts_with("---\n") { return (HashMap::new(), content.to_string()); }
    if let Some(end) = content[4..].find("\n---") {
        let fm = content[4..4+end].trim();
        let body = content[4+end+4..].trim().to_string();
        let fm_map = serde_yaml::from_str::<serde_yaml::Value>(fm).ok()
            .and_then(|v| {
                let mut m = HashMap::new();
                if let Some(obj) = v.as_mapping() {
                    for (k, val) in obj {
                        let key = k.as_str().unwrap_or("").to_string();
                        m.insert(key, crate::compile_parse::yaml_to_json(val.clone()));
                    }
                }
                Some(m)
            })
            .unwrap_or_default();
        (fm_map, body)
    } else {
        (HashMap::new(), content.to_string())
    }
}

fn as_string_set(val: Option<&serde_json::Value>) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    if let Some(v) = val {
        if let Some(arr) = v.as_array() {
            for item in arr { if let Some(s) = item.as_str() { set.insert(s.to_string()); } }
        } else if let Some(s) = v.as_str() { set.insert(s.to_string()); }
    }
    set
}

/// Merge bodies: keep old content, only append new sections that don't already appear.
fn merge_bodies(old_body: &str, new_body: &str) -> String {
    let old = old_body.trim();
    let new = new_body.trim();
    // If new contains old (re-compile yielded same content), return new as authoritative
    if new.contains(old) { return new.to_string(); }
    // If old already contains new, no change needed
    if old.contains(new) { return old.to_string(); }
    // Otherwise append with separator
    format!("{old}\n\n<!-- merged -->\n\n{new}")
}

fn update_index(wiki_dir: &Path) -> WikiResult<()> {
    let pages_dir = wiki_dir.join("pages");
    let mut index = String::from("# Wiki Index\n\n");
    for (subdir, label) in &[("concepts", "Concepts"), ("entities", "Entities")] {
        let dir = pages_dir.join(subdir);
        if !dir.exists() {
            continue;
        }
        index.push_str(&format!("## {label}\n\n"));
        let mut entries: Vec<(String, String)> = Vec::new();
        if let Ok(iter) = fs::read_dir(&dir) {
            for entry in iter.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                    let content = fs::read_to_string(&path).unwrap_or_default();
                    let title = extract_frontmatter_field(&content, "name")
                        .unwrap_or_else(|| name.clone());
                    entries.push((name, title));
                }
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (slug, title) in &entries {
            index.push_str(&format!("- [[{slug}]] — {title}\n"));
        }
        index.push('\n');
    }
    fs::write(pages_dir.join("index.md"), &index)?;
    Ok(())
}

pub fn extract_frontmatter_field(content: &str, key: &str) -> Option<String> {
    if content.len() < 8 || !content.starts_with("---\n") { return None; }
    let end = content[4..].find("\n---").map(|i| i + 4)?;
    let fm = &content[4..end];
    for line in fm.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == key {
                return Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
            }
        }
    }
    None
}

fn write_audit(wiki_dir: &Path, source: &str, pages_created: usize) -> WikiResult<()> {
    let audit_dir = wiki_dir.join("audit");
    fs::create_dir_all(&audit_dir)?;
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let audit_file = audit_dir.join(format!("compile-{timestamp}.json"));
    let entry = serde_json::json!({
        "type": "compile",
        "source": source,
        "pages_created": pages_created,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    fs::write(&audit_file, serde_json::to_string_pretty(&entry)?)?;

    let log_file = wiki_dir.join("log.md");
    let log_entry = format!(
        "\n## [{}] compile | {} → {} pages\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        source,
        pages_created
    );
    let mut log = fs::read_to_string(&log_file).unwrap_or_default();
    log.push_str(&log_entry);
    fs::write(&log_file, &log)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_sensitive_api_key() {
        let result = strip_sensitive("API key: sk-abc123def456ghijklmnopqrstuvwx");
        assert!(result.contains("[REDACTED: API key]"));
        assert!(!result.contains("sk-abc123"));
    }

    #[test]
    fn test_is_text_source() {
        assert!(is_text_source(Path::new("test.md")));
        assert!(is_text_source(Path::new("test.rs")));
        assert!(!is_text_source(Path::new("test.png")));
    }

    #[test]
    fn test_is_pdf_source() {
        assert!(is_pdf_source(Path::new("test.pdf")));
        assert!(!is_pdf_source(Path::new("test.md")));
    }
}
