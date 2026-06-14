//! Error types for llm-wiki.
//!
//! Uses `thiserror` for library-level typed errors. The binary entry point
//! (`main.rs`) converts these to user-facing messages via `anyhow`.

use thiserror::Error;

/// Unified error type for all wiki operations.
#[derive(Error, Debug)]
pub enum WikiError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("LLM API error: {0}")]
    Llm(String),

    #[error("DuckDB error: {0}")]
    DuckDb(#[from] duckdb::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("OCR error: {0}")]
    Ocr(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Lock error: {0}")]
    Lock(String),

    #[error("{0}")]
    Internal(String),
}

/// Convenience result type for wiki operations.
pub type WikiResult<T> = Result<T, WikiError>;

/// Convert a string into a WikiError::Internal.
impl From<String> for WikiError {
    fn from(s: String) -> Self {
        WikiError::Internal(s)
    }
}

/// Convert a &str into a WikiError::Internal.
impl From<&str> for WikiError {
    fn from(s: &str) -> Self {
        WikiError::Internal(s.to_string())
    }
}
