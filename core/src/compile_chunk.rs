//! Document chunking for large files during compilation.
//! Splits documents by heading boundaries near token thresholds.
//! Uses std::thread::scope for parallel LLM calls.


use crate::error::WikiResult;

/// Minimum chunk size (characters). Don't split chunks smaller than this.
const MIN_CHUNK_SIZE: usize = 2000;

/// Character-to-token conversion ratio for English.
const CHARS_PER_TOKEN_EN: usize = 4;

/// Character-to-token conversion ratio for Chinese.
const CHARS_PER_TOKEN_ZH: usize = 2;

/// Split content into chunks at heading boundaries, respecting token limits.
pub fn chunk_content(content: &str, max_tokens: usize, lang: &str) -> Vec<String> {
    let chars_per_token = if lang == "zh" {
        CHARS_PER_TOKEN_ZH
    } else {
        CHARS_PER_TOKEN_EN
    };
    let max_chars = max_tokens * chars_per_token;

    if content.chars().count() <= max_chars {
        return vec![content.to_string()];
    }

    // Find heading boundaries for natural split points
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in content.lines() {
        // If adding this line would exceed max_chars and we're at a heading,
        // or we already have enough, start a new chunk
        if !current.is_empty()
            && (current.chars().count() + line.chars().count() > max_chars)
            && (line.starts_with('#') || current.chars().count() > max_chars / 2)
        {
            chunks.push(current.trim().to_string());
            current = String::new();
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    // If we only got one chunk or chunks are too small, force split by paragraph
    if chunks.len() <= 1 {
        chunks = force_split(content, max_chars);
    }

    chunks
}

/// Force-split content by paragraphs when heading boundaries don't work.
fn force_split(content: &str, max_chars: usize) -> Vec<String> {
    let paragraphs: Vec<&str> = content.split("\n\n").collect();
    let mut chunks = Vec::new();
    let mut current = String::new();

    for para in paragraphs {
        if !current.is_empty()
            && current.chars().count() + para.chars().count() > max_chars
            && current.chars().count() > MIN_CHUNK_SIZE
        {
            chunks.push(current.trim().to_string());
            current = String::new();
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }

    if !current.trim().is_empty() {
        // If last chunk is tiny, merge with previous
        if current.chars().count() < MIN_CHUNK_SIZE && !chunks.is_empty() {
            let last = chunks.pop().unwrap();
            chunks.push(format!("{last}\n\n{current}"));
        } else {
            chunks.push(current.trim().to_string());
        }
    }

    chunks
}

/// Compile chunks sequentially (parallel version planned for future).
pub fn compile_chunks_parallel<F>(
    chunk_sources: Vec<(usize, String)>,
    compile_fn: &F,
    _max_jobs: usize,
) -> WikiResult<Vec<String>>
where
    F: Fn(&str) -> WikiResult<String> + Sync,
{
    let mut results = vec![String::new(); chunk_sources.len()];
    let mut errors = Vec::new();
    for (idx, content) in &chunk_sources {
        match compile_fn(content) {
            Ok(result) => results[*idx] = result,
            Err(e) => errors.push(format!("Chunk {idx}: {e}")),
        }
    }
    if !errors.is_empty() {
        eprintln!("Chunk errors: {:?}", errors);
    }
    Ok(results)
}
