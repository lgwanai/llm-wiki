//! Configuration loader for llm-wiki.
//!
//! Loads `wiki_config.yaml` with deep merge over defaults, env var expansion,
//! and provider-specific normalization. Caches config globally after first load.
//!
//! Config discovery order:
//! 1. `LLM_WIKI_CONFIG` environment variable
//! 2. `wiki_config.yaml` in CWD or parent directories
//! 3. `~/.config/llm-wiki/wiki_config.yaml`

use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use regex::Regex;

use crate::error::{WikiError, WikiResult};
use crate::types::{
    Config, ImageAnalysisConfig, LlmConfig, LoggingConfig, ModelConfig, OcrConfig, QueryConfig,
};

/// Config filename constant.
pub const CONFIG_FILENAME: &str = "wiki_config.yaml";

/// Environment variable for config file path override.
pub const CONFIG_ENV_VAR: &str = "LLM_WIKI_CONFIG";

/// Environment variable for wiki directory override.
pub const WIKI_DIR_ENV_VAR: &str = "LLM_WIKI_DIR";

/// Environment variable for project root.
pub const PROJECT_DIR_ENV_VAR: &str = "LLM_WIKI_PROJECT_DIR";

// ── Global caches ──

static CONFIG_CACHE: RwLock<Option<Config>> = RwLock::new(None);
static WIKI_DIR_CACHE: Mutex<Option<PathBuf>> = Mutex::new(None);
static PROJECT_ROOT_CACHE: Mutex<Option<PathBuf>> = Mutex::new(None);

// ═══════════════════════════════════════════════════════════════════════════
// Default config
// ═══════════════════════════════════════════════════════════════════════════

