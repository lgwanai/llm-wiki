//! llm-wiki: LLM-driven personal knowledge management system.
//!
//! Build, query, and maintain a personal knowledge base using LLMs.
//! Compile documents into structured wiki pages with knowledge graph linking.

pub mod benchmark;
pub mod bulk;
pub mod cli;
pub mod compile;
pub mod compile_chunk;
pub mod compile_parse;
pub mod compile_prompt;
pub mod config;
pub mod consolidate;
pub mod crystallize;
pub mod dream;
pub mod error;
pub mod gen_chart;
pub mod graph;
pub mod ledger;
pub mod lint;
pub mod llm;
pub mod local_ocr;
pub mod ocr_api;
pub mod package;
pub mod query;
pub mod search;
pub mod search_tokenize;
pub mod table_extract;
pub mod table_query;
pub mod types;
pub mod update;
pub mod url2markdown;

// Re-export commonly used types
pub use types::{
    CompileResult, Config, Edge, Entity, GraphStats, LintReport, LlmConfig, MemoryEntry, PageType,
    QueryAnswer, SearchResult, SearchStream, SourceType, WikiPage, WikiStatus,
};

pub use error::{WikiError, WikiResult};

pub use config::{
    create_default_config, find_config_file, get_api_url, get_audit_dir, get_config,
    get_entities_path, get_graph_dir, get_ledger_db_path, get_ledger_dir, get_liteparse_config,
    get_llm_config, get_memory_dir, get_ocr_config, get_pages_dir, get_project_root,
    get_query_config, get_source_images_dir, get_wiki_dir, load_config, print_config, reset_config,
    set_config_values, validate_config, writable_config_path, CONFIG_FILENAME,
};
