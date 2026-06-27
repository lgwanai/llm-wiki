//! Tauri commands bridging llm-wiki-core to the frontend.

use std::sync::Mutex;

use llm_wiki_core::config::{self, reset_config};
use llm_wiki_core::graph;
use llm_wiki_core::search;
use llm_wiki_core::types::WikiStatus;
use std::collections::HashMap;
use tauri::{Emitter, Manager, WebviewWindowBuilder};

/// Application state managed by Tauri.
pub struct AppState {
    pub project_path: Mutex<Option<String>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            project_path: Mutex::new(None),
        }
    }
}

#[tauri::command]
pub fn set_project_path(path: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    std::env::set_var("LLM_WIKI_PROJECT_DIR", &path);
    reset_config();
    *state.project_path.lock().map_err(|e| e.to_string())? = Some(path);
    Ok(())
}

#[tauri::command]
pub fn get_wiki_status() -> Result<WikiStatus, String> {
    let wiki_dir = config::get_wiki_dir();
    let pages_dir = wiki_dir.join("pages");
    let concepts = count_md(&pages_dir.join("concepts"));
    let entities = count_md(&pages_dir.join("entities"));
    let graph_dir = wiki_dir.join("graph");
    let ge = count_json_entities(&graph_dir.join("entities.json"));
    let gd = count_json_edges(&graph_dir.join("edges.json"));

    Ok(WikiStatus {
        pages: llm_wiki_core::types::PageStatus {
            concepts,
            entities,
            total: concepts + entities,
        },
        graph: llm_wiki_core::types::GraphStatus {
            entities: ge,
            edges: gd,
        },
        files: llm_wiki_core::types::FileStatus {
            index: pages_dir.join("index.md").exists(),
            log: wiki_dir.join("log.md").exists(),
            audit: wiki_dir.join("audit").exists(),
        },
    })
}

#[tauri::command]
pub fn get_wiki_pages() -> Result<Vec<serde_json::Value>, String> {
    let pages_dir = config::get_pages_dir();
    let mut results = Vec::new();
    for subdir in &["concepts", "entities"] {
        let dir = pages_dir.join(subdir);
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let id = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let name = llm_wiki_core::compile::extract_frontmatter_field(&content, "name")
                    .unwrap_or_else(|| id.clone());
                let etype = llm_wiki_core::compile::extract_frontmatter_field(&content, "type")
                    .unwrap_or_else(|| "entity".to_string());
                results.push(serde_json::json!({
                    "id": id, "name": name, "type": etype,
                    "path": path.to_string_lossy().to_string(),
                }));
            }
        }
    }
    results.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(results)
}

#[tauri::command]
pub fn get_page_content(page_id: String) -> Result<String, String> {
    let pages_dir = config::get_pages_dir();
    for subdir in &["concepts", "entities"] {
        let path = pages_dir.join(subdir).join(format!("{page_id}.md"));
        if path.exists() {
            return std::fs::read_to_string(&path).map_err(|e| e.to_string());
        }
    }
    Err(format!("Page not found: {page_id}"))
}

#[tauri::command]
pub fn get_source_file_content(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    validate_source_path(&path, &state)?;
    std::fs::read_to_string(&path).map_err(|e| format!("Read source file failed: {e}"))
}

#[tauri::command]
pub fn save_source_file_content(
    path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    validate_source_path(&path, &state)?;
    std::fs::write(&path, content).map_err(|e| format!("Save source file failed: {e}"))
}

fn validate_source_path(path: &str, state: &tauri::State<'_, AppState>) -> Result<(), String> {
    let requested = std::path::Path::new(path)
        .canonicalize()
        .map_err(|e| format!("Invalid source path: {e}"))?;
    let project = state
        .project_path
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "No workspace is open".to_string())?;
    let project = std::path::Path::new(&project)
        .canonicalize()
        .map_err(|e| format!("Invalid workspace path: {e}"))?;
    if requested.starts_with(project) {
        Ok(())
    } else {
        Err("Source file is outside the current workspace".into())
    }
}

#[tauri::command]
pub fn get_graph_data() -> Result<serde_json::Value, String> {
    let entities = graph::load_entities();
    let edges = graph::load_edges();

    let nodes: Vec<serde_json::Value> = entities
        .iter()
        .map(|(id, e)| {
            serde_json::json!({
                "id": id,
                "name": e.name,
                "type": e.entity_type,
                "confidence": e.confidence,
            })
        })
        .collect();

    let links: Vec<serde_json::Value> = edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "source": e.source,
                "target": e.target,
                "type": e.rel_type,
                "description": e.description,
            })
        })
        .collect();

    Ok(serde_json::json!({ "nodes": nodes, "edges": links }))
}

