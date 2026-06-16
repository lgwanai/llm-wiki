//! Response parser for LLM compile output.
//! Parses ===PAGE_END=== delimited multi-page responses with YAML frontmatter.
//! Handles LLM preambles and multiple frontmatter blocks per section.

use crate::error::{WikiError, WikiResult};
use crate::types::PageType;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ParsedPage {
    pub id: String,
    pub page_type: PageType,
    pub frontmatter: HashMap<String, serde_json::Value>,
    pub body: String,
}

/// Parse an LLM compile response into structured wiki pages.
pub fn parse_compile_response(response: &str, _lang: &str) -> WikiResult<Vec<ParsedPage>> {
    let mut pages = Vec::new();
    let sections: Vec<&str> = response.split("===PAGE_END===").collect();

    for section in sections {
        let trimmed = section.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Parse all frontmatter blocks in this section
        let extracted = parse_section(trimmed);
        pages.extend(extracted);
    }

    if pages.is_empty() {
        return Err(WikiError::Parse(
            "LLM response contained no valid pages — no ===PAGE_END=== delimiters found".into(),
        ));
    }
    Ok(pages)
}

/// Extract all pages from a single section (may contain multiple frontmatter blocks).
fn parse_section(section: &str) -> Vec<ParsedPage> {
    let mut pages = Vec::new();

    // Find all `---\n...\n---` frontmatter blocks regardless of position
    let mut remaining = section;
    let _prev_body_end = 0usize;

    let mut safety = 0;
    loop {
        safety += 1;
        if safety > 100 {
            break;
        } // prevent infinite loops

        // Find next `---` at start of a line
        let fm_start = match find_frontmatter_start(remaining) {
            Some(pos) => pos,
            None => break,
        };

        // Text between previous block and this one is body (preamble)
        // (only for first block; subsequent blocks get their own body)

        // Find matching closing `---`
        let after_open = &remaining[fm_start + 3..];
        let fm_end = match find_frontmatter_end(after_open) {
            Some(pos) => fm_start + 3 + pos,
            None => break,
        };

        let fm_str = &remaining[fm_start + 3..fm_end].trim();
        let after_close = &remaining[fm_end + 3..];

        // Find body: everything until next `---` block or end of section
        let body_end = find_frontmatter_start(after_close).unwrap_or(after_close.len());
        let body = after_close[..body_end].trim().to_string();

        remaining = &after_close[body_end..];

        // Parse frontmatter
        let fm = parse_frontmatter(fm_str);
        if fm.is_empty() {
            continue;
        }

        let page_type = fm
            .get("type")
            .and_then(|v| v.as_str())
            .map(|t| {
                if t.to_lowercase() == "entity" {
                    PageType::Entity
                } else {
                    PageType::Concept
                }
            })
            .unwrap_or(PageType::Concept);

        // Extract ID: prefer `id` > slugified `name` > first heading > content hash
        let id = fm
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| fm.get("name").and_then(|v| v.as_str()).map(|n| slugify(n)))
            .or_else(|| extract_first_heading(&body).map(|h| slugify(&h)))
            .unwrap_or_else(|| {
                // Deterministic hash-based ID — same content = same ID, no UUID spam
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                body.hash(&mut h);
                format!("doc-{:x}", h.finish())
            });

        pages.push(ParsedPage {
            id,
            page_type,
            frontmatter: fm,
            body,
        });
    }

    pages
}

/// Find `---` at the start of a line (after optional whitespace).
fn find_frontmatter_start(text: &str) -> Option<usize> {
    let mut pos = 0usize;
    for line in text.split_inclusive('\n') {
        if line.trim() == "---" {
            return Some(pos);
        }
        pos += line.len();
    }
    None
}

/// Find closing `\n---` after the opening `---`.
/// Simply finds the next line that is exactly `---`.
fn find_frontmatter_end(after_open: &str) -> Option<usize> {
    let mut pos = 0usize;
    for line in after_open.lines() {
        if line.trim() == "---" {
            return Some(pos);
        }
        pos += line.len() + 1;
    }
    None
}

/// Extract the first `# heading` from markdown body as a fallback title.
fn extract_first_heading(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            return Some(trimmed[2..].trim().to_string());
        }
    }
    None
}

/// Parse YAML frontmatter using serde_yaml for robustness.
fn parse_frontmatter(fm_str: &str) -> HashMap<String, serde_json::Value> {
    serde_yaml::from_str::<serde_yaml::Value>(fm_str)
        .ok()
        .and_then(|v| {
            v.as_mapping().map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        let key = k.as_str().unwrap_or("").to_string();
                        let val = yaml_to_json(v.clone());
                        (key, val)
                    })
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Convert serde_yaml::Value to serde_json::Value.
pub fn yaml_to_json(v: serde_yaml::Value) -> serde_json::Value {
    match v {
        serde_yaml::Value::String(s) => serde_json::Value::String(s),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::json!(f)
            } else {
                serde_json::Value::String(n.to_string())
            }
        }
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(b),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.into_iter().map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                let key = k.as_str().unwrap_or("").to_string();
                obj.insert(key, yaml_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
        _ => serde_json::Value::Null,
    }
}

/// Slugify preserving CJK characters.
fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                Some(c.to_ascii_lowercase())
            } else if ('\u{4e00}'..='\u{9fff}').contains(&c) {
                Some(c)
            } else {
                None
            }
        })
        .collect::<String>()
        .replace(' ', "-")
        .replace('_', "-");
    let mut result = String::new();
    let mut last_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !last_hyphen && !result.is_empty() {
                result.push('-');
                last_hyphen = true;
            }
        } else {
            result.push(c);
            last_hyphen = false;
        }
    }
    result.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_chinese() {
        assert_eq!(slugify("深度学习"), "深度学习");
        assert_eq!(slugify("Deep Learning"), "deep-learning");
    }

    #[test]
    fn test_parse_with_preamble() {
        // LLM often adds Chinese intro before the frontmatter
        let response = "好的，这是从文档中抽取的知识单元。\n\n---\nid: test-page\ntype: concept\nname: Test Page\n---\n\n# Test\n\nContent here.\n===PAGE_END===";
        let pages = parse_compile_response(response, "zh").unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].id, "test-page");
    }

    #[test]
    fn test_parse_multiple_in_section() {
        let response = "---\nid: page-1\ntype: concept\nname: Page One\n---\n\nBody 1.\n\n---\nid: page-2\ntype: entity\nname: Page Two\n---\n\nBody 2.\n===PAGE_END===";
        let pages = parse_compile_response(response, "en").unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].id, "page-1");
        assert_eq!(pages[1].id, "page-2");
    }

    #[test]
    fn test_fallback_to_heading() {
        let response =
            "---\ntype: concept\n---\n\n# My Cool Topic\n\nSome content.\n===PAGE_END===";
        let pages = parse_compile_response(response, "en").unwrap();
        assert_eq!(pages[0].id, "my-cool-topic");
    }
}
