//! Markdown table extraction and DuckDB storage.
//!
//! Parses markdown tables from wiki page bodies during compilation,
//! stores them as structured DuckDB tables via the ledger module,
//! and replaces raw table markdown with [[table:xxx]] navigable links.

use crate::error::WikiResult;
use crate::ledger;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// A parsed markdown table ready for storage.
#[derive(Debug, Clone)]
pub struct MarkdownTable {
    /// Column headers (trimmed, from the first row)
    pub headers: Vec<String>,
    /// Data rows (each row is a Vec of cell strings)
    pub rows: Vec<Vec<String>>,
    /// The original markdown text block for replacement
    pub raw: String,
}

/// Extract all markdown tables from content.
///
/// Detects GitHub-flavored markdown tables: pipe-delimited rows where
/// the second row is a separator line (|:---|). Returns tables in the
/// order they appear in the content.
pub fn extract_tables(content: &str) -> Vec<MarkdownTable> {
    let mut tables = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Skip empty lines and non-table lines
        let trimmed = lines[i].trim();
        if trimmed.is_empty() || !trimmed.contains('|') {
            i += 1;
            continue;
        }

        // Potential table start: find consecutive pipe-containing lines
        let table_start = i;
        let mut table_end = i;

        // Collect consecutive non-empty lines that contain |
        while table_end < lines.len()
            && lines[table_end].contains('|')
            && !lines[table_end].trim().is_empty()
        {
            table_end += 1;
        }

        let run_len = table_end - table_start;
        if run_len < 2 {
            i = table_end;
            continue;
        }

        // Check if the second line is a separator row.
        let sep = lines[table_start + 1].trim();
        let sep_cells = parse_pipe_row(sep);
        let is_sep = !sep_cells.is_empty()
            && sep_cells.iter().all(|cell| {
                let c = cell.trim();
                c.len() >= 3 && c.chars().all(|ch| ch == ':' || ch == '-')
            });

        if !is_sep {
            i = table_end;
            continue;
        }

        // Parse headers from first row
        let headers: Vec<String> = parse_pipe_row(lines[table_start])
            .into_iter()
            .map(|c| c.trim().to_string())
            .collect();

        if headers.is_empty() {
            i = table_end;
            continue;
        }

        // Parse data rows (skip separator at table_start + 1)
        let mut rows = Vec::new();
        for row_idx in (table_start + 2)..table_end {
            let cells: Vec<String> = parse_pipe_row(lines[row_idx])
                .into_iter()
                .map(|c| c.trim().to_string())
                .collect();
            // Pad or truncate to match header count
            let mut padded = cells;
            padded.resize(headers.len(), String::new());
            rows.push(padded);
        }

        // Capture raw markdown for replacement
        let raw_start = lines[table_start..table_end].join("\n");
        // Include surrounding blank lines in the raw capture for clean replacement
        let raw = raw_start;

        tables.push(MarkdownTable { headers, rows, raw });

        i = table_end;
    }

    tables
}

fn parse_pipe_row(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    trimmed.split('|').collect()
}

/// Store all useful Markdown tables from a source document and replace them with table links.
///
/// The project treats embedded Markdown tables as structured data by default, because users rarely
/// create ledger tables explicitly. Any table with at least one data row is persisted.
pub fn extract_large_tables_to_links(
    content: &str,
    source: &Path,
    errors: &mut Vec<String>,
) -> String {
    let tables = extract_tables(content);
    if tables.is_empty() {
        return content.to_string();
    }

    let mut output = content.to_string();
    for (idx, table) in tables.iter().enumerate() {
        if table.rows.is_empty() {
            continue;
        }
        let table_name = source_table_name(source, idx + 1);
        match store_table(&table_name, &table.headers, &table.rows) {
            Ok(name) => {
                let link = table_link(&name, &table.headers);
                output = output.replacen(&table.raw, &link, 1);
            }
            Err(e) => errors.push(format!("Markdown table storage '{}': {e}", table_name)),
        }
    }
    output
}

fn source_table_name(source: &Path, index: usize) -> String {
    let stem = source
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.to_string_lossy().hash(&mut hasher);
    format!("{stem} md-table-{:x}-{index}", hasher.finish())
}

