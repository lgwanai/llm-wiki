//! LLM API utilities — unified API call with retry logic.
//!
//! Handles provider-specific payloads (OpenAI-compatible, Ollama, Custom) and
//! automatic retry with exponential backoff on transient failures.

use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use reqwest::blocking::Client;

use crate::config::{get_api_url, get_llm_config};
use crate::error::{WikiError, WikiResult};

// ═══════════════════════════════════════════════════════════════════════════
// Model context window detection
// ═══════════════════════════════════════════════════════════════════════════

/// Known context windows by model family (tokens). Conservative lower bounds.
const MODEL_CONTEXT_FALLBACK: u32 = 131072; // 128K

fn model_context_map() -> HashMap<&'static str, u32> {
    HashMap::from([
        // DeepSeek family
        ("deepseek-v4", 131072),
        ("deepseek-v3", 65536),
        ("deepseek-r1", 131072),
        ("deepseek-chat", 65536),
        ("deepseek-coder", 65536),
        ("deepseek-v2", 131072),
        // OpenAI family
        ("gpt-4o", 131072),
        ("gpt-4-turbo", 131072),
        ("gpt-4", 8192),
        ("gpt-4-32k", 32768),
        ("gpt-3.5-turbo", 16384),
        ("o1", 200000),
        ("o3", 200000),
        // Anthropic family
        ("claude", 200000),
        // Meta family
        ("llama3.2", 131072),
        ("llama3.1", 131072),
        ("llama3", 8192),
        ("llama2", 4096),
        // Qwen family
        ("qwen3", 131072),
        ("qwen2.5", 131072),
        ("qwen2", 32768),
        ("qwen", 32768),
        // Mistral family
        ("mistral-large", 131072),
        ("mistral-small", 32768),
        ("mistral", 32768),
        ("mixtral", 32768),
        // Google family
        ("gemini-2", 1048576),
        ("gemini-1.5", 1048576),
        ("gemini", 32768),
        // Yi family
        ("yi", 200000),
    ])
}

const PROMPT_OVERHEAD_ESTIMATE: u32 = 4000;

/// Detect the model's maximum context window (tokens).
pub fn get_model_max_context() -> u32 {
    let llm_config = get_llm_config();

    // Explicit config overrides
    if llm_config.num_ctx > 0 {
        return llm_config.num_ctx;
    }

    let model_name = llm_config.model.to_lowercase();
    let context_map = model_context_map();

    // Match by model family prefix (most specific first)
    let mut sorted_keys: Vec<&&str> = context_map.keys().collect();
    sorted_keys.sort_by_key(|k| -(k.len() as i32));

    for prefix in sorted_keys {
        if model_name.starts_with(prefix) || model_name.contains(prefix) {
            return context_map[prefix];
        }
    }

    MODEL_CONTEXT_FALLBACK
}

/// Return the token threshold above which documents should be chunked.
pub fn get_chunk_threshold(override_threshold: Option<u32>) -> u32 {
    if let Some(t) = override_threshold {
        if t > 0 {
            return t;
        }
    }

    let max_ctx = get_model_max_context();
    let usable = max_ctx.saturating_sub(PROMPT_OVERHEAD_ESTIMATE);
    let threshold = (usable as f64 * 0.6) as u32;
    threshold.max(4000) // floor at 4K
}

// ═══════════════════════════════════════════════════════════════════════════
// Main LLM call
// ═══════════════════════════════════════════════════════════════════════════

