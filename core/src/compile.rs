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
use crate::config::{get_config, get_wiki_dir};
use crate::error::{WikiError, WikiResult};
use crate::graph;
use crate::llm;
use crate::table_extract;
use crate::types::{CompileResult, PageType, SourceType};

const TEXT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "rst", "adoc", "csv", "tsv", "json", "jsonl", "yaml", "yml", "html",
    "htm", "xml", "svg", "py", "js", "ts", "jsx", "tsx", "go", "rs", "java", "c", "cc", "cpp", "h",
    "hpp", "cs", "php", "rb", "sh", "bash", "zsh", "sql", "toml", "ini", "cfg",
];

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff", "tif", "avif", "heic", "heif",
];

const PDF_EXTENSIONS: &[&str] = &["pdf"];
const TABLE_EXTENSIONS: &[&str] = &["csv", "tsv", "json", "xlsx", "xls"];

const SKIP_DIR_NAMES: &[&str] = &[
    ".wiki",
    ".git",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    "node_modules",
    "target",
];

const SENSITIVE_PATTERNS: &[(&str, &str)] = &[
    (
        r"(?i)(?:sk|pk|rk)-(?:[a-zA-Z0-9]{20,})",
        "[REDACTED: API key]",
    ),
    (
        r"(?:ghp|gho|ghu|ghs|ghr)_[a-zA-Z0-9]{36,}",
        "[REDACTED: GitHub token]",
    ),
    (
        r"(?s)-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----.*?-----END (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
        "[REDACTED: Private key]",
    ),
    (r"(?i)password\s*[=:]\s*\S+", "password=[REDACTED]"),
    (r"[\w\.-]+@[\w\.-]+\.\w{2,}", "[REDACTED: Email]"),
];

#[derive(Debug, Clone)]
pub struct CompilePrompt {
    pub system_prompt: String,
    pub user_prompt: String,
    pub language: String,
    pub content_len: usize,
}

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
    path.is_file()
        && (is_text_source(path)
            || is_image_source(path)
            || is_pdf_source(path)
            || is_table_source(path))
}

pub fn is_table_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| TABLE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
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
            "Unsupported file type: {}",
            path.display()
        )))
    }
}

