//! Entity extraction / crystallization pipeline.
//! LLM-driven structured fact extraction from new/updated sources.

use crate::error::WikiResult;
use crate::llm;

pub fn extract_facts(content: &str) -> WikiResult<serde_json::Value> {
    let system = "You are a fact extraction engine. Extract structured entities and relationships from text. Output valid JSON.";
    let user = format!(
        "Extract entities and relationships from:\n\n{content}\n\nOutput JSON: {{\"entities\": [{{\"id\": \"...\", \"type\": \"...\", \"name\": \"...\", \"confidence\": 0.0-1.0}}], \"edges\": [{{\"source\": \"...\", \"target\": \"...\", \"type\": \"...\", \"description\": \"...\"}}]}}"
    );

    let response = llm::call_llm_default(system, &user)?;
    // Extract JSON from response
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            &response
        }
    } else {
        &response
    };

    serde_json::from_str(json_str)
        .map_err(|e| crate::error::WikiError::Parse(format!("JSON parse: {e}")))
}