/// Call the configured LLM with automatic retry on transient failures.
///
/// Retries with exponential backoff on network errors and 429 rate limits.
/// Non-retryable errors (401, 403) are raised immediately.
pub fn call_llm(
    system_prompt: &str,
    user_content: &str,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    timeout_secs: u64,
    max_retries: u32,
) -> WikiResult<String> {
    let llm_config = get_llm_config();
    let provider = llm_config.provider.as_str();

    // Build provider-specific payload and headers
    let (api_url, payload, headers) = match provider {
        "ollama" => {
            let api_url = format!("{}/api/chat", llm_config.base_url.trim_end_matches('/'));
            let payload = serde_json::json!({
                "model": llm_config.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_content},
                ],
                "stream": false,
                "options": {
                    "temperature": temperature.unwrap_or(llm_config.temperature),
                    "num_ctx": llm_config.num_ctx,
                },
            });
            let headers = reqwest::header::HeaderMap::new();
            (api_url, payload, headers)
        }
        "custom" => {
            let api_url = get_api_url();
            let payload = serde_json::json!({
                "model": llm_config.model,
                "temperature": temperature.unwrap_or(llm_config.temperature),
                "max_tokens": max_tokens.unwrap_or(llm_config.max_tokens),
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_content},
                ],
            });
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_static("application/json"),
            );
            if !llm_config.api_key.is_empty() {
                if let Ok(auth) = reqwest::header::HeaderValue::from_str(&format!(
                    "Bearer {}",
                    llm_config.api_key
                )) {
                    headers.insert(reqwest::header::AUTHORIZATION, auth);
                }
            }
            (api_url, payload, headers)
        }
        _ => {
            // deepseek, openai, or any OpenAI-compatible provider
            let api_url = get_api_url();

            if llm_config.api_key.is_empty() {
                return Err(WikiError::Llm(
                    "LLM API key not configured. Set model.api_key in wiki_config.yaml.".into(),
                ));
            }

            let mut payload = serde_json::json!({
                "model": llm_config.model,
                "temperature": temperature.unwrap_or(llm_config.temperature),
                "max_tokens": max_tokens.unwrap_or(llm_config.max_tokens),
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_content},
                ],
            });

            // Disable thinking for DeepSeek models
            if provider == "deepseek" {
                payload["thinking"] = serde_json::json!({"type": "disabled"});
            }

            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_static("application/json"),
            );
            if let Ok(auth) =
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", llm_config.api_key))
            {
                headers.insert(reqwest::header::AUTHORIZATION, auth);
            }

            (api_url, payload, headers)
        }
    };

    // ── Retry loop ──
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| WikiError::Llm(format!("Failed to create HTTP client: {e}")))?;

    for attempt in 0..max_retries {
        let request = client
            .post(&api_url)
            .headers(headers.clone())
            .json(&payload);

        match request.send() {
            Ok(resp) => {
                let status = resp.status();

                // Rate limit — retry with exponential backoff
                if status.as_u16() == 429 {
                    if attempt < max_retries - 1 {
                        let wait = (2u64.pow(attempt) * 3).min(30);
                        eprintln!(
                            "LLM rate limited (429), retrying in {}s... (attempt {}/{})",
                            wait,
                            attempt + 1,
                            max_retries
                        );
                        thread::sleep(Duration::from_secs(wait));
                        continue;
                    }
                    return Err(WikiError::Llm(format!(
                        "LLM API rate limited after {} attempts",
                        max_retries
                    )));
                }

                // Auth errors — do NOT retry
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return Err(WikiError::Llm(format!(
                        "LLM API authentication failed ({}). Check your API key.",
                        status.as_u16()
                    )));
                }

                // Check for other errors
                if !status.is_success() {
                    let body = resp.text().unwrap_or_default();
                    return Err(WikiError::Llm(format!(
                        "LLM API returned error {}: {}",
                        status.as_u16(),
                        body
                    )));
                }

                // Parse response
                let data: serde_json::Value = resp
                    .json()
                    .map_err(|e| WikiError::Llm(format!("LLM API returned invalid JSON: {e}")))?;

                // Extract content based on provider format
                let content = if provider == "ollama" {
                    data.get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string()
                } else {
                    data["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string()
                };

                if content.is_empty() {
                    return Err(WikiError::Llm("LLM returned empty response content".into()));
                }

                return Ok(content.trim().to_string());
            }
            Err(e) => {
                if attempt < max_retries - 1 {
                    let wait = (2u64.pow(attempt) * 2).min(15);
                    eprintln!(
                        "LLM call attempt {}/{} failed: {}, retrying in {}s...",
                        attempt + 1,
                        max_retries,
                        e,
                        wait
                    );
                    thread::sleep(Duration::from_secs(wait));
                } else {
                    return Err(WikiError::Llm(format!(
                        "LLM API call failed after {} attempts: {}",
                        max_retries, e
                    )));
                }
            }
        }
    }

    Err(WikiError::Llm(format!(
        "LLM API call failed after {} attempts",
        max_retries
    )))
}

