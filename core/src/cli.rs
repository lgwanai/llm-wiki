//! CLI dispatcher for llm-wiki.
//!
//! Uses clap derive to define all subcommands and delegates to the appropriate
//! module for execution.

use clap::{Parser, Subcommand};

/// LLM Wiki — Personal knowledge base powered by LLMs (Rust edition)
#[derive(Parser, Debug)]
#[command(
    name = "wiki",
    version = env!("CARGO_PKG_VERSION"),
    about = "LLM Wiki CLI — Personal knowledge base powered by LLMs",
    long_about = "Build, query, and maintain a personal knowledge base using LLMs.\n\
                  Compile documents into structured wiki pages with knowledge graph linking.",
    after_help = "Environment:\n  \
                  LLM_WIKI_DIR     Wiki directory path (default: .wiki)\n  \
                  LLM_WIKI_CONFIG  Config file path (default: wiki_config.yaml)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize wiki directory structure
    Init,

    /// Show or create configuration
    Config {
        /// Create default config file in current directory
        #[arg(long)]
        init: bool,

        /// Validate configuration and exit
        #[arg(long)]
        check: bool,

        /// Set config value with dotted key syntax, e.g. --set ocr.engine=paddleocr
        #[arg(long = "set", value_name = "KEY=VALUE")]
        set: Vec<String>,
    },

    /// Compile source file/directory to wiki pages
    Compile {
        /// Source file or directory to compile
        source: String,

        /// Source type: doc, article, code, or conversation
        #[arg(long, default_value = "doc")]
        source_type: String,

        /// Force re-compile even if source hasn't changed
        #[arg(long)]
        force: bool,

        /// Preview without writing files
        #[arg(long)]
        dry_run: bool,

        /// Directory recursion depth (0 = direct files only, omit = all)
        #[arg(long)]
        depth: Option<usize>,

        /// Max concurrent LLM calls (default: 1, max: 4)
        #[arg(short = 'j', long)]
        jobs: Option<usize>,
    },

    /// Print the prompts needed for Agent-powered compilation
    CompilePrompt {
        /// Source file to compile
        source: String,

        /// Source type: doc, article, code, or conversation
        #[arg(long, default_value = "doc")]
        source_type: String,
    },

    /// Ingest an Agent-generated compile response into wiki pages
    CompileIngest {
        /// Source file represented by the compile response
        source: String,

        /// Source type: doc, article, code, or conversation
        #[arg(long, default_value = "doc")]
        source_type: String,

        /// File containing the Agent response. Reads stdin when omitted.
        #[arg(long)]
        response: Option<String>,

        /// Source language hint used by the compile parser
        #[arg(long, default_value = "en")]
        lang: String,
    },

    /// Query wiki and get synthesized answer
    Query {
        /// Question to answer
        question: String,

        /// File answer back to wiki
        #[arg(long)]
        file_back: bool,

        /// Output format: markdown, table, timeline, slides, json, graph
        #[arg(long, default_value = "markdown")]
        format: String,

        /// Skip LLM synthesis — return raw search results
        #[arg(long)]
        no_synthesis: bool,

        /// Print search trace for retrieval debugging
        #[arg(long)]
        debug_search: bool,
    },

    /// Run four-phase dream consolidation in the background
    Dream {
        /// Run in the foreground instead of spawning a background worker
        #[arg(long)]
        foreground: bool,

        /// Allow mechanical auto-execution in later phases when safety gates are available
        #[arg(long)]
        auto: bool,

        /// Internal worker mode used by the non-blocking launcher
        #[arg(long, hide = true)]
        worker: bool,
    },

    /// Search diagnostics and evaluation
    Search {
        #[command(subcommand)]
        cmd: SearchCmd,
    },

    /// Run RAG benchmark
    Benchmark {
        /// Benchmark eval jsonl file
        file: String,

        /// Benchmark method: retrieval, ragas-lite, both
        #[arg(long, default_value = "both")]
        method: String,

        /// Top-k retrieval cutoff
        #[arg(short = 'k', long, default_value = "5")]
        top_k: usize,

        /// Write result JSON to file
        #[arg(short = 'o', long)]
        output: Option<String>,
    },

    /// Health check wiki pages and graph
    Lint {
        /// Auto-fix detected issues
        #[arg(long)]
        auto_heal: bool,
    },

    /// Bulk operations: stats, clean, merge, export, delete
    Bulk {
        #[command(subcommand)]
        cmd: BulkCmd,
    },

    /// Ledger/台账 management (DuckDB-backed structured tables)
    Ledger {
        #[command(subcommand)]
        cmd: LedgerCmd,
    },

    /// View and query extracted markdown tables
    Table {
        #[command(subcommand)]
        cmd: TableCmd,
    },

    /// Consolidate memory tiers and apply retention decay
    Consolidate {
        /// Tiers to consolidate (comma-separated)
        #[arg(long, default_value = "working,episodic,semantic")]
        tiers: String,

        /// Only apply retention decay, skip promotion
        #[arg(long)]
        decay_only: bool,
    },

    /// Show wiki statistics dashboard
    Status,

    /// Update wiki from git (self-update)
    Update,
}