fn read_pdf_source(path: &Path) -> WikiResult<String> {
    let path_str = path.to_string_lossy().to_string();
    let lp_cfg = crate::config::get_liteparse_config();
    let ocr_cfg = crate::config::get_ocr_config();

    // Try liteparse first
    let config = liteparse::LiteParseConfig {
        ocr_language: lp_cfg.ocr_language,
        ocr_enabled: lp_cfg.ocr_enabled,
        ocr_server_url: if lp_cfg.ocr_server_url.is_empty() {
            None
        } else {
            Some(lp_cfg.ocr_server_url.clone())
        },
        dpi: lp_cfg.dpi,
        max_pages: lp_cfg.max_pages,
        num_workers: if lp_cfg.num_workers == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get().saturating_sub(1).max(1))
                .unwrap_or(1)
        } else {
            lp_cfg.num_workers
        },
        ..Default::default()
    };

    let mut parser = liteparse::LiteParse::new(config);
    if lp_cfg.ocr_enabled && lp_cfg.ocr_server_url.is_empty() {
        let engine = crate::local_ocr::LocalOcrEngine::new(ocr_cfg.into());
        parser = parser.with_ocr_engine(std::sync::Arc::new(engine));
    }

    let result = match pollster::block_on(parser.parse(&path_str)) {
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
        sections.push(format!(
            "> **Format**: PDF (raw text extraction — liteparse failed)"
        ));
        sections.push(String::new());
        let bytes = std::fs::read(path)?;
        let raw = String::from_utf8_lossy(&bytes);
        // Extract readable text between stream/endstream markers
        let re = regex::Regex::new(r"(?s)stream\s+(.*?)endstream").unwrap();
        let mut found = 0;
        for cap in re.captures_iter(&raw) {
            let content = &cap[1];
            // Filter printable ASCII
            let cleaned: String = content
                .chars()
                .filter(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
                .collect();
            if cleaned.len() > 50 {
                sections.push(format!(
                    "## Extracted Block {}\n\n```\n{}\n```\n",
                    found + 1,
                    cleaned.trim()
                ));
                found += 1;
            }
        }
        if found == 0 {
            return Err(WikiError::Parse(format!(
                "Cannot extract text from PDF: {}",
                path.display()
            )));
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
            vec![
                "concept",
                "entity",
                "model",
                "technique",
                "benchmark",
                "paper",
            ],
            "claims, concepts, models, techniques, benchmarks, and cited work",
        ),
        SourceType::Code => (
            vec![
                "entity",
                "concept",
                "framework",
                "tool",
                "file",
                "library",
                "decision",
            ],
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
    let mut result = CompileResult {
        source: source.to_string_lossy().to_string(),
        pages_created: 0,
        pages_updated: 0,
        entities_added: 0,
        edges_added: 0,
        errors: Vec::new(),
    };

    if is_table_source(source) {
        if dry_run {
            println!(
                "[DRY-RUN] Would import structured table: {}",
                source.display()
            );
            return Ok(result);
        }
        return import_structured_source(source);
    }

    let raw_content = match read_source_content(source) {
        Ok(content) => content,
        Err(e) => {
            result.errors.push(format!("{e}"));
            return Ok(result);
        }
    };
    let mut content = if get_config().compile.strip_sensitive {
        strip_sensitive(&raw_content)
    } else {
        raw_content
    };
    if !dry_run && is_markdown_source(source) {
        let mut table_errors = Vec::new();
        content = table_extract::extract_large_tables_to_links(&content, source, &mut table_errors);
        result.errors.extend(table_errors);
    }

    let prompt = match compile_prompt_from_content(source_type, content) {
        Ok(p) => p,
        Err(e) => {
            result.errors.push(format!("{e}"));
            return Ok(result);
        }
    };

    if dry_run {
        println!("[DRY-RUN] Would compile: {}", source.display());
        println!(
            "  Length: {} chars, Language: {}",
            prompt.content_len, prompt.language
        );
        return Ok(result);
    }

    let response = llm::call_llm_default(&prompt.system_prompt, &prompt.user_prompt)?;
    apply_compile_response(source, source_type, &response, &prompt.language)
}

pub fn compile_prompt_for_source(
    source: &Path,
    source_type: &SourceType,
) -> WikiResult<CompilePrompt> {
    let raw_content = read_source_content(source)?;
    let mut content = if get_config().compile.strip_sensitive {
        strip_sensitive(&raw_content)
    } else {
        raw_content
    };
    if is_markdown_source(source) {
        let mut table_errors = Vec::new();
        content = table_extract::extract_large_tables_to_links(&content, source, &mut table_errors);
        if !table_errors.is_empty() {
            eprintln!("{}", table_errors.join("\n"));
        }
    }
    compile_prompt_from_content(source_type, content)
}

fn compile_prompt_from_content(
    source_type: &SourceType,
    content: String,
) -> WikiResult<CompilePrompt> {
    let lang = llm::detect_language(&content);
    let (entity_types, focus_description) = ingest_rules(source_type);
    let (base_system_prompt, user_prompt) = build_compile_prompt(
        &content,
        lang,
        &entity_types,
        focus_description,
        source_type,
    );
    let system_prompt = with_compile_schema(base_system_prompt);
    Ok(CompilePrompt {
        system_prompt,
        user_prompt,
        language: lang.to_string(),
        content_len: content.len(),
    })
}

fn is_markdown_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_lowercase().as_str(), "md" | "markdown" | "mdown"))
        .unwrap_or(false)
}

fn import_structured_source(source: &Path) -> WikiResult<CompileResult> {
    let source_name = source
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let path = source.to_string_lossy();
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let msg = match ext.as_str() {
        "csv" | "tsv" => crate::ledger::import_csv(&path, Some(&source_name))?,
        "json" => crate::ledger::import_json(&path, Some(&source_name))?,
        "xlsx" | "xls" => crate::ledger::import_excel(&path, Some(&source_name))?,
        _ => {
            return Err(WikiError::Parse(format!(
                "Unsupported table source: {}",
                source.display()
            )))
        }
    };
    eprintln!("Table imported: {msg}");
    Ok(CompileResult {
        source: source.to_string_lossy().to_string(),
        pages_created: 1,
        pages_updated: 0,
        entities_added: 0,
        edges_added: 0,
        errors: Vec::new(),
    })
}

