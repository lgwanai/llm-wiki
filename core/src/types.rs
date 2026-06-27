//! Core data types for llm-wiki.
//!
//! All shared types used across modules: configuration, knowledge graph entities,
//! edges, wiki pages, search results, and memory entries.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════
// Configuration types
// ═══════════════════════════════════════════════════════════════════════════

/// Top-level wiki configuration loaded from `wiki_config.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_wiki_dir")]
    pub wiki_dir: String,

    #[serde(default)]
    pub model: ModelConfig,

    #[serde(default)]
    pub ocr: OcrConfig,

    #[serde(default)]
    pub liteparse: LiteparseConfig,

    #[serde(default)]
    pub image_analysis: ImageAnalysisConfig,

    #[serde(default)]
    pub query: QueryConfig,

    #[serde(default)]
    pub logging: LoggingConfig,

    #[serde(default)]
    pub compile: CompileConfig,
}

fn default_wiki_dir() -> String {
    ".wiki".to_string()
}

/// LLM model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    #[serde(default = "default_provider")]
    pub provider: String,

    #[serde(default)]
    pub api_key: String,

    #[serde(default = "default_base_url")]
    pub base_url: String,

    #[serde(default)]
    pub api_url: String,

    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_temperature")]
    pub temperature: f64,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    #[serde(default = "default_num_ctx")]
    pub num_ctx: u32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            api_key: String::new(),
            base_url: default_base_url(),
            api_url: String::new(),
            model: default_model(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            num_ctx: default_num_ctx(),
        }
    }
}

fn default_provider() -> String {
    "deepseek".into()
}
fn default_base_url() -> String {
    "https://api.deepseek.com".into()
}
fn default_model() -> String {
    "deepseek-v4-flash".into()
}
fn default_temperature() -> f64 {
    0.3
}
fn default_max_tokens() -> u32 {
    32000
}
fn default_num_ctx() -> u32 {
    32768
}

/// LLM configuration resolved by provider (used at runtime after normalization).
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: String,
    pub base_url: String,
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f64,
    pub max_tokens: u32,
    pub num_ctx: u32,
}

/// OCR configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    #[serde(default = "default_ocr_mode")]
    pub mode: String,

    #[serde(default = "default_ocr_backend")]
    pub backend: String,

    /// Local OCR engine used for PDF OCR when liteparse OCR is enabled.
    /// Supported values: unlimited-ocr-mlx, paddleocr, paddleocr-vl, mineru, deepseek-ocr.
    #[serde(default = "default_ocr_engine")]
    pub engine: String,

    /// Local model name. For PaddleOCR this can be a PP-OCR model family/name.
    #[serde(default = "default_ocr_model")]
    pub model: String,

    /// Root directory containing local OCR model folders.
    #[serde(default = "default_ocr_model_root")]
    pub model_root: String,

    /// Inference device: auto, cpu, cuda, or mps.
    #[serde(default = "default_ocr_device")]
    pub device: String,

    /// Automatically create the local OCR runtime and download model weights.
    #[serde(default = "default_ocr_auto_download")]
    pub auto_download: bool,

    #[serde(default)]
    pub api_provider: String,

    #[serde(default)]
    pub api_url: String,

    #[serde(default)]
    pub api_key: String,

    #[serde(default)]
    pub api_model: String,

    #[serde(default = "default_ocr_prompt")]
    pub api_prompt: String,

    #[serde(default = "default_pdf_dpi")]
    pub pdf_dpi: u32,

    #[serde(default)]
    pub options: HashMap<String, serde_yaml::Value>,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            mode: default_ocr_mode(),
            backend: default_ocr_backend(),
            engine: default_ocr_engine(),
            model: default_ocr_model(),
            model_root: default_ocr_model_root(),
            device: default_ocr_device(),
            auto_download: default_ocr_auto_download(),
            api_provider: String::new(),
            api_url: String::new(),
            api_key: String::new(),
            api_model: String::new(),
            api_prompt: default_ocr_prompt(),
            pdf_dpi: default_pdf_dpi(),
            options: HashMap::new(),
        }
    }
}

