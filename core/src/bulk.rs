//! Bulk operations: stats, clean, merge, export, delete.

use crate::config::get_pages_dir;
use crate::error::WikiResult;
use crate::types::LintIssue;

pub fn bulk_stats() -> WikiResult<serde_json::Value> {
    let pages_dir = get_pages_dir();
    let mut concepts = 0usize;
    let mut entities = 0usize;
    let mut total_size = 0u64;
    for subdir in &["concepts", "entities"] {
        let dir = pages_dir.join(subdir);
        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("md") {
                        match *subdir {
                            "concepts" => concepts += 1,
                            "entities" => entities += 1,
                            _ => {}
                        }
                        if let Ok(meta) = std::fs::metadata(&path) {
                            total_size += meta.len();
                        }
                    }
                }
            }
        }
    }
    Ok(serde_json::json!({
        "pages": {"concepts": concepts, "entities": entities, "total": concepts + entities},
        "total_size_kb": total_size / 1024,
    }))
}

pub fn clean_orphans(dry_run: bool) -> WikiResult<Vec<LintIssue>> {
    let orphans = crate::lint::find_orphans();
    if !dry_run {
        for orphan in &orphans {
            if let Some(ref id) = orphan.entity_id {
                for subdir in &["concepts", "entities"] {
                    let path = get_pages_dir().join(subdir).join(format!("{id}.md"));
                    if path.exists() {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
    }
    Ok(orphans)
}