fn with_compile_schema(system_prompt: String) -> String {
    let schema = load_compile_schema();
    format!(
        "{system_prompt}\n\n## Wiki Schema and Compile Policy\n\
The following schema is part of the compile contract. Follow it together with the output template above. \
Use its entity rules, relationship rules, ingest rules, quality standards, privacy rules, and required fields when producing pages.\n\n\
```markdown\n{schema}\n```"
    )
}

fn load_compile_schema() -> String {
    let schema_path = get_wiki_dir().join("schema.md");
    std::fs::read_to_string(&schema_path)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| include_str!("../templates/schema.md").to_string())
}

pub fn apply_compile_response(
    source: &Path,
    source_type: &SourceType,
    response: &str,
    lang: &str,
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

    let pages = parse_compile_response(response, lang)?;

    // Dedup: remove pages with duplicate IDs (LLM sometimes outputs same entity twice)
    let mut seen_ids = std::collections::HashSet::new();
    let unique_pages: Vec<&crate::compile_parse::ParsedPage> = pages
        .iter()
        .filter(|p| seen_ids.insert(p.id.clone()))
        .collect();
    if unique_pages.len() < pages.len() {
        eprintln!(
            "Dedup: {} duplicates removed",
            pages.len() - unique_pages.len()
        );
    }

    // Clone into mutable pages so we can replace tables with links
    let mut final_pages: Vec<crate::compile_parse::ParsedPage> =
        unique_pages.iter().map(|p| (*p).clone()).collect();
    for page in &mut final_pages {
        annotate_page_source(page, source, source_type);
    }

    // Extract and store tables from each page body, replace with [[table:xxx]] links
    let mut total_tables = 0usize;
    for page in &mut final_pages {
        let tables = table_extract::extract_tables(&page.body);
        if tables.is_empty() {
            continue;
        }
        for table in &tables {
            let table_name = format!("{}_{}", page.id, total_tables);
            match table_extract::store_table(&table_name, &table.headers, &table.rows) {
                Ok(name) => {
                    let link = table_extract::table_link(&name, &table.headers);
                    page.body = page.body.replacen(&table.raw, &link, 1);
                    total_tables += 1;
                }
                Err(e) => {
                    result
                        .errors
                        .push(format!("Table storage '{}': {e}", table_name));
                }
            }
        }
    }
    if total_tables > 0 {
        eprintln!("Tables extracted: {} tables stored in DuckDB", total_tables);
    }

    let mut page_ids: Vec<String> = Vec::new();
    for page in &final_pages {
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

fn write_wiki_page(
    page: &crate::compile_parse::ParsedPage,
    wiki_dir: &Path,
) -> WikiResult<PathBuf> {
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
            } else if k == "aliases" || k == "keywords" || k == "facts" || k == "source_files" {
                // Merge lists
                let mut set: std::collections::HashSet<String> = as_string_set(merged_fm.get(k));
                for item in as_string_set(Some(v)) {
                    set.insert(item);
                }
                let merged: Vec<serde_json::Value> =
                    set.into_iter().map(serde_json::Value::String).collect();
                merged_fm.insert(k.clone(), serde_json::Value::Array(merged));
            } else if k == "source_refs" {
                let merged = merge_source_refs(merged_fm.get(k), v);
                merged_fm.insert(k.clone(), serde_json::Value::Array(merged));
            } else if k == "source" {
                let mut set = as_string_set(merged_fm.get("source_files"));
                if let Some(s) = merged_fm.get("source").and_then(|x| x.as_str()) {
                    set.insert(s.to_string());
                }
                if let Some(s) = v.as_str() {
                    set.insert(s.to_string());
                }
                let merged: Vec<serde_json::Value> =
                    set.into_iter().map(serde_json::Value::String).collect();
                merged_fm.insert("source_files".to_string(), serde_json::Value::Array(merged));
            } else if k != "id" {
                merged_fm.insert(k.clone(), v.clone());
            }
        }

        let merged_fm_str = serde_yaml::to_string(&merged_fm).unwrap_or_default();
        let merged_body = merge_bodies(&old_body, &page.body, &source_label(page));
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

fn annotate_page_source(
    page: &mut crate::compile_parse::ParsedPage,
    source: &Path,
    source_type: &SourceType,
) {
    let source_path = source.to_string_lossy().to_string();
    let source_name = source
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    page.frontmatter.insert(
        "source".to_string(),
        serde_json::Value::String(source_path.clone()),
    );
    page.frontmatter.insert(
        "source_name".to_string(),
        serde_json::Value::String(source_name.clone()),
    );
    page.frontmatter.insert(
        "source_type".to_string(),
        serde_json::Value::String(source_type.as_str().to_string()),
    );
    page.frontmatter.insert(
        "source_files".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::String(source_path.clone())]),
    );
    page.frontmatter.insert(
        "source_refs".to_string(),
        serde_json::Value::Array(vec![serde_json::json!({
            "path": source_path,
            "name": source_name,
            "type": source_type.as_str(),
        })]),
    );
}