fn default_ocr_mode() -> String {
    "local".into()
}
fn default_ocr_backend() -> String {
    "unlimited-ocr-mlx".into()
}
fn default_ocr_engine() -> String {
    "unlimited-ocr-mlx".into()
}
fn default_ocr_model() -> String {
    "Unlimited-OCR-MLX".into()
}
fn default_ocr_model_root() -> String {
    if let Ok(path) = std::env::var("LLM_WIKI_OCR_MODEL_ROOT") {
        return path;
    }
    String::new()
}
fn default_ocr_device() -> String {
    "auto".into()
}
fn default_ocr_auto_download() -> bool {
    true
}
fn default_ocr_prompt() -> String {
    "Convert the document to clean markdown format.".into()
}
fn default_pdf_dpi() -> u32 {
    150
}

// ═══════════════════════════════════════════════════════════════════════════
// Liteparse configuration
// ═══════════════════════════════════════════════════════════════════════════

/// Liteparse PDF parsing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteparseConfig {
    /// HTTP OCR server URL (third-party OCR API). When set, liteparse sends
    /// images to this URL for OCR instead of using local Tesseract.
    #[serde(default)]
    pub ocr_server_url: String,

    /// OCR language code (e.g., "eng", "chi_sim", "chi_sim+eng").
    #[serde(default = "default_ocr_lang")]
    pub ocr_language: String,

    /// Whether OCR is enabled for text-sparse pages and embedded images.
    #[serde(default = "default_ocr_enabled")]
    pub ocr_enabled: bool,

    /// DPI for page rendering.
    #[serde(default = "default_liteparse_dpi")]
    pub dpi: f32,

    /// Maximum pages to parse (default: 1000).
    #[serde(default = "default_max_pages")]
    pub max_pages: usize,

    /// Number of concurrent OCR workers.
    #[serde(default)]
    pub num_workers: usize,
}

impl Default for LiteparseConfig {
    fn default() -> Self {
        Self {
            ocr_server_url: String::new(),
            ocr_language: default_ocr_lang(),
            ocr_enabled: default_ocr_enabled(),
            dpi: default_liteparse_dpi(),
            max_pages: default_max_pages(),
            num_workers: 0, // 0 means auto-detect (CPU cores - 1)
        }
    }
}

fn default_ocr_lang() -> String {
    "chi_sim+eng".into()
}
fn default_liteparse_dpi() -> f32 {
    200.0
}
fn default_max_pages() -> usize {
    1000
}
fn default_ocr_enabled() -> bool {
    false
}

/// Image analysis config for compile-time vision model analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysisConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub api_provider: String,

    #[serde(default)]
    pub api_url: String,

    #[serde(default)]
    pub api_key: String,

    #[serde(default)]
    pub api_model: String,

    #[serde(default)]
    pub api_prompt: String,

    #[serde(default = "default_true")]
    pub ocr_fallback: bool,

    #[serde(default = "default_min_chars")]
    pub ocr_min_chars: usize,
}

impl Default for ImageAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_provider: String::new(),
            api_url: String::new(),
            api_key: String::new(),
            api_model: String::new(),
            api_prompt: String::new(),
            ocr_fallback: true,
            ocr_min_chars: default_min_chars(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_min_chars() -> usize {
    800
}

/// Query behavior configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryConfig {
    #[serde(default = "default_true")]
    pub llm_synthesis: bool,

    #[serde(default = "default_format")]
    pub default_format: String,

    #[serde(default = "default_max_results")]
    pub max_results: usize,

    #[serde(default = "default_search_streams")]
    pub search_streams: String,

    #[serde(default)]
    pub llm_query_expansion: bool,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            llm_synthesis: true,
            default_format: default_format(),
            max_results: default_max_results(),
            search_streams: default_search_streams(),
            llm_query_expansion: false,
        }
    }
}

