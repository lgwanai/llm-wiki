//! Query engine: multi-stream search + LLM synthesis + formatting.

use std::collections::HashSet;

use crate::config::get_query_config;
use crate::error::WikiResult;
use crate::graph;
use crate::llm;
use crate::search::{self};
use crate::types::{QueryAnswer, QueryPlan, SearchResult, SourceCitation};

const DEFAULT_STREAMS: &[&str] = &["metadata", "bm25", "graph"];

pub fn enabled_search_streams() -> HashSet<String> {
    let config = get_query_config();
    let configured = &config.search_streams;
    if configured.is_empty() || configured == "all" || configured == "*" {
        return DEFAULT_STREAMS.iter().map(|s| s.to_string()).collect();
    }
    configured
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn plan_query(query: &str) -> QueryPlan {
    let q = query.to_lowercase();
    let ledger_terms = ["表", "台账", "预算", "row", "table", "ledger", "sql"];
    let graph_terms = [
        "影响",
        "依赖",
        "关系",
        "路径",
        "impact",
        "depends",
        "relationship",
    ];

    let intent = if ledger_terms.iter().any(|t| q.contains(t)) {
        "ledger_filter"
    } else if graph_terms.iter().any(|t| q.contains(t)) {
        "relationship"
    } else {
        "fact"
    };

    let preferred = match intent {
        "ledger_filter" => vec![
            "ledger".to_string(),
            "metadata".to_string(),
            "bm25".to_string(),
        ],
        "relationship" => vec![
            "graph".to_string(),
            "metadata".to_string(),
            "bm25".to_string(),
        ],
        _ => vec![
            "metadata".to_string(),
            "bm25".to_string(),
            "graph".to_string(),
        ],
    };

    let keywords: Vec<String> = query.split_whitespace().map(|s| s.to_string()).collect();
    QueryPlan {
        intent: intent.to_string(),
        preferred_streams: preferred,
        keywords,
    }
}

pub fn query_wiki(
    question: &str,
    synthesis: bool,
    fmt: &str,
    debug_search: bool,
) -> WikiResult<QueryAnswer> {
    let streams = enabled_search_streams();
    let plan = plan_query(question);
    let max_results = get_query_config().max_results;

    let results = search::search(question, &streams, max_results);

    // Check for graph relationship intent
    if plan.intent == "relationship" {
        if let (Some(source), Some(target)) = extract_graph_query(question) {
            if let Some(path) = graph::find_path(&source, &target) {
                let path_desc: Vec<String> = path
                    .iter()
                    .map(|e| format!("{} ->[{}]-> {}", e.source, e.rel_type, e.target))
                    .collect();
                return Ok(QueryAnswer {
                    question: question.to_string(),
                    answer: format!("Path found:\n{}", path_desc.join("\n")),
                    format: "graph".into(),
                    sources: vec![],
                    debug_search: None,
                });
            }
        }
    }

    let answer = if synthesis && !results.is_empty() {
        synthesize(question, &results)?
    } else {
        format_results(&results, fmt)
    };

    let sources: Vec<SourceCitation> = results
        .iter()
        .map(|r| SourceCitation {
            id: r.id.clone(),
            name: r.title.clone().unwrap_or_else(|| r.id.clone()),
            path: r.path.to_string_lossy().to_string(),
            page_type: r.entity_type.clone().unwrap_or_else(|| "unknown".into()),
            relevance: r.rrf_score.unwrap_or(r.score),
        })
        .collect();

    Ok(QueryAnswer {
        question: question.to_string(),
        answer,
        format: fmt.to_string(),
        sources,
        debug_search: if debug_search {
            Some(serde_json::json!({"results": results}))
        } else {
            None
        },
    })
}

fn synthesize(question: &str, results: &[SearchResult]) -> WikiResult<String> {
    let mut context = String::new();
    for (i, r) in results.iter().enumerate() {
        let content = std::fs::read_to_string(&r.path).unwrap_or_default();
        let body = if content.starts_with("---") {
            content[4..]
                .find("\n---")
                .map(|e| content[4 + e + 4..].to_string())
                .unwrap_or(content)
        } else {
            content
        };
        context.push_str(&format!(
            "\n### Source {}: {}\n{}\n",
            i + 1,
            r.title.as_deref().unwrap_or(&r.id),
            &body[..body.len().min(2000)]
        ));
    }

    let system = "You are a precise knowledge assistant. Answer questions based ONLY on the provided wiki context. Cite sources by number. If the context doesn't contain the answer, say so clearly.";
    let user = format!("Question: {question}\n\nRelevant wiki pages:{context}\n\nAnswer the question concisely based on the above context. Cite sources like [1], [2].");

    llm::call_llm_default(system, &user)
}

fn format_results(results: &[SearchResult], fmt: &str) -> String {
    match fmt {
        "json" => serde_json::to_string_pretty(results).unwrap_or_default(),
        "table" => {
            let mut out =
                String::from("| # | Title | Type | Score |\n|---|-------|------|-------|\n");
            for (i, r) in results.iter().enumerate() {
                out.push_str(&format!(
                    "| {} | {} | {} | {:.3} |\n",
                    i + 1,
                    r.title.as_deref().unwrap_or(&r.id),
                    r.entity_type.as_deref().unwrap_or("-"),
                    r.rrf_score.unwrap_or(r.score)
                ));
            }
            out
        }
        _ => {
            let mut out = String::new();
            for (i, r) in results.iter().enumerate() {
                out.push_str(&format!(
                    "## {}. {}\n",
                    i + 1,
                    r.title.as_deref().unwrap_or(&r.id)
                ));
                if let Some(summary) = &r.summary {
                    out.push_str(&format!("_{summary}_\n\n"));
                }
                out.push_str(&format!("- Score: {:.3}\n", r.rrf_score.unwrap_or(r.score)));
                out.push_str(&format!("- Path: {}\n\n", r.path.display()));
            }
            out
        }
    }
}

fn extract_graph_query(query: &str) -> (Option<String>, Option<String>) {
    let parts: Vec<&str> = query.split_whitespace().collect();
    let mut source = None;
    let mut target = None;
    let mut found_from = false;
    for part in &parts {
        let p = part
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
            .to_string();
        if p.is_empty() {
            continue;
        }
        if found_from && target.is_none() {
            target = Some(p);
        } else if p == "from" || p == "between" {
            found_from = true;
        } else if p == "to" || p == "and" {
            found_from = false;
        } else if source.is_none() {
            source = Some(p);
        }
    }
    (source, target)
}
