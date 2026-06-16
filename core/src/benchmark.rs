//! RAG quality benchmarking (BEIR + RAGAS-lite).

use crate::error::WikiResult;
use crate::search;

pub fn benchmark_retrieval(file: &str, top_k: usize) -> WikiResult<serde_json::Value> {
    let content = std::fs::read_to_string(file)?;
    let mut total = 0usize;
    let mut hits = 0usize;
    let mut mrr_sum = 0.0f64;

    let streams: std::collections::HashSet<String> = ["bm25", "metadata", "graph"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let qa: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| crate::error::WikiError::Parse(format!("JSONL parse: {e}")))?;
        let query = qa["query"].as_str().unwrap_or("");
        let expected = qa["expected_id"].as_str().unwrap_or("");

        let results = search::search(query, &streams, top_k);
        total += 1;

        for (rank, result) in results.iter().enumerate() {
            if result.id == expected {
                hits += 1;
                mrr_sum += 1.0 / (rank + 1) as f64;
                break;
            }
        }
    }

    Ok(serde_json::json!({
        "total_queries": total,
        "hits": hits,
        "hit_rate": if total > 0 { hits as f64 / total as f64 } else { 0.0 },
        "mrr": if total > 0 { mrr_sum / total as f64 } else { 0.0 },
    }))
}