fn default_format() -> String {
    "markdown".into()
}
fn default_max_results() -> usize {
    5
}
fn default_search_streams() -> String {
    "metadata,bm25,graph,ledger".into()
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

fn default_log_level() -> String {
    "INFO".into()
}

/// Quality scoring configuration (optional).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityConfig {
    pub min_confidence: Option<f64>,
    pub max_stale_days: Option<u32>,
}

/// Compilation behaviour configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileConfig {
    /// Strip sensitive data (API keys, tokens, passwords) before sending to LLM.
    #[serde(default = "default_true")]
    pub strip_sensitive: bool,
}

impl Default for CompileConfig {
    fn default() -> Self {
        Self {
            strip_sensitive: true,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Knowledge Graph types
// ═══════════════════════════════════════════════════════════════════════════

/// A knowledge graph entity (node).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,

    #[serde(rename = "type")]
    pub entity_type: String,

    pub name: String,

    #[serde(default)]
    pub attributes: HashMap<String, serde_json::Value>,

    #[serde(default)]
    pub confidence: f64,

    #[serde(default)]
    pub sources: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_confirmed: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Graph of entities keyed by ID.
pub type EntityGraph = HashMap<String, Entity>;

/// A knowledge graph edge (relationship between entities).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    pub source: String,

    pub target: String,

    #[serde(rename = "type")]
    pub rel_type: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Container for the edges JSON file format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeCollection {
    pub edges: Vec<Edge>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Wiki page types
// ═══════════════════════════════════════════════════════════════════════════

/// The type of a wiki page.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PageType {
    #[serde(rename = "entity")]
    Entity,
    #[serde(rename = "concept")]
    Concept,
}

impl std::fmt::Display for PageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PageType::Entity => write!(f, "entity"),
            PageType::Concept => write!(f, "concept"),
        }
    }
}

/// Source type for compilation (controls entity focus).
#[derive(Debug, Clone, PartialEq)]
pub enum SourceType {
    Doc,
    Article,
    Code,
    Conversation,
}

impl SourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceType::Doc => "doc",
            SourceType::Article => "article",
            SourceType::Code => "code",
            SourceType::Conversation => "conversation",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "doc" => SourceType::Doc,
            "article" => SourceType::Article,
            "code" => SourceType::Code,
            "conversation" => SourceType::Conversation,
            _ => SourceType::Doc,
        }
    }
}

/// A compiled wiki page.
#[derive(Debug, Clone)]
pub struct WikiPage {
    pub id: String,
    pub page_type: PageType,
    pub entity_type: String,
    pub name: String,
    pub path: PathBuf,
    pub content: String,
    pub frontmatter: HashMap<String, serde_json::Value>,
    pub facts_count: usize,
    pub relationships: Vec<PageRelationship>,
}

/// A parsed relationship from a wiki page.
#[derive(Debug, Clone)]
pub struct PageRelationship {
    pub rel_type: String,
    pub target_id: String,
    pub description: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Search types
// ═══════════════════════════════════════════════════════════════════════════

/// A search result from any retrieval stream.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub id: String,
    pub path: PathBuf,
    pub score: f64,
    pub stream: SearchStream,
    pub rrf_score: Option<f64>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub entity_type: Option<String>,
    pub stream_ranks: HashMap<String, usize>,
    pub stream_scores: HashMap<String, f64>,
}

/// Which retrieval stream produced a result.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SearchStream {
    #[serde(rename = "bm25")]
    Bm25,
    #[serde(rename = "metadata")]
    Metadata,
    #[serde(rename = "graph")]
    Graph,
    #[serde(rename = "ledger")]
    Ledger,
}

impl std::fmt::Display for SearchStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchStream::Bm25 => write!(f, "bm25"),
            SearchStream::Metadata => write!(f, "metadata"),
            SearchStream::Graph => write!(f, "graph"),
            SearchStream::Ledger => write!(f, "ledger"),
        }
    }
}

/// BM25 index entry for a single document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bm25Doc {
    pub path: String,
    pub tokens: Vec<String>,
    pub freqs: HashMap<String, usize>,
    pub length: usize,
}

// ═══════════════════════════════════════════════════════════════════════════
// Memory / consolidation types
// ═══════════════════════════════════════════════════════════════════════════