#[tauri::command]
pub fn search_wiki(query: String) -> Result<Vec<serde_json::Value>, String> {
    let streams: std::collections::HashSet<String> = ["metadata", "bm25", "graph"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let results = search::search(&query, &streams, 10);
    Ok(results
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id, "title": r.title, "score": r.rrf_score.unwrap_or(r.score),
                "path": r.path.to_string_lossy(),
                "entity_type": r.entity_type, "summary": r.summary,
            })
        })
        .collect())
}

#[tauri::command]
pub fn run_lint() -> Result<String, String> {
    llm_wiki_core::lint::run_lint(false).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_file(source: String, dest_dir: String) -> Result<String, String> {
    let src = std::path::Path::new(&source);
    let name = src
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let dest = std::path::Path::new(&dest_dir).join(&name);
    std::fs::copy(&source, &dest).map_err(|e| format!("Copy failed: {e}"))?;
    Ok(name)
}

#[tauri::command]
pub fn list_source_files(root: String) -> Result<Vec<serde_json::Value>, String> {
    // Load compile state: path -> (mtime, pages_created)
    let state_path = llm_wiki_core::config::get_wiki_dir().join("compile_state.json");
    let compile_state: HashMap<String, serde_json::Value> = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let root_path = std::path::Path::new(&root);
    let mut files = Vec::new();
    gather_files(root_path, root_path, "", &compile_state, 0, &mut files);
    files.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(files)
}

fn gather_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    prefix: &str,
    state: &HashMap<String, serde_json::Value>,
    depth: usize,
    files: &mut Vec<serde_json::Value>,
) {
    if depth > 2 {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name: String = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into();
            if name.starts_with('.')
                || name == "node_modules"
                || name == ".wiki"
                || name == "target"
            {
                continue;
            }
            if path.is_dir() {
                let new_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                gather_files(root, &path, &new_prefix, state, depth + 1, files);
            } else if path.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ![
                    "md", "markdown", "mdown", "txt", "pdf", "png", "jpg", "jpeg", "svg", "py",
                    "rs", "js", "ts", "json", "csv", "tsv", "yaml", "toml", "html", "xlsx", "xls",
                ]
                .contains(&ext)
                {
                    continue;
                }
                let path_str = path.to_string_lossy().to_string();
                let display = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                let meta = std::fs::metadata(&path).ok();
                let mtime = meta
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    })
                    .unwrap_or(0);
                // Determine status: check stored state
                let stored = state.get(&path_str);
                let stored_mtime = stored
                    .and_then(|v| v.get("mtime"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let stored_pages = stored
                    .and_then(|v| v.get("pages"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                let is_done = stored_mtime == mtime && stored_pages > 0;
                let (status, pages) = if is_done {
                    ("done", stored_pages)
                } else {
                    ("pending", 0usize)
                };
                files.push(serde_json::json!({
                    "path": path_str, "name": display, "status": status, "pages": pages,
                }));
            }
        }
    }
}

/// Save compile state after a successful compile.
pub fn save_compile_state(path: &str, pages: usize) {
    let state_path = llm_wiki_core::config::get_wiki_dir().join("compile_state.json");
    let mut state: HashMap<String, serde_json::Value> = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
        .unwrap_or(0);
    state.insert(
        path.to_string(),
        serde_json::json!({"mtime": mtime, "pages": pages}),
    );
    if let Some(p) = state_path.parent() {
        if let Err(e) = std::fs::create_dir_all(p) {
            eprintln!("[compile] failed to create state dir: {e}");
        }
    }
    if let Err(e) = std::fs::write(
        &state_path,
        serde_json::to_string_pretty(&state).unwrap_or_else(|e| {
            eprintln!("[compile] state serialize error: {e}");
            "{}".to_string()
        }),
    ) {
        eprintln!("[compile] failed to write compile state: {e}");
    }
}

