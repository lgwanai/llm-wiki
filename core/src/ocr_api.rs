//! API-based OCR and vision analysis for images.
//! Supports OpenAI-compatible vision APIs (GPT-4o, DeepSeek-VL2, etc.).

use std::fs;
use std::path::Path;

use crate::error::{WikiError, WikiResult};
use crate::types::ImageAnalysisConfig;

/// Analyze an image using a vision model API.
pub fn analyze_image(path: &Path, config: &ImageAnalysisConfig) -> WikiResult<String> {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let bytes = fs::read(path)?;
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    let data_uri = format!("data:{mime};base64,{encoded}");

    let prompt = if config.api_prompt.is_empty() {
        "Describe this image in detail. Extract all visible text, structure, and key information.".to_string()
    } else {
        config.api_prompt.clone()
    };

    let payload = serde_json::json!({
        "model": config.api_model,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image_url", "image_url": {"url": &data_uri}},
                {"type": "text", "text": &prompt}
            ]
        }],
        "max_tokens": 16384,
    });

    let api_url = if !config.api_url.is_empty() {
        config.api_url.clone()
    } else {
        format!("https://api.openai.com/v1/chat/completions")
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::CONTENT_TYPE, reqwest::header::HeaderValue::from_static("application/json"));
    if let Ok(auth) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", config.api_key)) {
        headers.insert(reqwest::header::AUTHORIZATION, auth);
    }

    let resp: serde_json::Value = client
        .post(&api_url)
        .headers(headers)
        .json(&payload)
        .send()?
        .json()?;

    let content = resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| WikiError::Ocr("Empty vision API response".into()))?;

    Ok(content.to_string())
}

/// OCR an image using the configured OCR API backend.
pub fn ocr_image(path: &Path) -> WikiResult<String> {
    let ocr_config = crate::config::get_ocr_config();

    if ocr_config.mode == "api" || ocr_config.backend == "api" {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        let bytes = fs::read(path)?;
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
        let data_uri = format!("data:{mime};base64,{encoded}");

        let payload = serde_json::json!({
            "model": ocr_config.api_model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": &data_uri}},
                    {"type": "text", "text": &ocr_config.api_prompt}
                ]
            }],
            "max_tokens": 16384,
        });

        let api_url = if !ocr_config.api_url.is_empty() {
            ocr_config.api_url.clone()
        } else {
            "https://api.openai.com/v1/chat/completions".to_string()
        };

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::CONTENT_TYPE, reqwest::header::HeaderValue::from_static("application/json"));
        if !ocr_config.api_key.is_empty() {
            if let Ok(auth) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", ocr_config.api_key)) {
                headers.insert(reqwest::header::AUTHORIZATION, auth);
            }
        }

        let resp: serde_json::Value = client.post(&api_url).headers(headers).json(&payload).send()?.json()?;
        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| WikiError::Ocr("Empty OCR API response".into()))?;
        return Ok(content.to_string());
    }

    // Local OCR not available in Rust — suggest API mode
    Err(WikiError::Ocr(
        "Local OCR backends are not supported in the Rust version. Please set ocr.mode to 'api' in wiki_config.yaml and configure an API provider.".into(),
    ))
}