/// Convenience wrapper with default parameters.
pub fn call_llm_default(system_prompt: &str, user_content: &str) -> WikiResult<String> {
    call_llm(system_prompt, user_content, None, None, 120, 3)
}

/// Streaming LLM call — calls callback with each token.
pub fn call_llm_stream(
    system_prompt: &str,
    user_content: &str,
    mut on_token: impl FnMut(&str),
) -> WikiResult<()> {
    let llm_config = get_llm_config();
    let provider = llm_config.provider.as_str();
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| WikiError::Llm(format!("Client: {e}")))?;

    if provider == "ollama" {
        let payload = serde_json::json!({
            "model": llm_config.model, "stream": true,
            "messages": [{"role":"system","content":system_prompt},{"role":"user","content":user_content}],
            "options": {"temperature": llm_config.temperature, "num_ctx": llm_config.num_ctx},
        });
        let resp = client
            .post(format!(
                "{}/api/chat",
                llm_config.base_url.trim_end_matches('/')
            ))
            .json(&payload)
            .send()?;
        for line in std::io::BufRead::lines(std::io::BufReader::new(resp)) {
            if let Ok(line) = line {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(t) = v["message"]["content"].as_str() {
                        on_token(t);
                    }
                }
            }
        }
    } else {
        let api_url = get_api_url();
        let mut payload = serde_json::json!({
            "model": llm_config.model, "temperature": llm_config.temperature, "max_tokens": llm_config.max_tokens.min(4096), "stream": true,
            "messages": [{"role":"system","content":system_prompt},{"role":"user","content":user_content}],
        });
        if provider == "deepseek" {
            payload["thinking"] = serde_json::json!({"type":"disabled"});
        }
        let resp = client
            .post(&api_url)
            .header("Authorization", format!("Bearer {}", llm_config.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()?;
        for line in std::io::BufRead::lines(std::io::BufReader::new(resp)) {
            if let Ok(line) = line {
                let line = line.trim().to_string();
                if line == "[DONE]" || line.is_empty() {
                    continue;
                }
                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
                            on_token(t);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Token estimation
// ═══════════════════════════════════════════════════════════════════════════

/// Estimate token count from text length and language.
/// Rough heuristic: ~4 chars/token for English, ~2 CJK chars/token.
pub fn estimate_tokens(text: &str, lang: &str) -> usize {
    if lang == "zh" {
        let cjk_count = text
            .chars()
            .filter(|c| {
                let cp = *c as u32;
                (0x4E00..=0x9FFF).contains(&cp) || (0x3400..=0x4DBF).contains(&cp)
            })
            .count();
        let non_cjk = text.chars().count() - cjk_count;
        cjk_count / 2 + non_cjk / 4
    } else {
        text.chars().count() / 4
    }
}

/// Detect language: returns "zh" for Chinese, "en" otherwise.
pub fn detect_language(text: &str) -> &'static str {
    let total = text.chars().count();
    if total == 0 {
        return "en";
    }
    let cjk_count = text
        .chars()
        .filter(|c| {
            let cp = *c as u32;
            (0x4E00..=0x9FFF).contains(&cp) || (0x3400..=0x4DBF).contains(&cp)
        })
        .count();
    if cjk_count as f64 / total as f64 > 0.08 {
        "zh"
    } else {
        "en"
    }
}