/// Split a page into (frontmatter_json, body_string).
fn split_page(content: &str) -> (HashMap<String, serde_json::Value>, String) {
    if content.len() < 8 || !content.starts_with("---\n") {
        return (HashMap::new(), content.to_string());
    }
    if let Some(end) = content[4..].find("\n---") {
        let fm = content[4..4 + end].trim();
        let body = content[4 + end + 4..].trim().to_string();
        let fm_map = serde_yaml::from_str::<serde_yaml::Value>(fm)
            .ok()
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
            for item in arr {
                if let Some(s) = item.as_str() {
                    set.insert(s.to_string());
                }
            }
        } else if let Some(s) = v.as_str() {
            set.insert(s.to_string());
        }
    }
    set
}

fn merge_source_refs(
    old: Option<&serde_json::Value>,
    new: &serde_json::Value,
) -> Vec<serde_json::Value> {
    let mut refs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for value in old.into_iter().chain(std::iter::once(new)) {
        match value {
            serde_json::Value::Array(items) => {
                for item in items {
                    push_unique_source_ref(item.clone(), &mut refs, &mut seen);
                }
            }
            serde_json::Value::String(s) => {
                push_unique_source_ref(
                    serde_json::json!({ "path": s, "name": s }),
                    &mut refs,
                    &mut seen,
                );
            }
            serde_json::Value::Object(_) => {
                push_unique_source_ref(value.clone(), &mut refs, &mut seen);
            }
            _ => {}
        }
    }
    refs
}

fn push_unique_source_ref(
    value: serde_json::Value,
    refs: &mut Vec<serde_json::Value>,
    seen: &mut std::collections::HashSet<String>,
) {
    let key = value
        .get("path")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("name").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| value.to_string());
    if seen.insert(key) {
        refs.push(value);
    }
}

fn source_label(page: &crate::compile_parse::ParsedPage) -> String {
    page.frontmatter
        .get("source")
        .and_then(|v| v.as_str())
        .or_else(|| page.frontmatter.get("source_name").and_then(|v| v.as_str()))
        .unwrap_or("unknown-source")
        .to_string()
}

/// Merge bodies by section and fuse similar lines instead of appending whole duplicate blocks.
fn merge_bodies(old_body: &str, new_body: &str, source: &str) -> String {
    let old = old_body.trim();
    let new = new_body.trim();
    if old == new {
        return new.to_string();
    }
    // If old already contains new, no change needed
    if old.contains(new) {
        return old.to_string();
    }
    if old.contains(&format!("<!-- source-ref:{source} -->")) {
        return old.to_string();
    }
    merge_body_sections(old, new)
}

fn merge_body_sections(left: &str, right: &str) -> String {
    let mut sections = split_body_sections(left);
    for (heading, lines) in split_body_sections(right) {
        let entry = sections.entry(heading).or_default();
        for line in lines {
            insert_fused_body_line(entry, line);
        }
    }
    render_body_sections(sections)
}

fn split_body_sections(body: &str) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut sections = std::collections::BTreeMap::new();
    let mut current = "概述".to_string();
    for raw in clean_merge_artifacts(body).lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(heading) = canonical_body_heading(line) {
            current = heading;
            sections.entry(current.clone()).or_insert_with(Vec::new);
            continue;
        }
        insert_fused_body_line(
            sections.entry(current.clone()).or_insert_with(Vec::new),
            line.to_string(),
        );
    }
    sections
}