/// A memory tier entry (for consolidation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,

    #[serde(default)]
    pub claim: String,

    #[serde(default)]
    pub entity_id: String,

    #[serde(default)]
    pub confidence: f64,

    #[serde(default)]
    pub sources: Vec<String>,

    #[serde(default)]
    pub last_confirmed: String,

    #[serde(default)]
    pub reinforcements: u32,

    #[serde(default)]
    pub contradictions: Vec<String>,

    #[serde(default = "default_status")]
    pub status: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprioritized: Option<bool>,
}

fn default_status() -> String {
    "active".into()
}

/// An observation in working memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub timestamp: String,
    pub entity_ids: Vec<String>,
    pub content: String,
}

/// An episode summary in episodic memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub date: String,
    pub summary: String,
    pub observations: Vec<String>,
    pub entities: Vec<String>,
    pub decisions: Vec<String>,
    pub confidence: f64,
    pub created_at: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// Compile-related types
// ═══════════════════════════════════════════════════════════════════════════

/// Parsed content from an LLM compile response (single page section).
#[derive(Debug, Clone)]
pub struct ParsedPage {
    pub page_type: PageType,
    pub frontmatter: HashMap<String, serde_json::Value>,
    pub body: String,
}

/// Compilation result statistics.
#[derive(Debug, Clone, Serialize)]
pub struct CompileResult {
    pub source: String,
    pub pages_created: usize,
    pub pages_updated: usize,
    pub entities_added: usize,
    pub edges_added: usize,
    pub errors: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Query types
// ═══════════════════════════════════════════════════════════════════════════

/// A query plan that determines retrieval strategy.
#[derive(Debug, Clone)]
pub struct QueryPlan {
    pub intent: String,
    pub preferred_streams: Vec<String>,
    pub keywords: Vec<String>,
}

/// An answer returned by `wiki query`.
#[derive(Debug, Clone, Serialize)]
pub struct QueryAnswer {
    pub question: String,
    pub answer: String,
    pub format: String,
    pub sources: Vec<SourceCitation>,
    pub debug_search: Option<serde_json::Value>,
}

/// A citation to a wiki page used in an answer.
#[derive(Debug, Clone, Serialize)]
pub struct SourceCitation {
    pub id: String,
    pub name: String,
    pub path: String,
    pub page_type: String,
    pub relevance: f64,
}

// ═══════════════════════════════════════════════════════════════════════════
// Lint types
// ═══════════════════════════════════════════════════════════════════════════

/// A lint issue found during wiki health check.
#[derive(Debug, Clone, Serialize)]
pub struct LintIssue {
    pub issue_type: String,
    pub entity_id: Option<String>,
    pub name: Option<String>,
    pub description: String,
    pub severity: String, // "high", "medium", "low"
}

/// Lint report containing all issues.
#[derive(Debug, Clone, Serialize)]
pub struct LintReport {
    pub orphans: Vec<LintIssue>,
    pub stale: Vec<LintIssue>,
    pub broken_links: Vec<LintIssue>,
    pub contradictions: Vec<LintIssue>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Graph query types
// ═══════════════════════════════════════════════════════════════════════════

/// Statistics about the knowledge graph.
#[derive(Debug, Clone, Serialize)]
pub struct GraphStats {
    pub entity_count: usize,
    pub edge_count: usize,
    pub edge_types: HashMap<String, usize>,
    pub avg_edges_per_entity: f64,
    pub orphan_count: usize,
}

// ═══════════════════════════════════════════════════════════════════════════
// Wiki status type
// ═══════════════════════════════════════════════════════════════════════════

/// Overall wiki status report.
#[derive(Debug, Clone, Serialize)]
pub struct WikiStatus {
    pub pages: PageStatus,
    pub graph: GraphStatus,
    pub files: FileStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageStatus {
    pub concepts: usize,
    pub entities: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphStatus {
    pub entities: usize,
    pub edges: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileStatus {
    pub index: bool,
    pub log: bool,
    pub audit: bool,
}