#[tauri::command]
pub async fn compile_source_file(path: String) -> Result<serde_json::Value, String> {
    llm_wiki_core::dream::cancel_active_dream("desktop compile started");
    tauri::async_runtime::spawn_blocking(move || {
        let source_path = std::path::Path::new(&path);
        let ext = source_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

        // Route structured data files to ledger import instead of LLM compile.
        if ext == "json" || ext == "csv" || ext == "tsv" || ext == "xlsx" || ext == "xls" {
            let name = source_path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let result = if ext == "json" {
                llm_wiki_core::ledger::import_json(&path, Some(&name))
            } else if ext == "csv" || ext == "tsv" {
                llm_wiki_core::ledger::import_csv(&path, Some(&name))
            } else {
                llm_wiki_core::ledger::import_excel(&path, Some(&name))
            };
            match result {
                Ok(msg) => {
                    save_compile_state(&path, 1); // Mark as done
                    Ok(serde_json::json!({"pages_created": 1, "errors": [], "message": msg}))
                }
                Err(e) => {
                    save_compile_state(&path, 0);
                    Ok(serde_json::json!({"pages_created": 0, "errors": [format!("{e}")], "message": ""}))
                }
            }
        } else {
            let st = llm_wiki_core::types::SourceType::Doc;
            match llm_wiki_core::compile::compile_source(source_path, &st, false, false) {
                Ok(result) => {
                    save_compile_state(&path, result.pages_created);
                    Ok(serde_json::json!({
                        "pages_created": result.pages_created,
                        "errors": result.errors,
                    }))
            },
            Err(e) => Err(format!("Compile error: {e}")),
        }
        }
    }).await.map_err(|e| format!("Task join error: {e}"))?
}

#[tauri::command]
pub async fn chat_query(
    question: String,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    llm_wiki_core::dream::cancel_active_dream("desktop query started");
    let start = std::time::Instant::now();
    let (tx, rx) = tokio::sync::oneshot::channel::<serde_json::Value>();

    // Emit: searching phase
    let _ = app.emit(
        "chat-phase",
        serde_json::json!({"phase": "searching", "elapsed": 0.0}),
    );

    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let t0 = std::time::Instant::now();
            let streams: std::collections::HashSet<String> = ["metadata", "bm25", "graph"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let results = llm_wiki_core::search::search(&question, &streams, 5);
            let search_time = t0.elapsed().as_secs_f64();
            let mut context = String::new();
            for (i, r) in results.iter().enumerate() {
                let content = std::fs::read_to_string(&r.path).unwrap_or_default();
                let body = if content.starts_with("---") {
                    content[4..]
                        .find("\n---")
                        .map(|e| content[4 + e + 4..].to_string())
                        .unwrap_or(content)
                } else {
                    content
                };
                context.push_str(&format!(
                    "\n### {}: {}\n{}\n",
                    i + 1,
                    r.title.as_deref().unwrap_or(&r.id),
                    &body[..body.len().min(1500)]
                ));
            }
            let sources: Vec<_> = results.iter().map(|r| serde_json::json!({
                "id": r.id, "name": r.title.as_deref().unwrap_or(&r.id), "path": r.path.to_string_lossy(), "pageType": r.entity_type.as_deref().unwrap_or("unknown"), "relevance": r.rrf_score.unwrap_or(r.score)
            })).collect();
            let t1 = std::time::Instant::now();
            let system = "You are a precise knowledge assistant. Answer based on wiki context. Be concise. Reference sources as [1], [2].";
            let user = format!(
                "Question: {question}\n\nRelevant wiki pages:{context}\n\nAnswer concisely."
            );
            let answer = llm_wiki_core::llm::call_llm_default(&system, &user)
                .unwrap_or_else(|e| format!("LLM error: {e}"));
            let query_answer = llm_wiki_core::types::QueryAnswer {
                question: question.clone(),
                answer: answer.clone(),
                format: "markdown".to_string(),
                sources: results
                    .iter()
                    .map(|r| llm_wiki_core::types::SourceCitation {
                        id: r.id.clone(),
                        name: r.title.as_deref().unwrap_or(&r.id).to_string(),
                        path: r.path.to_string_lossy().to_string(),
                        page_type: r.entity_type.as_deref().unwrap_or("unknown").to_string(),
                        relevance: r.rrf_score.unwrap_or(r.score),
                    })
                    .collect(),
                debug_search: None,
            };
            if let Err(e) = llm_wiki_core::dream::log_query(&query_answer, true) {
                eprintln!("Query log error: {e}");
            }
            let gen_time = t1.elapsed().as_secs_f64();
            serde_json::json!({
                "answer": answer, "sources": sources,
                "searchTime": (search_time * 1000.0).round() / 1000.0,
                "genTime": (gen_time * 1000.0).round() / 1000.0,
                "totalTime": t0.elapsed().as_secs_f64().round(),
            })
        }));
        let payload = match result {
            Ok(json) => json,
            Err(panic_err) => {
                let msg = if let Some(s) = panic_err.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic_err.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                };
                serde_json::json!({"answer": format!("Internal error: {msg}"), "sources": []})
            }
        };
        let _ = tx.send(payload);
    });

    // Emit: generating phase (approximate — search typically < 200ms, LLM is the bottleneck)
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    let _ = app.emit(
        "chat-phase",
        serde_json::json!({"phase": "generating", "elapsed": start.elapsed().as_secs_f64()}),
    );

    let result = rx.await.map_err(|e| format!("Channel error: {e}"))?;
    let _ = app.emit("chat-phase", serde_json::json!({"phase": "done", "elapsed": start.elapsed().as_secs_f64(), "result": &result}));
    Ok(result)
}

