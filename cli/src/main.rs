//! llm-wiki binary entry point.
//!
//! Parses CLI arguments and dispatches to the appropriate module.

use std::process;

use clap::Parser;

use llm_wiki_core::cli::{Cli, Commands};
use llm_wiki_core::config;
use llm_wiki_core::error::WikiResult;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => cmd_init(),
        Commands::Config { init, check } => cmd_config(init, check),
        Commands::Compile { source, source_type, force, dry_run, depth, jobs } => {
            cmd_compile(source, source_type, force, dry_run, depth, jobs)
        }
        Commands::Query { question, file_back, format, no_synthesis, debug_search } => {
            cmd_query(question, file_back, format, !no_synthesis, debug_search)
        }
        Commands::Search { cmd } => cmd_search(cmd),
        Commands::Benchmark { file, method, top_k, output } => {
            cmd_benchmark(file, method, top_k, output)
        }
        Commands::Lint { auto_heal } => cmd_lint(auto_heal),
        Commands::Bulk { cmd } => cmd_bulk(cmd),
        Commands::Ledger { cmd } => cmd_ledger(cmd),
        Commands::Consolidate { tiers, decay_only } => cmd_consolidate(tiers, decay_only),
        Commands::Status => cmd_status(),
        Commands::Update => cmd_update(),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

// ── Command implementations (stubs for now) ──

fn cmd_init() -> WikiResult<()> {
    config::reset_config();
    let wiki_dir = config::get_wiki_dir();

    let dirs = vec![
        wiki_dir.join("source/articles"),
        wiki_dir.join("source/documents"),
        wiki_dir.join("source/code"),
        wiki_dir.join("source/misc"),
        wiki_dir.join("pages/concepts"),
        wiki_dir.join("pages/entities"),
        wiki_dir.join("pages/sessions"),
        wiki_dir.join("graph"),
        wiki_dir.join("ledger"),
        wiki_dir.join("memory"),
        wiki_dir.join("audit"),
    ];

    for d in &dirs {
        std::fs::create_dir_all(d)?;
    }

    // Create index.md if not exists
    let index_file = wiki_dir.join("pages/index.md");
    if !index_file.exists() {
        std::fs::write(&index_file, "# Wiki Index\n\nWelcome to your knowledge base.\n")?;
    }

    // Create log.md if not exists
    let log_file = wiki_dir.join("log.md");
    if !log_file.exists() {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
        std::fs::write(
            &log_file,
            format!("# Wiki Log\n\nChronological record of all wiki operations.\n\n## [{now}] init | wiki initialized\n"),
        )?;
    }

    // Create schema.md if not exists
    let schema_file = wiki_dir.join("schema.md");
    if !schema_file.exists() {
        std::fs::write(
            &schema_file,
            include_str!("../templates/schema.md"),
        )?;
    }

    println!("Wiki initialized: {} directories created", dirs.len());
    println!("Wiki directory: {}", wiki_dir.display());
    Ok(())
}