fn default_config() -> Config {
    Config {
        wiki_dir: ".wiki".into(),
        model: ModelConfig::default(),
        ocr: OcrConfig::default(),
        liteparse: crate::types::LiteparseConfig::default(),
        image_analysis: ImageAnalysisConfig::default(),
        query: QueryConfig::default(),
        logging: LoggingConfig::default(),
        compile: crate::types::CompileConfig::default(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Config file discovery
// ═══════════════════════════════════════════════════════════════════════════

/// Find config file in priority order: env var > cwd > parent dirs > home.
pub fn find_config_file() -> Option<PathBuf> {
    // 1. LLM_WIKI_CONFIG env var
    if let Ok(path) = std::env::var(CONFIG_ENV_VAR) {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. cwd and parent directories
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join(CONFIG_FILENAME);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 3. ~/.config/llm-wiki/
    if let Some(home) = dirs_home() {
        let home_config = home.join(".config").join("llm-wiki").join(CONFIG_FILENAME);
        if home_config.exists() {
            return Some(home_config);
        }
    }

    None
}

/// Find a project-local config file in cwd or parents.
pub fn find_local_config_file() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    for ancestor in cwd.ancestors() {
        let candidate = ancestor.join(CONFIG_FILENAME);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Return the config path that should be modified by CLI and desktop settings.
///
/// This mirrors discovery where possible so both surfaces mutate the same
/// effective config: explicit env path, then project-local config, then the
/// user-level config path.
pub fn writable_config_path() -> PathBuf {
    if let Ok(path) = std::env::var(CONFIG_ENV_VAR) {
        return PathBuf::from(path);
    }
    if let Some(local) = find_local_config_file() {
        return local;
    }
    dirs_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("llm-wiki")
        .join(CONFIG_FILENAME)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

// ═══════════════════════════════════════════════════════════════════════════
// Project root discovery
// ═══════════════════════════════════════════════════════════════════════════

/// Return the current wiki project root.
///
/// Resolution order:
/// 1. `LLM_WIKI_PROJECT_DIR` env var
/// 2. Parent of a local `wiki_config.yaml`
/// 3. Nearest parent with a `.wiki/` directory
/// 4. Fallback to CWD
pub fn get_project_root() -> PathBuf {
    // Check cache first
    if let Ok(cache) = PROJECT_ROOT_CACHE.lock() {
        if let Some(ref root) = *cache {
            return root.clone();
        }
    }

    let root = resolve_project_root();

    if let Ok(mut cache) = PROJECT_ROOT_CACHE.lock() {
        *cache = Some(root.clone());
    }

    root
}

fn resolve_project_root() -> PathBuf {
    // 1. Explicit env var
    if let Ok(dir) = std::env::var(PROJECT_DIR_ENV_VAR) {
        let p = PathBuf::from(dir);
        if let Ok(p) = std::fs::canonicalize(&p) {
            return p;
        }
        return p;
    }

    // 2. Local config's parent
    if let Some(local_config) = find_local_config_file() {
        return local_config
            .parent()
            .unwrap_or(&PathBuf::from("."))
            .to_path_buf();
    }

    // 3. Nearest .wiki/ directory parent
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            if ancestor.join(".wiki").is_dir() {
                return ancestor.to_path_buf();
            }
        }
        return cwd;
    }

    PathBuf::from(".")
}

// ═══════════════════════════════════════════════════════════════════════════
// Env var expansion
// ═══════════════════════════════════════════════════════════════════════════

/// Expand `${VAR}` and `$VAR` patterns in a string from environment.
fn expand_env_vars_in_string(value: &str) -> String {
    let re = Regex::new(r"\$\{([^}]+)\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    re.replace_all(value, |caps: &regex::Captures| {
        let var_name = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        std::env::var(var_name).unwrap_or_else(|_| caps[0].to_string())
    })
    .to_string()
}

/// Recursively expand env vars in a YAML value.
fn expand_env_vars(value: &mut serde_yaml::Value) {
    match value {
        serde_yaml::Value::String(s) => {
            *s = expand_env_vars_in_string(s);
        }
        serde_yaml::Value::Mapping(map) => {
            for (_, v) in map.iter_mut() {
                expand_env_vars(v);
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for v in seq.iter_mut() {
                expand_env_vars(v);
            }
        }
        _ => {}
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Deep merge
// ═══════════════════════════════════════════════════════════════════════════

/// Deep merge two YAML values. `overrides` takes precedence.
fn deep_merge(base: &mut serde_yaml::Value, overrides: serde_yaml::Value) {
    match (base, overrides) {
        (serde_yaml::Value::Mapping(base_map), serde_yaml::Value::Mapping(over_map)) => {
            for (k, v) in over_map {
                if let Some(existing) = base_map.get_mut(&k) {
                    deep_merge(existing, v);
                } else {
                    base_map.insert(k, v);
                }
            }
        }
        (base_val, over_val) => {
            *base_val = over_val;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Config normalization (backward compat with legacy keys)
// ═══════════════════════════════════════════════════════════════════════════

/// Normalize legacy config shapes into the current schema.
fn normalize_config(config: &mut serde_yaml::Value) {
    let map = match config {
        serde_yaml::Value::Mapping(m) => m,
        _ => return,
    };

    // ── Phase 1: Collect legacy values without holding mutable borrows ──
    let legacy_llm = map.remove("llm");
    let legacy_ollama = map.remove("ollama");
    let legacy_custom = map.remove("custom");
    let legacy_mineru = map.remove("mineru");
    let legacy_deepseek_ocr = map.remove("deepseek_ocr");
    let legacy_logics = map.remove("logics_parsing");
    let legacy_paddle = map.remove("paddleocr");

    // ── Phase 2: Ensure model/ocr sections exist ──
    // Get provider before borrowing mutably
    let provider = {
        let model = map
            .entry("model".into())
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        model
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("deepseek")
            .to_string()
    };

    // ── Phase 3: Apply legacy overrides ──
    let model = map.get_mut("model").unwrap(); // guaranteed to exist from above

    if let Some(serde_yaml::Value::Mapping(llm_map)) = legacy_llm {
        if let serde_yaml::Value::Mapping(model_map) = model {
            for (k, v) in llm_map {
                model_map.entry(k).or_insert(v);
            }
        }
    }

    // Apply ollama legacy
    if provider == "ollama" {
        if let Some(serde_yaml::Value::Mapping(legacy_map)) = legacy_ollama {
            if let serde_yaml::Value::Mapping(model_map) = model {
                for key in &["base_url", "model", "temperature", "num_ctx"] {
                    let k = serde_yaml::Value::from(*key);
                    if let Some(v) = legacy_map.get(&k) {
                        let is_empty = model_map.get(&k).map_or(true, |e| {
                            e.is_null() || e.as_str().map_or(true, str::is_empty)
                        });
                        if is_empty {
                            model_map.insert(k, v.clone());
                        }
                    }
                }
            }
        }
    }

    // Apply custom legacy
    if provider == "custom" {
        if let Some(serde_yaml::Value::Mapping(legacy_map)) = legacy_custom {
            if let serde_yaml::Value::Mapping(model_map) = model {
                for key in &["base_url", "api_url", "api_key", "model"] {
                    let k = serde_yaml::Value::from(*key);
                    if let Some(v) = legacy_map.get(&k) {
                        let is_empty = model_map.get(&k).map_or(true, |e| {
                            e.is_null() || e.as_str().map_or(true, str::is_empty)
                        });
                        if is_empty {
                            model_map.insert(k, v.clone());
                        }
                    }
                }
            }
        }
    }

    // ── Phase 4: Apply OCR legacy backends into options ──
    let backend = {
        let ocr = map
            .entry("ocr".into())
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        ocr.get("backend")
            .and_then(|v| v.as_str())
            .unwrap_or("mineru")
            .to_string()
    };

    let legacy_for_backend = match backend.as_str() {
        "mineru" => legacy_mineru,
        "deepseek" => legacy_deepseek_ocr,
        "logics" => legacy_logics,
        "paddle" => legacy_paddle,
        _ => None,
    };

    if let Some(legacy_val) = legacy_for_backend {
        let ocr = map.get_mut("ocr").unwrap();
        if let serde_yaml::Value::Mapping(ocr_map) = ocr {
            let options = ocr_map
                .entry("options".into())
                .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
            deep_merge(options, legacy_val);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Load & cache config
// ═══════════════════════════════════════════════════════════════════════════

/// Load configuration from file, with defaults as fallback. Caches result.
pub fn load_config() -> Config {
    // Fast path: read from cache
    if let Ok(cache) = CONFIG_CACHE.read() {
        if let Some(ref config) = *cache {
            return config.clone();
        }
    }

    // Slow path: load and cache
    let default = default_config();
    let default_yaml = serde_yaml::to_value(&default).unwrap_or_default();
    let mut merged = default_yaml;

    if let Some(config_file) = find_config_file() {
        if let Ok(content) = std::fs::read_to_string(&config_file) {
            if let Ok(user_yaml) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                deep_merge(&mut merged, user_yaml);
            } else {
                eprintln!(
                    "Warning: Failed to parse config file: {}",
                    config_file.display()
                );
            }
        }
    }

    normalize_config(&mut merged);
    expand_env_vars(&mut merged);

    let config = serde_yaml::from_value::<Config>(merged).unwrap_or(default);

    if let Ok(mut cache) = CONFIG_CACHE.write() {
        *cache = Some(config.clone());
    }

    config
}

/// Get current configuration (cached).
pub fn get_config() -> Config {
    load_config()
}

// ═══════════════════════════════════════════════════════════════════════════
// Accessor functions
// ═══════════════════════════════════════════════════════════════════════════

/// Get wiki directory path (resolved and absolute). Cached after first call.
pub fn get_wiki_dir() -> PathBuf {
    if let Ok(cache) = WIKI_DIR_CACHE.lock() {
        if let Some(ref dir) = *cache {
            return dir.clone();
        }
    }

    let wiki_dir = resolve_wiki_dir();

    if let Ok(mut cache) = WIKI_DIR_CACHE.lock() {
        *cache = Some(wiki_dir.clone());
    }

    wiki_dir
}

fn resolve_wiki_dir() -> PathBuf {
    let wiki_dir_str = std::env::var(WIKI_DIR_ENV_VAR).ok().unwrap_or_else(|| {
        // Avoid recursive lock by loading config from cache or fresh
        let config = load_config();
        config.wiki_dir.clone()
    });

    let wiki_dir = PathBuf::from(&wiki_dir_str);

    let wiki_dir = if wiki_dir.is_absolute() {
        wiki_dir
    } else {
        get_project_root().join(&wiki_dir)
    };

    std::fs::canonicalize(&wiki_dir).unwrap_or(wiki_dir)
}

/// Get the pages directory.
pub fn get_pages_dir() -> PathBuf {
    get_wiki_dir().join("pages")
}

/// Get the graph directory.
pub fn get_graph_dir() -> PathBuf {
    get_wiki_dir().join("graph")
}

/// Get the ledger directory.
pub fn get_ledger_dir() -> PathBuf {
    get_wiki_dir().join("ledger")
}

/// Get the ledger database path.
pub fn get_ledger_db_path() -> PathBuf {
    get_ledger_dir().join("ledger.duckdb")
}

/// Get the memory directory.
pub fn get_memory_dir() -> PathBuf {
    get_wiki_dir().join("memory")
}

/// Get the audit directory.
pub fn get_audit_dir() -> PathBuf {
    get_wiki_dir().join("audit")
}

/// Get the source images directory.
pub fn get_source_images_dir() -> PathBuf {
    get_wiki_dir().join("source").join("images")
}

/// Get entities.json path.
pub fn get_entities_path() -> PathBuf {
    get_graph_dir().join("entities.json")
}

/// Get edges.json path.
pub fn get_edges_path() -> PathBuf {
    get_graph_dir().join("edges.json")
}

// ═══════════════════════════════════════════════════════════════════════════
// Resolved LLM config
// ═══════════════════════════════════════════════════════════════════════════

/// Get the resolved LLM configuration with provider-specific defaults.
pub fn get_llm_config() -> LlmConfig {
    let config = get_config();
    let model = &config.model;
    let provider = &model.provider;

    match provider.as_str() {
        "ollama" => LlmConfig {
            provider: "ollama".into(),
            base_url: if model.base_url.is_empty() {
                "http://localhost:11434".into()
            } else {
                model.base_url.clone()
            },
            api_url: String::new(),
            api_key: String::new(),
            model: if model.model.is_empty() {
                "llama3.2".into()
            } else {
                model.model.clone()
            },
            temperature: model.temperature,
            max_tokens: model.max_tokens,
            num_ctx: model.num_ctx,
        },
        "custom" => LlmConfig {
            provider: "custom".into(),
            base_url: model.base_url.clone(),
            api_url: model.api_url.clone(),
            api_key: model.api_key.clone(),
            model: model.model.clone(),
            temperature: model.temperature,
            max_tokens: model.max_tokens,
            num_ctx: model.num_ctx,
        },
        _ => {
            // deepseek, openai, or any OpenAI-compatible provider
            let base_url = if model.base_url.is_empty() {
                match provider.as_str() {
                    "openai" => "https://api.openai.com".to_string(),
                    _ => "https://api.deepseek.com".to_string(),
                }
            } else {
                model.base_url.clone()
            };

            LlmConfig {
                provider: provider.clone(),
                base_url: base_url.clone(),
                api_url: String::new(),
                api_key: model.api_key.clone(),
                model: model.model.clone(),
                temperature: model.temperature,
                max_tokens: model.max_tokens,
                num_ctx: model.num_ctx,
            }
        }
    }
}

/// Get the complete API URL for chat completions.
pub fn get_api_url() -> String {
    let llm = get_llm_config();
    match llm.provider.as_str() {
        "ollama" => format!("{}/api/chat", llm.base_url.trim_end_matches('/')),
        "custom" if !llm.api_url.is_empty() => llm.api_url,
        _ => format!("{}/v1/chat/completions", llm.base_url.trim_end_matches('/')),
    }
}

/// Get query configuration.
pub fn get_query_config() -> QueryConfig {
    get_config().query.clone()
}

/// Get OCR configuration.
pub fn get_ocr_config() -> OcrConfig {
    get_config().ocr.clone()
}

/// Get image analysis configuration.
pub fn get_image_analysis_config() -> ImageAnalysisConfig {
    get_config().image_analysis.clone()
}

/// Get liteparse configuration.
pub fn get_liteparse_config() -> crate::types::LiteparseConfig {
    get_config().liteparse.clone()
}

// ═══════════════════════════════════════════════════════════════════════════
// Reset / init helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Reset configuration cache (for testing or re-initialization).
pub fn reset_config() {
    if let Ok(mut cache) = CONFIG_CACHE.write() {
        *cache = None;
    }
    if let Ok(mut cache) = WIKI_DIR_CACHE.lock() {
        *cache = None;
    }
    if let Ok(mut cache) = PROJECT_ROOT_CACHE.lock() {
        *cache = None;
    }
}

/// Create a default wiki_config.yaml at the given path.
pub fn create_default_config(dest: &Path) -> WikiResult<PathBuf> {
    if dest.exists() {
        return Err(WikiError::Config(format!(
            "Config file already exists: {}",
            dest.display()
        )));
    }

    let yaml = serde_yaml::to_string(&default_config())
        .map_err(|e| WikiError::Config(format!("Failed to serialize default config: {e}")))?;

    let content = format!(
        "# llm-wiki configuration\n# See docs for all options\n\n{}\n",
        yaml
    );

    std::fs::write(dest, &content)?;
    Ok(dest.to_path_buf())
}

/// Set dotted configuration keys in the writable config file.
///
/// Example keys: `ocr.engine`, `ocr.model`, `model.api_key`,
/// `liteparse.ocr_enabled`.
pub fn set_config_values(values: &[(String, String)]) -> WikiResult<PathBuf> {
    let path = writable_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut existing: serde_yaml::Value = if path.exists() {
        serde_yaml::from_str(&std::fs::read_to_string(&path)?)?
    } else {
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    };

    for (key, raw) in values {
        let value = parse_config_value(raw);
        set_dotted_yaml(&mut existing, key, value)?;
    }

    let yaml = serde_yaml::to_string(&existing)?;
    std::fs::write(&path, yaml)?;
    reset_config();
    Ok(path)
}

fn parse_config_value(raw: &str) -> serde_yaml::Value {
    if raw.eq_ignore_ascii_case("true") {
        return serde_yaml::Value::Bool(true);
    }
    if raw.eq_ignore_ascii_case("false") {
        return serde_yaml::Value::Bool(false);
    }
    if let Ok(i) = raw.parse::<i64>() {
        return serde_yaml::Value::Number(i.into());
    }
    if let Ok(f) = raw.parse::<f64>() {
        if let Ok(v) = serde_yaml::to_value(f) {
            return v;
        }
    }
    serde_yaml::Value::String(raw.to_string())
}

fn set_dotted_yaml(
    root: &mut serde_yaml::Value,
    key: &str,
    value: serde_yaml::Value,
) -> WikiResult<()> {
    let parts: Vec<&str> = key.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err(WikiError::Config("Config key cannot be empty".into()));
    }

    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        if !matches!(current, serde_yaml::Value::Mapping(_)) {
            return Err(WikiError::Config(format!(
                "Cannot set '{key}': '{part}' is not a mapping (it is a scalar value). \
                 Remove or rename the conflicting key before setting nested values under it."
            )));
        }
        let map = current.as_mapping_mut().unwrap();
        current = map
            .entry(serde_yaml::Value::String((*part).to_string()))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }

    if !matches!(current, serde_yaml::Value::Mapping(_)) {
        return Err(WikiError::Config(format!(
            "Cannot set '{key}': leaf path conflicts with an existing scalar value."
        )));
    }
    current.as_mapping_mut().unwrap().insert(
        serde_yaml::Value::String(parts[parts.len() - 1].to_string()),
        value,
    );
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Validation
// ═══════════════════════════════════════════════════════════════════════════

/// Validate configuration and return human-readable issues.
pub fn validate_config(config: &Config) -> Vec<String> {
    let mut issues: Vec<String> = Vec::new();

    // Model validation
    let valid_providers = ["deepseek", "openai", "ollama", "custom"];
    if !valid_providers.contains(&config.model.provider.as_str()) {
        issues.push(format!(
            "model.provider: '{}' is invalid — must be one of {:?}",
            config.model.provider, valid_providers
        ));
    }

    // API key check for providers that need it
    if config.model.provider != "ollama" && config.model.api_key.is_empty() {
        issues.push(format!(
            "model.api_key: required for provider '{}' — set api_key in config",
            config.model.provider
        ));
    }

    // OCR validation
    let valid_modes = ["local", "api"];
    if !valid_modes.contains(&config.ocr.mode.as_str()) {
        issues.push(format!(
            "ocr.mode: '{}' is invalid — must be one of {:?}",
            config.ocr.mode, valid_modes
        ));
    }

    let valid_backends = [
        "mineru",
        "deepseek",
        "logics",
        "paddle",
        "paddleocr",
        "deepseek-ocr",
        "api",
    ];
    if !valid_backends.contains(&config.ocr.backend.as_str()) {
        issues.push(format!(
            "ocr.backend: '{}' is invalid — must be one of {:?}",
            config.ocr.backend, valid_backends
        ));
    }

    if config.ocr.mode == "api" {
        if config.ocr.api_model.is_empty() {
            issues.push("ocr.api_model: required when mode is 'api'".into());
        }
    }

    let valid_ocr_engines = ["paddleocr", "paddleocr-vl", "mineru", "deepseek-ocr"];
    if !valid_ocr_engines.contains(&config.ocr.engine.as_str()) {
        issues.push(format!(
            "ocr.engine: '{}' is invalid — must be one of {:?}",
            config.ocr.engine, valid_ocr_engines
        ));
    }

    // Image analysis validation
    if config.image_analysis.enabled && config.image_analysis.api_model.is_empty() {
        issues.push("image_analysis.api_model: required when enabled".into());
    }

    // Query validation
    let valid_streams = ["metadata", "bm25", "graph", "ledger", "chunk", "vector"];
    let streams = config.query.search_streams.as_str();
    if streams != "all" && streams != "*" {
        for s in streams.split(',') {
            let s = s.trim();
            if !s.is_empty() && !valid_streams.contains(&s) {
                issues.push(format!(
                    "query.search_streams: unknown stream '{}' (valid: {:?})",
                    s, valid_streams
                ));
            }
        }
    }

    if config.query.max_results == 0 {
        issues.push("query.max_results: must be positive".into());
    }

    issues
}

// ═══════════════════════════════════════════════════════════════════════════
// Display
// ═══════════════════════════════════════════════════════════════════════════

/// Print effective configuration as JSON.
pub fn print_config() {
    let config = get_config();
    let llm = get_llm_config();
    let compact = serde_json::json!({
        "wiki_dir": config.wiki_dir,
        "model": {
            "provider": llm.provider,
            "base_url": llm.base_url,
            "model": llm.model,
            "temperature": llm.temperature,
            "max_tokens": llm.max_tokens,
            "num_ctx": llm.num_ctx,
        },
        "ocr": config.ocr,
        "liteparse": config.liteparse,
        "image_analysis": config.image_analysis,
        "query": config.query,
        "logging": config.logging,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&compact).unwrap_or_default()
    );
    println!("\nWiki directory: {}", get_wiki_dir().display());
    println!("API URL: {}", get_api_url());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_expand_env_vars_simple() {
        std::env::set_var("TEST_VAR", "hello");
        let result = expand_env_vars_in_string("prefix_${TEST_VAR}_suffix");
        assert_eq!(result, "prefix_hello_suffix");
    }

    #[test]
    fn test_expand_env_vars_no_brace() {
        std::env::set_var("TEST_VAR2", "world");
        let result = expand_env_vars_in_string("hello_$TEST_VAR2");
        assert_eq!(result, "hello_world");
    }

    #[test]
    fn test_deep_merge() {
        let mut base = serde_yaml::from_str("a: 1\nb:\n  c: 2").unwrap();
        let over = serde_yaml::from_str("b:\n  d: 3\ne: 4").unwrap();
        deep_merge(&mut base, over);

        let result: HashMap<String, serde_yaml::Value> = serde_yaml::from_value(base).unwrap();
        assert_eq!(result.get("a").and_then(|v| v.as_i64()), Some(1));
    }
}
