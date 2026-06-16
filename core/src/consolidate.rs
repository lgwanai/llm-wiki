//! Memory tier consolidation with Ebbinghaus retention decay.
//! Three tiers: working → episodic → semantic.

use std::fs;

use crate::config::get_memory_dir;
use crate::error::WikiResult;

const DECAY_S_VALUES: &[(&str, f64)] = &[
    ("architecture", 260.0),
    ("project", 130.0),
    ("bug", 20.0),
    ("meeting", 10.0),
    ("pattern", 87.0),
    ("preference", 527.0),
];

fn memory_path(filename: &str) -> std::path::PathBuf {
    get_memory_dir().join(filename)
}

fn load_json_list(path: &std::path::Path) -> Vec<serde_json::Value> {
    if !path.exists() {
        return vec![];
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_json(path: &std::path::Path, data: &serde_json::Value) -> WikiResult<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(path, serde_json::to_string_pretty(data)?)?;
    Ok(())
}

fn days_since(iso_str: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso_str)
        .map(|dt| {
            let utc_dt = dt.with_timezone(&chrono::Utc);
            (chrono::Utc::now() - utc_dt).num_days().max(0)
        })
        .unwrap_or(999)
}

pub fn apply_retention_decay() -> WikiResult<serde_json::Value> {
    let semantic_path = memory_path("semantic.json");
    let semantic: Vec<serde_json::Value> = load_json_list(&semantic_path);
    if semantic.is_empty() {
        return Ok(serde_json::json!({"decayed": 0, "archived": 0}));
    }

    let mut active = Vec::new();
    let mut archived = Vec::new();
    let mut decayed = 0;

    for fact in &semantic {
        let entity_id = fact.get("entity_id").and_then(|v| v.as_str()).unwrap_or("");
        let mut fact_type = "project";

        if entity_id.contains("bug") || entity_id.contains("fix") {
            fact_type = "bug";
        } else if entity_id.contains("meeting") {
            fact_type = "meeting";
        } else if entity_id.contains("pattern") {
            fact_type = "pattern";
        } else if entity_id.contains("decision") || entity_id.contains("arch") {
            fact_type = "architecture";
        }

        let s = DECAY_S_VALUES
            .iter()
            .find(|(k, _)| *k == fact_type)
            .map(|(_, v)| *v)
            .unwrap_or(130.0);
        let last = fact
            .get("last_confirmed")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let days = days_since(last);
        let retention = (-days as f64 / s).exp();

        if retention < 0.15 {
            archived.push(fact.clone());
        } else {
            let mut f = fact.clone();
            if retention < 0.3 {
                f["deprioritized"] = serde_json::Value::Bool(true);
            }
            active.push(f);
        }
        decayed += 1;
    }

    let active_json = serde_json::json!(active);
    save_json(&semantic_path, &active_json)?;
    let archived_count = archived.len();
    if !archived.is_empty() {
        let archive_path = memory_path("semantic.json.archived");
        let mut existing: Vec<serde_json::Value> = load_json_list(&archive_path);
        existing.extend(archived);
        let existing_json = serde_json::json!(existing);
        save_json(&archive_path, &existing_json)?;
    }

    Ok(serde_json::json!({"decayed": decayed, "archived": archived_count}))
}

pub fn promote_working_to_episodic() -> WikiResult<usize> {
    let working_path = memory_path("working.json");
    let working: Vec<serde_json::Value> = load_json_list(&working_path);
    if working.len() < 5 {
        return Ok(0);
    }

    // Group by date
    let mut groups: std::collections::HashMap<String, Vec<&serde_json::Value>> =
        std::collections::HashMap::new();
    for obs in &working {
        let ts = obs.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        groups
            .entry(ts.chars().take(10).collect())
            .or_default()
            .push(obs);
    }

    let episodes_path = memory_path("episodic.json");
    let mut episodes: Vec<serde_json::Value> = load_json_list(&episodes_path);
    let mut promoted = 0;
    let mut remaining: Vec<serde_json::Value> = Vec::new();

    for (date_key, group) in groups {
        if group.len() >= 5 {
            let empty_vec = vec![];
            let entity_ids: Vec<String> = group
                .iter()
                .flat_map(|obs| {
                    obs.get("entity_ids")
                        .and_then(|v| v.as_array())
                        .unwrap_or(&empty_vec)
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                })
                .collect();
            episodes.push(serde_json::json!({
                "id": format!("episode-{date_key}"),
                "date": date_key,
                "summary": format!("Consolidated {} observations", group.len()),
                "entities": entity_ids,
                "confidence": 0.5,
                "created_at": chrono::Utc::now().to_rfc3339(),
            }));
            promoted += group.len();
        } else {
            for obs in group {
                remaining.push(obs.clone());
            }
        }
    }

    let remaining_json = serde_json::json!(remaining);
    save_json(&working_path, &remaining_json)?;
    let episodes_json = serde_json::json!(episodes);
    save_json(&episodes_path, &episodes_json)?;
    Ok(promoted)
}

pub fn consolidate(tiers: &str, decay_only: bool) -> WikiResult<serde_json::Value> {
    let mut result = serde_json::json!({});

    if decay_only {
        let r = apply_retention_decay()?;
        return Ok(r);
    }

    let tier_list: Vec<&str> = tiers.split(',').map(|s| s.trim()).collect();

    if tier_list.contains(&"working") {
        let n = promote_working_to_episodic()?;
        result["working_to_episodic"] = serde_json::json!(n);
    }

    if tier_list.contains(&"semantic") {
        let r = apply_retention_decay()?;
        result["decay"] = r;
    }

    Ok(result)
}
