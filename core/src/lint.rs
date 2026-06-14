//! Wiki health checker: orphans, stale claims, broken wikilinks, contradictions.

use std::collections::HashSet;
use std::fs;

use crate::config::get_pages_dir;
use crate::error::WikiResult;
use crate::graph;
use crate::types::LintIssue;

pub fn find_orphans() -> Vec<LintIssue> {
    let entities = graph::load_entities();
    let edges = graph::load_edges();
    let mut has_edge: HashSet<String> = HashSet::new();
    for e in &edges {
        has_edge.insert(e.source.clone());
        has_edge.insert(e.target.clone());
    }
    entities.iter()
        .filter(|(id, _)| !has_edge.contains(*id))
        .map(|(id, e)| LintIssue {
            issue_type: "orphan".into(),
            entity_id: Some(id.clone()),
            name: Some(e.name.clone()),
            description: format!("Entity '{}' has no connections", e.name),
            severity: "medium".into(),
        })
        .collect()
}

pub fn find_stale_claims() -> Vec<LintIssue> {
    let entities = graph::load_entities();
    let now = chrono::Utc::now();
    entities.iter()
        .filter_map(|(id, e)| {
            let last = e.last_confirmed.as_deref()?;
            let dt = chrono::DateTime::parse_from_rfc3339(last).ok()?;
            let utc_dt = dt.with_timezone(&chrono::Utc);
            let days = (now - utc_dt).num_days();
            if days > 90 {
                Some(LintIssue {
                    issue_type: "stale".into(),
                    entity_id: Some(id.clone()),
                    name: Some(e.name.clone()),
                    description: format!("Not confirmed in {days} days"),
                    severity: if days > 365 { "high".into() } else { "low".into() },
                })
            } else {
                None
            }
        })
        .collect()
}

pub fn find_broken_links() -> Vec<LintIssue> {
    let entities = graph::load_entities();
    let valid_ids: HashSet<String> = entities.keys().cloned().collect();
    let mut broken = Vec::new();

    for subdir in &["entities", "concepts", "decisions", "sessions"] {
        let dir = get_pages_dir().join(subdir);
        if !dir.exists() { continue; }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }
                let content = fs::read_to_string(&path).unwrap_or_default();
                let re = regex::Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
                for cap in re.captures_iter(&content) {
                    let target = cap[1].split('|').next().unwrap_or(&cap[1]).trim();
                    if !valid_ids.contains(target) {
                        broken.push(LintIssue {
                            issue_type: "broken_link".into(),
                            entity_id: None,
                            name: Some(target.to_string()),
                            description: format!("Broken link in {}", path.display()),
                            severity: "medium".into(),
                        });
                    }
                }
            }
        }
    }
    broken
}

pub fn find_contradictions() -> Vec<LintIssue> {
    let entities = graph::load_entities();
    let mut by_name: std::collections::HashMap<String, Vec<(String, f64)>> = std::collections::HashMap::new();
    for (id, e) in &entities {
        by_name.entry(e.name.to_lowercase()).or_default().push((id.clone(), e.confidence));
    }

    let mut contradictions = Vec::new();
    for (name, entries) in &by_name {
        if entries.len() < 2 { continue; }
        let confs: Vec<f64> = entries.iter().map(|(_, c)| *c).collect();
        let min_c = confs.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_c = confs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        if max_c - min_c > 0.3 {
            contradictions.push(LintIssue {
                issue_type: "contradiction".into(),
                entity_id: Some(entries[0].0.clone()),
                name: Some(name.clone()),
                description: format!("Confidence range {min_c:.2}-{max_c:.2} — consider merging"),
                severity: "high".into(),
            });
        }
    }
    contradictions
}

pub fn run_lint(auto_heal: bool) -> WikiResult<String> {
    let orphans = find_orphans();
    let stale = find_stale_claims();
    let broken = find_broken_links();
    let contradictions = find_contradictions();

    let total = orphans.len() + stale.len() + broken.len() + contradictions.len();
    let mut report = format!(
        "# Wiki Health Report\n\n**Date:** {}\n\n## Summary\n- Issues found: **{total}**\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    );

    if auto_heal && total == 0 {
        report.push_str("✅ No issues found.\n");
        return Ok(report);
    }

    for (label, issues) in &[
        ("Orphans", &orphans),
        ("Stale Claims", &stale),
        ("Broken Links", &broken),
        ("Contradictions", &contradictions),
    ] {
        if !issues.is_empty() {
            report.push_str(&format!("## {label} ({})\n\n", issues.len()));
            for issue in issues.iter().take(10) {
                report.push_str(&format!(
                    "- 🔴 **{}**: {}\n",
                    issue.name.as_deref().unwrap_or("?"),
                    issue.description
                ));
            }
            report.push('\n');
        }
    }

    Ok(report)
}
