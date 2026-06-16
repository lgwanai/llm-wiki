//! Markdown table extraction and DuckDB storage.
//!
//! Parses markdown tables from wiki page bodies during compilation,
//! stores them as structured DuckDB tables via the ledger module,
//! and replaces raw table markdown with [[table:xxx]] navigable links.

use crate::error::WikiResult;
use crate::ledger;

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

        // Check if the second line is a separator row
        let sep = lines[table_start + 1].trim();
        let is_sep = sep.contains('|')
            && sep
                .chars()
                .all(|c| c == '|' || c == ':' || c == '-' || c == ' ' || c == '\t');

        if !is_sep {
            i = table_end;
            continue;
        }

        // Parse headers from first row
        let headers: Vec<String> = lines[table_start]
            .split('|')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();

        if headers.is_empty() {
            i = table_end;
            continue;
        }

        // Parse data rows (skip separator at table_start + 1)
        let mut rows = Vec::new();
        for row_idx in (table_start + 2)..table_end {
            let cells: Vec<String> = lines[row_idx]
                .split('|')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
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
    let safe_name = ledger::create_table(
        &slug,
        &fields_json,
        None,       // unique
        false,      // auto_increment
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
}