#[derive(Subcommand, Debug)]
pub enum SearchCmd {
    /// Diagnose retrieval index health
    Doctor,
    /// Evaluate retrieval from a jsonl file
    Eval {
        /// Retrieval eval jsonl file
        file: String,
        /// Top-k results to evaluate
        #[arg(long, default_value = "5")]
        limit: usize,
    },
}

#[derive(Subcommand, Debug)]
pub enum BulkCmd {
    /// Detailed wiki statistics
    Stats,
    /// Clean orphan pages
    Clean {
        /// Preview only, don't delete
        #[arg(long)]
        dry_run: bool,
    },
    /// Merge duplicate entities
    Merge {
        /// Preview only
        #[arg(long)]
        dry_run: bool,
    },
    /// Export wiki subset
    Export {
        /// Entity type to export
        #[arg(long)]
        entity_type: Option<String>,
    },
    /// Bulk delete pages
    Delete {
        /// Delete stale (low-confidence) pages
        #[arg(long)]
        stale: bool,
        /// Delete below confidence threshold
        #[arg(long)]
        confidence: Option<f64>,
        /// Preview only
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum LedgerCmd {
    /// List all tables
    List,
    /// Show table schema and data
    Show {
        /// Table name (display or actual)
        table: String,
    },
    /// Create a new table
    Create {
        /// Display name for the table
        display_name: String,
        /// Field definitions as JSON
        #[arg(long)]
        fields: String,
        /// Unique key field(s)
        #[arg(long)]
        unique: Option<String>,
        /// Add auto-increment _id field
        #[arg(long)]
        auto_increment: bool,
        /// Override safe table name
        #[arg(long)]
        table_name: Option<String>,
        /// Table description
        #[arg(long, default_value = "")]
        description: String,
    },
    /// Insert data into a table
    Insert {
        /// Table name
        table: String,
        /// JSON data (object or array)
        #[arg(long)]
        data: String,
        /// Continue on partial errors
        #[arg(long)]
        batch: bool,
    },
    /// Modify table schema
    UpdateSchema {
        /// Table name
        table: String,
        /// Add fields as JSON
        #[arg(long)]
        add: Option<String>,
        /// Remove fields: name1,name2
        #[arg(long)]
        remove: Option<String>,
        /// Rename field: old:new
        #[arg(long)]
        rename: Option<String>,
        /// Change field type as JSON
        #[arg(long)]
        modify: Option<String>,
    },
    /// Delete a table
    Delete {
        /// Table name
        table: String,
        /// Keep files on disk
        #[arg(long)]
        keep_files: bool,
    },
    /// Show table statistics
    Stats {
        /// Table name (omit for all tables)
        table: Option<String>,
    },
    /// Show table schema for SQL generation
    Schema {
        /// Table name
        table: String,
    },
    /// Execute raw SQL (read-only)
    Sql {
        /// SQL SELECT statement
        sql_text: String,
    },
    /// Paginated SQL query on a table
    Query {
        /// Table name
        table: String,
        /// SQL SELECT statement
        #[arg(long)]
        sql: String,
        /// Page number
        #[arg(long, default_value = "1")]
        page: u32,
        /// Rows per page
        #[arg(long, default_value = "20")]
        page_size: u32,
    },
    /// Batch traversal through table rows
    Traverse {
        /// Table name
        table: String,
        /// Rows per batch
        #[arg(long, default_value = "100")]
        batch_size: usize,
        /// Starting offset
        #[arg(long, default_value = "0")]
        offset: usize,
    },
    /// Natural language question → SQL → results
    Ask {
        /// Table name
        table: String,
        /// Natural language question
        question: String,
        /// Page number
        #[arg(long, default_value = "1")]
        page: u32,
        /// Rows per page
        #[arg(long, default_value = "20")]
        page_size: u32,
    },
    /// Prepare schema + function context for SQL generation
    Context {
        /// Table name
        table: String,
        /// Natural language question
        question: String,
    },
    /// Import CSV/TSV/JSON/Excel file as ledger table
    Import {
        /// CSV, TSV, JSON, XLSX, or XLS file path
        file: String,
        /// Table display name
        #[arg(long)]
        name: Option<String>,
    },
    /// Search across ledger tables
    Search {
        /// Search query
        query: String,
    },
    /// Export ledger table as CSV
    Export {
        /// Table name to export
        table: String,
        /// Output CSV path
        #[arg(short = 'o', long)]
        output: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum TableCmd {
    /// List all extracted markdown tables
    List,
    /// Show table content (from markdown extraction)
    Show {
        /// Table name
        table: String,
    },
    /// Query a table with natural language
    Ask {
        /// Table name
        table: String,
        /// Natural language question
        question: String,
    },
    /// Get table schema
    Schema {
        /// Table name
        table: String,
    },
}