fn canonical_body_heading(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_start_matches('#').trim();
    match trimmed {
        "概述" | "Overview" | "Summary" => Some("概述".into()),
        "关键细节" | "Details" | "Key Details" => Some("关键细节".into()),
        "关系" | "Relationships" => Some("关系".into()),
        "来源" | "Source" | "Sources" => Some("来源".into()),
        "Source-Specific Notes" => None,
        _ if line.starts_with('#') => Some(trimmed.to_string()),
        _ => None,
    }
}

fn render_body_sections(sections: std::collections::BTreeMap<String, Vec<String>>) -> String {
    let order = ["概述", "关键细节", "关系", "来源"];
    let mut out = String::new();
    let mut rendered = std::collections::HashSet::new();
    for heading in order {
        if let Some(lines) = sections.get(heading) {
            push_body_section(&mut out, heading, lines);
            rendered.insert(heading.to_string());
        }
    }
    for (heading, lines) in sections {
        if !rendered.contains(&heading) {
            push_body_section(&mut out, &heading, &lines);
        }
    }
    out.trim().to_string()
}

fn push_body_section(out: &mut String, heading: &str, lines: &[String]) {
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

fn insert_fused_body_line(lines: &mut Vec<String>, candidate: String) {
    let candidate_norm = normalize_body_line(&candidate);
    if candidate_norm.is_empty() {
        return;
    }
    for existing in lines.iter_mut() {
        let existing_norm = normalize_body_line(existing);
        if existing_norm == candidate_norm || similar_body_line(&existing_norm, &candidate_norm) {
            if candidate.chars().count() > existing.chars().count() {
                *existing = candidate;
            }
            return;
        }
    }
    lines.push(candidate);
}

fn clean_merge_artifacts(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("<!-- merged")
                && !trimmed.starts_with("<!-- source-ref:")
                && !trimmed.starts_with("_Source:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_body_line(line: &str) -> String {
    line.to_lowercase()
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && !c.is_ascii_punctuation()
                && !"，。；：、“”‘’（）【】《》—".contains(*c)
        })
        .collect()
}

fn similar_body_line(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.contains(b) || b.contains(a) {
        return true;
    }
    let grams_a = body_bigrams(a);
    let grams_b = body_bigrams(b);
    if grams_a.is_empty() || grams_b.is_empty() {
        return false;
    }
    let intersection = grams_a.intersection(&grams_b).count() as f64;
    let union = grams_a.union(&grams_b).count() as f64;
    intersection / union >= 0.42
}

fn body_bigrams(text: &str) -> std::collections::HashSet<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return chars.into_iter().map(|c| c.to_string()).collect();
    }
    chars
        .windows(2)
        .map(|w| w.iter().collect::<String>())
        .collect()
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
                    let name = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let content = fs::read_to_string(&path).unwrap_or_default();
                    let title =
                        extract_frontmatter_field(&content, "name").unwrap_or_else(|| name.clone());
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
    if content.len() < 8 || !content.starts_with("---\n") {
        return None;
    }
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

    #[test]
    fn test_merge_bodies_fuses_sections_without_append_markers() {
        let merged = merge_bodies(
            "概述\n\nAI 最佳实践强调人工审核和合规检查。\n\n关键细节\n\n人工审核：所有 AI 产出物必须经过人工审核。",
            "概述\n\nAI 最佳实践强调人工审核、合规检查和质量把控。\n\n关键细节\n\n合规检查：建立合规检查清单。",
            "clients/acme.md",
        );
        assert!(merged.contains("人工审核"));
        assert!(merged.contains("合规检查"));
        assert!(!merged.contains("Source-Specific Notes"));
        assert!(!merged.contains("source-ref"));
    }

    #[test]
    fn test_merge_source_refs_deduplicates_by_path() {
        let old = serde_json::json!([{ "path": "a.md", "name": "a.md" }]);
        let new = serde_json::json!([
            { "path": "a.md", "name": "a copy.md" },
            { "path": "b.md", "name": "b.md" }
        ]);
        let merged = merge_source_refs(Some(&old), &new);
        assert_eq!(merged.len(), 2);
    }
}