/// Store a markdown table in DuckDB via the ledger module.
///
/// Returns the sanitized DuckDB table name.
pub fn store_table(name: &str, headers: &[String], rows: &[Vec<String>]) -> WikiResult<String> {
    // Build fields JSON for ledger::create_table
    let fields: Vec<serde_json::Value> = headers
        .iter()
        .map(|h| serde_json::json!({"name": h, "type": "string"}))
        .collect();
    let fields_json = serde_json::to_string(&fields)
        .map_err(|e| crate::error::WikiError::Parse(format!("Fields serialization: {e}")))?;

    // Create the DuckDB table via ledger.
    // Slugify the table name to prevent SQL identifier issues from
    // LLM-generated page IDs containing special characters.
    let slug = ledger::slugify(name);
    let _ = ledger::delete_table(&slug);
    let safe_name = ledger::create_table(
        name,
        &fields_json,
        None,        // unique
        false,       // auto_increment
        Some(&slug), // table_name
        "Extracted from markdown document during compilation",
    )?;

    // Insert rows
    if !rows.is_empty() {
        // Build JSON array of row objects
        let row_objects: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                let mut obj = serde_json::Map::new();
                for (i, cell) in row.iter().enumerate() {
                    let key = if i < headers.len() {
                        headers[i].clone()
                    } else {
                        format!("col_{i}")
                    };
                    obj.insert(key, serde_json::Value::String(cell.clone()));
                }
                serde_json::Value::Object(obj)
            })
            .collect();

        let data_json = serde_json::to_string(&row_objects)
            .map_err(|e| crate::error::WikiError::Parse(format!("Data serialization: {e}")))?;

        ledger::insert_data(&safe_name, &data_json, true)?;
    }

    Ok(safe_name)
}

/// Build a [[table:name|label]] wikilink string for use in markdown.
pub fn table_link(name: &str, headers: &[String]) -> String {
    let label = if headers.is_empty() {
        format!("📊 {}", name)
    } else {
        let cols = headers
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if headers.len() > 3 { "..." } else { "" };
        format!("📊 {}{}", cols, suffix)
    };
    format!("[[table:{}|{}]]", name, label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_extract_simple_table() {
        let content = "\
Some text.

| Name | Age | City |
|------|-----|------|
| Alice | 30 | NYC |
| Bob | 25 | LA |

More text.";
        let tables = extract_tables(content);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].headers, vec!["Name", "Age", "City"]);
        assert_eq!(tables[0].rows.len(), 2);
        assert_eq!(tables[0].rows[0], vec!["Alice", "30", "NYC"]);
        assert_eq!(tables[0].rows[1], vec!["Bob", "25", "LA"]);
    }

    #[test]
    fn test_extract_multiple_tables() {
        let content = "\
| A | B |
|---|---|
| 1 | 2 |

Middle text.

| X | Y | Z |
|---|---|---|
| a | b | c |
| d | e | f |";
        let tables = extract_tables(content);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].headers, vec!["A", "B"]);
        assert_eq!(tables[1].headers, vec!["X", "Y", "Z"]);
    }

    #[test]
    fn test_no_table() {
        let content = "Just some regular text\nwith no tables at all.";
        let tables = extract_tables(content);
        assert!(tables.is_empty());
    }

    #[test]
    fn test_single_pipe_not_table() {
        let content = "Rust inline closure: |x| x + 1";
        let tables = extract_tables(content);
        assert!(tables.is_empty());
    }

    #[test]
    fn test_aligned_columns() {
        let content = "\
| Left | Center | Right |
|:-----|:------:|------:|
| L1   | C1     | R1    |
| L2   | C2     | R2    |";
        let tables = extract_tables(content);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].headers, vec!["Left", "Center", "Right"]);
    }

    #[test]
    fn test_table_with_uneven_columns() {
        let content = "\
| A | B |
|---|---|
| 1 | 2 | 3 |
| 4 |"; // row 0 has 3 cells, row 1 has 1 cell
        let tables = extract_tables(content);
        assert_eq!(tables.len(), 1);
        // Should pad/truncate to match header count (2)
        assert_eq!(tables[0].rows[0].len(), 2);
        assert_eq!(tables[0].rows[1].len(), 2);
    }

    #[test]
    fn test_table_link_format() {
        let link = table_link("my_table", &["Name".into(), "Age".into()]);
        assert!(link.contains("[[table:my_table|"));
        assert!(link.contains("Name, Age"));
    }

    #[test]
    fn test_table_link_truncates_long_headers() {
        let link = table_link(
            "t",
            &["A".into(), "B".into(), "C".into(), "D".into(), "E".into()],
        );
        assert!(link.contains("..."));
        assert!(!link.contains("D"));
        assert!(!link.contains("E"));
    }

    #[test]
    fn test_extract_markdown_tables_to_links_stores_table() {
        let _guard = env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let wiki_dir = tmp.path().join("wiki");
        std::fs::create_dir_all(&wiki_dir).unwrap();
        std::env::set_var("LLM_WIKI_DIR", &wiki_dir);
        crate::config::reset_config();

        let source = tmp.path().join("客户评分.md");
        let content = "\
# Report

| 客户 | 分数 |
|------|------|
| A | 90 |
| B | 80 |

Done.";
        let mut errors = Vec::new();
        let replaced = extract_large_tables_to_links(content, &source, &mut errors);

        assert!(errors.is_empty(), "{errors:?}");
        assert!(replaced.contains("[[table:"));
        assert!(!replaced.contains("| A | 90 |"));

        let tables = ledger::list_tables().unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0]["record_count"], serde_json::json!(2));
        assert!(tables[0]["table"]
            .as_str()
            .unwrap()
            .starts_with("md-table-"));
    }
}