#[tauri::command]
pub fn list_ledger_tables() -> Result<Vec<serde_json::Value>, String> {
    llm_wiki_core::ledger::list_tables().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_table_content(table: String) -> Result<String, String> {
    // Validate table name to prevent SQL injection
    if !table
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!("Invalid table name: {table}"));
    }
    let conn = duckdb::Connection::open(&llm_wiki_core::config::get_ledger_db_path())
        .map_err(|e| e.to_string())?;
    // Use parameterized query via format with validated name
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM \"{}\" LIMIT 1000", table))
        .map_err(|e| e.to_string())?;
    let cols: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let rows = stmt
        .query_map([], |row| {
            let mut obj = serde_json::Map::new();
            for (i, c) in cols.iter().enumerate() {
                let json_val = duckdb_value_to_json(row, i);
                obj.insert(c.clone(), json_val);
            }
            Ok(serde_json::Value::Object(obj))
        })
        .map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    for row in rows {
        data.push(row.map_err(|e| e.to_string())?);
    }
    serde_json::to_string_pretty(&data).map_err(|e| e.to_string())
}

fn duckdb_value_to_json(row: &duckdb::Row, i: usize) -> serde_json::Value {
    match row.get::<_, duckdb::types::Value>(i) {
        Ok(duckdb::types::Value::Null) => serde_json::Value::Null,
        Ok(duckdb::types::Value::Boolean(b)) => serde_json::Value::Bool(b),
        Ok(duckdb::types::Value::TinyInt(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::SmallInt(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::Int(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::BigInt(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::UTinyInt(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::USmallInt(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::UInt(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::UBigInt(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::Float(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::Double(v)) => serde_json::json!(v),
        Ok(duckdb::types::Value::Text(s)) => serde_json::Value::String(s),
        Ok(v) => serde_json::Value::String(format!("{v:?}")),
        Err(_) => serde_json::Value::Null,
    }
}

#[tauri::command]
pub fn save_page_content(page_id: String, content: String) -> Result<(), String> {
    let pages_dir = llm_wiki_core::config::get_pages_dir();
    for subdir in &["concepts", "entities"] {
        let path = pages_dir.join(subdir).join(format!("{page_id}.md"));
        if path.exists() {
            std::fs::write(&path, &content).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err(format!("Page not found: {page_id}"))
}

#[tauri::command]
pub fn open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    // Close existing settings window if open
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.close();
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        "settings",
        tauri::WebviewUrl::App("public/settings.html".into()),
    )
    .title("Settings — llm-wiki")
    .inner_size(600.0, 520.0)
    .resizable(true)
    .center()
    .build()
    .map_err(|e| format!("{e}"))?;
    Ok(())
}

#[tauri::command]
pub fn check_config() -> Result<bool, String> {
    let llm = llm_wiki_core::config::get_llm_config();
    Ok(!llm.api_key.is_empty())
}

#[tauri::command]
pub fn save_config(config: serde_json::Value) -> Result<(), String> {
    let config_path = llm_wiki_core::config::writable_config_path();
    if let Some(p) = config_path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }

    // Load existing config, merge new values over it
    let mut existing: serde_json::Value = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_yaml::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    if let Some(obj) = config.as_object() {
        if let Some(ex) = existing.as_object_mut() {
            for (k, v) in obj {
                match k.as_str() {
                    "apiKey" => set_nested(ex, "model", "api_key", v),
                    "provider" => set_nested(ex, "model", "provider", v),
                    "model" => set_nested(ex, "model", "model", v),
                    "temperature" => set_nested(ex, "model", "temperature", v),
                    "baseUrl" => set_nested(ex, "model", "base_url", v),
                    "ocrServerUrl" => set_nested(ex, "liteparse", "ocr_server_url", v),
                    "ocrLanguage" => set_nested(ex, "liteparse", "ocr_language", v),
                    "ocrEnabled" => set_nested(ex, "liteparse", "ocr_enabled", v),
                    "ocrEngine" => set_nested(ex, "ocr", "engine", v),
                    "ocrModel" => set_nested(ex, "ocr", "model", v),
                    "ocrModelRoot" => set_nested(ex, "ocr", "model_root", v),
                    "ocrDevice" => set_nested(ex, "ocr", "device", v),
                    "ocrAutoDownload" => set_nested(ex, "ocr", "auto_download", v),
                    "unlimitedOcrTask" => set_nested3(ex, "ocr", "options", "task", v),
                    "unlimitedOcrPrompt" => set_nested3(ex, "ocr", "options", "prompt", v),
                    "unlimitedOcrMaxNewTokens" => {
                        set_nested3(ex, "ocr", "options", "max_new_tokens", v)
                    }
                    "unlimitedOcrCropMode" => set_nested3(ex, "ocr", "options", "crop_mode", v),
                    "unlimitedOcrNoRepeatNgramSize" => {
                        set_nested3(ex, "ocr", "options", "no_repeat_ngram_size", v)
                    }
                    "unlimitedOcrNgramWindow" => {
                        set_nested3(ex, "ocr", "options", "ngram_window", v)
                    }
                    "unlimitedOcrSlidingWindow" => {
                        set_nested3(ex, "ocr", "options", "sliding_window", v)
                    }
                    "unlimitedOcrTemperature" => {
                        set_nested3(ex, "ocr", "options", "temperature", v)
                    }
                    "maxResults" => set_nested(ex, "query", "max_results", v),
                    "stripSensitive" => set_nested(ex, "compile", "strip_sensitive", v),
                    _ => {
                        ex.insert(k.clone(), v.clone());
                    }
                }
            }
        }
    }

    let yaml = serde_yaml::to_string(&existing).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, yaml).map_err(|e| e.to_string())?;
    llm_wiki_core::config::reset_config();
    Ok(())
}

#[tauri::command]
pub fn get_full_config() -> Result<serde_json::Value, String> {
    let cfg = llm_wiki_core::config::get_config();
    let llm = llm_wiki_core::config::get_llm_config();
    let liteparse = llm_wiki_core::config::get_liteparse_config();
    Ok(serde_json::json!({
        "model": { "provider": llm.provider, "apiKey": llm.api_key, "model": llm.model, "baseUrl": llm.base_url, "temperature": llm.temperature, "maxTokens": llm.max_tokens },
        "liteparse": { "ocrServerUrl": liteparse.ocr_server_url, "ocrLanguage": liteparse.ocr_language, "ocrEnabled": liteparse.ocr_enabled, "dpi": liteparse.dpi },
        "ocr": { "engine": cfg.ocr.engine, "model": cfg.ocr.model, "modelRoot": cfg.ocr.model_root, "device": cfg.ocr.device, "autoDownload": cfg.ocr.auto_download, "options": cfg.ocr.options },
        "query": { "maxResults": cfg.query.max_results, "llmSynthesis": cfg.query.llm_synthesis },
        "compile": { "stripSensitive": cfg.compile.strip_sensitive },
    }))
}

fn set_nested(
    ex: &mut serde_json::Map<String, serde_json::Value>,
    section: &str,
    key: &str,
    value: &serde_json::Value,
) {
    let entry = ex
        .entry(section.to_string())
        .or_insert(serde_json::json!({}));
    if let Some(obj) = entry.as_object_mut() {
        obj.insert(key.to_string(), value.clone());
    } else {
        // Replace non-object with a new object containing this key
        *entry = serde_json::json!({key: value});
    }
}

fn set_nested3(
    ex: &mut serde_json::Map<String, serde_json::Value>,
    section: &str,
    subsection: &str,
    key: &str,
    value: &serde_json::Value,
) {
    let entry = ex
        .entry(section.to_string())
        .or_insert(serde_json::json!({}));
    if !entry.is_object() {
        *entry = serde_json::json!({});
    }
    if let Some(section_obj) = entry.as_object_mut() {
        let nested = section_obj
            .entry(subsection.to_string())
            .or_insert(serde_json::json!({}));
        if !nested.is_object() {
            *nested = serde_json::json!({});
        }
        if let Some(nested_obj) = nested.as_object_mut() {
            nested_obj.insert(key.to_string(), value.clone());
        }
    }
}

// Helpers
fn count_md(dir: &std::path::Path) -> usize {
    if !dir.exists() {
        return 0;
    }
    std::fs::read_dir(dir)
        .map(|e| {
            e.filter_map(|x| x.ok())
                .filter(|x| x.path().extension().and_then(|e| e.to_str()) == Some("md"))
                .count()
        })
        .unwrap_or(0)
}

fn count_json_entities(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().map(|o| o.len()))
        .unwrap_or(0)
}

fn count_json_edges(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("edges").and_then(|e| e.as_array()).map(|a| a.len()))
        .unwrap_or(0)
}