fn cmd_config(init: bool, check: bool) -> WikiResult<()> {
    if init {
        let dest = std::env::current_dir()?.join(config::CONFIG_FILENAME);
        match config::create_default_config(&dest) {
            Ok(path) => {
                println!("Config file created: {}", path.display());
                println!("Edit the file to set your API key and preferences.");
                return Ok(());
            }
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }

    if check {
        let cfg = config::get_config();
        let issues = config::validate_config(&cfg);
        if issues.is_empty() {
            println!("Configuration is valid.");
            return Ok(());
        }
        println!("Configuration issues found:");
        for issue in &issues {
            println!("  - {issue}");
        }
        std::process::exit(1);
    }

    config::print_config();
    Ok(())
}

fn cmd_compile(
    source: String,
    source_type: String,
    force: bool,
    dry_run: bool,
    depth: Option<usize>,
    jobs: Option<usize>,
) -> WikiResult<()> {
    use llm_wiki_core::compile;
    use llm_wiki_core::types::SourceType;

    let source_path = std::path::Path::new(&source);
    let st = SourceType::from_str(&source_type);
    let max_jobs = jobs.unwrap_or(1).min(4);

    if source_path.is_dir() {
        let files = compile::iter_source_files(source_path, depth);
        println!("Found {} source files", files.len());
        for file in &files {
            println!("  Compiling: {}", file.display());
            let result = compile::compile_source(file, &st, force, dry_run)?;
            if !result.errors.is_empty() {
                for err in &result.errors {
                    eprintln!("  Error: {err}");
                }
            }
            println!("  Created {} pages", result.pages_created);
        }
    } else if source_path.is_file() {
        let result = compile::compile_source(source_path, &st, force, dry_run)?;
        for err in &result.errors {
            eprintln!("Error: {err}");
        }
        println!("Compiled: {} pages created", result.pages_created);
    } else {
        eprintln!("Source not found: {source}");
    }
    Ok(())
}

fn cmd_query(
    question: String,
    _file_back: bool,
    format: String,
    synthesis: bool,
    debug_search: bool,
) -> WikiResult<()> {
    use llm_wiki_core::query;
    let result = query::query_wiki(&question, synthesis, &format, debug_search)?;
    println!("{}", result.answer);
    if debug_search {
        if let Some(dbg) = &result.debug_search {
            println!("\n--- DEBUG ---\n{}", serde_json::to_string_pretty(dbg).unwrap_or_default());
        }
    }
    Ok(())
}

fn cmd_search(cmd: llm_wiki_core::cli::SearchCmd) -> WikiResult<()> {
    use llm_wiki_core::search;
    match cmd {
        llm_wiki_core::cli::SearchCmd::Doctor => {
            let report = search::search_doctor();
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        }
        llm_wiki_core::cli::SearchCmd::Eval { file: _, limit: _ } => {
            println!("Search eval: use benchmark command instead");
        }
    }
    Ok(())
}

fn cmd_benchmark(
    file: String,
    _method: String,
    top_k: usize,
    output: Option<String>,
) -> WikiResult<()> {
    use llm_wiki_core::benchmark;
    let result = benchmark::benchmark_retrieval(&file, top_k)?;
    let out = serde_json::to_string_pretty(&result).unwrap_or_default();
    if let Some(path) = output {
        std::fs::write(&path, &out)?;
        println!("Benchmark written to {path}");
    } else {
        println!("{out}");
    }
    Ok(())
}

fn cmd_lint(auto_heal: bool) -> WikiResult<()> {
    use llm_wiki_core::lint;
    let report = lint::run_lint(auto_heal)?;
    println!("{report}");
    Ok(())
}

fn cmd_bulk(cmd: llm_wiki_core::cli::BulkCmd) -> WikiResult<()> {
    use llm_wiki_core::{bulk, cli::BulkCmd};
    match cmd {
        BulkCmd::Stats => {
            let stats = bulk::bulk_stats()?;
            println!("{}", serde_json::to_string_pretty(&stats).unwrap());
        }
        BulkCmd::Clean { dry_run } => {
            let orphans = bulk::clean_orphans(dry_run)?;
            println!("Orphans: {} ({})", orphans.len(), if dry_run { "dry-run" } else { "removed" });
        }
        BulkCmd::Merge { .. } => println!("Merge: not yet implemented"),
        BulkCmd::Export { .. } => println!("Export: not yet implemented"),
        BulkCmd::Delete { .. } => println!("Delete: not yet implemented"),
    }
    Ok(())
}

fn cmd_ledger(cmd: llm_wiki_core::cli::LedgerCmd) -> WikiResult<()> {
    use llm_wiki_core::{cli::LedgerCmd, ledger};
    match cmd {
        LedgerCmd::List => {
            let tables = ledger::list_tables()?;
            println!("{}", serde_json::to_string_pretty(&tables).unwrap_or_default());
        }
        LedgerCmd::Show { table } => {
            let info = ledger::show_table(&table)?;
            println!("{}", serde_json::to_string_pretty(&info).unwrap_or_default());
        }
        LedgerCmd::Create { display_name, fields, unique, auto_increment, table_name, description } => {
            let name = ledger::create_table(&display_name, &fields, unique.as_deref(), auto_increment, table_name.as_deref(), &description)?;
            println!("Created table: {name}");
        }
        LedgerCmd::Insert { table, data, batch } => {
            let n = ledger::insert_data(&table, &data, batch)?;
            println!("Inserted {n} rows");
        }
        LedgerCmd::Delete { table, .. } => {
            ledger::delete_table(&table)?;
            println!("Deleted table: {table}");
        }
        LedgerCmd::Stats { table } => {
            let stats = ledger::table_stats(table.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&stats).unwrap_or_default());
        }
        LedgerCmd::Export { table, output } => {
            let csv = ledger::export_csv(&table, output.as_deref())?;
            if output.is_none() { println!("{csv}"); }
        }
        LedgerCmd::Import { file, name } => {
            let t = ledger::import_csv(&file, name.as_deref())?;
            println!("Imported to table: {t}");
        }
        LedgerCmd::Sql { sql_text } => {
            use llm_wiki_core::table_query;
            let rows = table_query::execute_sql(&sql_text)?;
            println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        }
        LedgerCmd::Ask { table, question, .. } => {
            use llm_wiki_core::table_query;
            let sql = table_query::ask_table(&table, &question)?;
            println!("SQL: {sql}");
            let rows = table_query::execute_sql(&sql)?;
            println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        }
        _ => println!("Command not yet implemented"),
    }
    Ok(())
}

fn cmd_consolidate(tiers: String, decay_only: bool) -> WikiResult<()> {
    use llm_wiki_core::consolidate;
    let result = consolidate::consolidate(&tiers, decay_only)?;
    println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
    Ok(())
}

fn cmd_status() -> WikiResult<()> {
    let wiki_dir = config::get_wiki_dir();
    let pages_dir = wiki_dir.join("pages");
    let graph_dir = wiki_dir.join("graph");

    let concepts = count_md_files(&pages_dir.join("concepts"));
    let entities = count_md_files(&pages_dir.join("entities"));

    let graph_entities = count_json_entities(&graph_dir.join("entities.json"));
    let graph_edges = count_json_edges(&graph_dir.join("edges.json"));

    let status = serde_json::json!({
        "pages": {
            "concepts": concepts,
            "entities": entities,
            "total": concepts + entities,
        },
        "graph": {
            "entities": graph_entities,
            "edges": graph_edges,
        },
        "files": {
            "index": pages_dir.join("index.md").exists(),
            "log": wiki_dir.join("log.md").exists(),
            "audit": wiki_dir.join("audit").exists(),
        }
    });

    println!("{}", serde_json::to_string_pretty(&status).unwrap_or_default());
    Ok(())
}

fn cmd_update() -> WikiResult<()> {
    use llm_wiki_core::update;
    update::update_from_git()
}

// ── Helpers ──

fn count_md_files(dir: &std::path::Path) -> usize {
    if !dir.exists() {
        return 0;
    }
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
                .count()
        })
        .unwrap_or(0)
}

fn count_json_entities(path: &std::path::Path) -> usize {
    if !path.exists() {
        return 0;
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| {
            if let Some(obj) = v.as_object() {
                obj.len()
            } else {
                0
            }
        })
        .unwrap_or(0)
}

fn count_json_edges(path: &std::path::Path) -> usize {
    if !path.exists() {
        return 0;
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| {
            v.get("edges")
                .and_then(|e| e.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        })
        .unwrap_or(0)
}
