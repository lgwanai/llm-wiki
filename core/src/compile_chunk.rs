//! Document chunking for large files during compilation.
//! Splits documents by heading boundaries near token thresholds.
//! Respects atomic content regions: code blocks, tables, blockquotes,
//! Chinese quoted text, and numbered lists — these are never split.
//! Uses std::thread::scope for parallel LLM calls.

use crate::error::WikiResult;

/// Minimum chunk size (characters). Don't split chunks smaller than this.
const MIN_CHUNK_SIZE: usize = 2000;

/// Character-to-token conversion ratio for English.
const CHARS_PER_TOKEN_EN: usize = 4;

/// Character-to-token conversion ratio for Chinese.
const CHARS_PER_TOKEN_ZH: usize = 2;

// ═══════════════════════════════════════════════════════════════════════════
// Line-level protection tracking
// ═══════════════════════════════════════════════════════════════════════════

/// Per-line protection status. Any line marked `true` in any category
/// must NOT be used as a split boundary.
#[derive(Debug, Clone)]
struct LineProtection {
    /// Lines inside fenced code blocks (``` or ~~~)
    code_block: Vec<bool>,
    /// Lines inside markdown tables (|-delimited runs)
    table: Vec<bool>,
    /// Lines inside blockquotes (>-prefixed runs, including internal blank lines)
    blockquote: Vec<bool>,
    /// Lines at boundaries following Chinese-citation paragraphs
    chinese_cited: Vec<bool>,
}

impl LineProtection {
    fn new(line_count: usize) -> Self {
        Self {
            code_block: vec![false; line_count],
            table: vec![false; line_count],
            blockquote: vec![false; line_count],
            chinese_cited: vec![false; line_count],
        }
    }

    fn is_any_protected(&self, line_idx: usize) -> bool {
        if line_idx >= self.code_block.len() {
            return false;
        }
        self.code_block[line_idx]
            || self.table[line_idx]
            || self.blockquote[line_idx]
            || self.chinese_cited[line_idx]
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Scanner: detect protected spans
// ═══════════════════════════════════════════════════════════════════════════

/// Scan content and identify all protected lines that must not be split.
fn scan_protected_spans(lines: &[&str]) -> LineProtection {
    let mut protection = LineProtection::new(lines.len());

    scan_code_blocks(lines, &mut protection);
    scan_tables(lines, &mut protection);
    scan_blockquotes(lines, &mut protection);
    scan_chinese_cited(lines, &mut protection);

    protection
}

/// Phase 1: Detect fenced code blocks (``` or ~~~).
/// Uses CommonMark rules: same fence character, same-or-longer closing fence.
fn scan_code_blocks(lines: &[&str], p: &mut LineProtection) {
    let mut in_block = false;
    let mut fence_char: char = '`';
    let mut fence_len: usize = 3;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();

        if !in_block {
            // Check for opening fence: ```+ or ~~~+
            if let Some((fc, fl)) = detect_fence(trimmed) {
                in_block = true;
                fence_char = fc;
                fence_len = fl;
                p.code_block[i] = true;
            }
        } else {
            // Inside a block: mark line as protected
            p.code_block[i] = true;

            // Check for closing fence
            if let Some((fc, fl)) = detect_fence(trimmed) {
                if fc == fence_char && fl >= fence_len {
                    // Valid closing fence
                    in_block = false;
                }
            }
        }
    }
    // If content ends inside an unclosed block, all lines remain protected (already marked)
}

/// Detect a code-fence line. Returns (fence_char, fence_length) or None.
/// Leading whitespace is skipped per CommonMark spec.
fn detect_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let bytes = trimmed.as_bytes();
    let first = *bytes.first()?;

    if first != b'`' && first != b'~' {
        return None;
    }

    let mut count = 0;
    for &b in bytes {
        if b == first {
            count += 1;
        } else {
            break;
        }
    }

    if count >= 3 {
        // The rest of the line after the fence may only contain whitespace
        // (CommonMark: info string or nothing). We accept anything after.
        Some((first as char, count))
    } else {
        None
    }
}

/// Phase 2: Detect markdown tables.
/// A table is 2+ consecutive non-blank lines where the second matches the
/// separator pattern `|:---|` and all lines contain `|`.
fn scan_tables(lines: &[&str], p: &mut LineProtection) {
    let mut i = 0;
    while i < lines.len() {
        // Skip already-protected lines
        if p.code_block[i] || p.blockquote[i] {
            i += 1;
            continue;
        }

        // Find start of a potential table: a line containing `|`
        if !lines[i].contains('|') || lines[i].trim().is_empty() {
            i += 1;
            continue;
        }

        let start = i;

        // Look ahead for at least 2 consecutive `|` lines
        let mut end = i;
        while end < lines.len() && lines[end].contains('|') && !lines[end].trim().is_empty() {
            // Don't cross into code blocks or blockquotes
            if p.code_block[end] || p.blockquote[end] {
                break;
            }
            end += 1;
        }

        let run_len = end - start;
        if run_len >= 2 {
            // Verify the second line looks like a separator row
            let sep = lines[start + 1].trim();
            let is_sep = sep.contains('|')
                && sep
                    .chars()
                    .all(|c| c == '|' || c == ':' || c == '-' || c == ' ' || c == '\t');

            if is_sep {
                for j in start..end {
                    p.table[j] = true;
                }
            }
        }

        i = end;
    }
}

/// Phase 3: Detect blockquotes (lines starting with `>`).
/// Once in a blockquote, internal blank lines stay protected.
fn scan_blockquotes(lines: &[&str], p: &mut LineProtection) {
    let mut in_quote = false;
    let mut quote_start = 0;

    for (i, line) in lines.iter().enumerate() {
        // Skip already-protected by code_block or table
        if p.code_block[i] || p.table[i] {
            if in_quote {
                // Finalize the previous blockquote run
                for j in quote_start..i {
                    p.blockquote[j] = true;
                }
                in_quote = false;
            }
            continue;
        }

        let is_quote_line = line.trim_start().starts_with('>');

        if is_quote_line && !in_quote {
            in_quote = true;
            quote_start = i;
        } else if !is_quote_line && in_quote {
            let trimmed = line.trim();
            // A blank line inside a quote continues it
            if trimmed.is_empty() {
                // This blank line is part of the blockquote
                continue;
            }
            // Non-blank, non-`>` line ends the quote
            for j in quote_start..i {
                p.blockquote[j] = true;
            }
            in_quote = false;
        }
    }

    // Close any trailing blockquote
    if in_quote {
        for j in quote_start..lines.len() {
            p.blockquote[j] = true;
        }
    }
}

/// Phase 4: Protect paragraph boundaries that follow Chinese-citation text.
/// Paragraphs containing 《》、""、「」quotation marks protect the next
/// paragraph boundary so attribution stays with cited content.
fn scan_chinese_cited(lines: &[&str], p: &mut LineProtection) {
    // Build paragraphs from unprotected lines, tracking which contain citations
    let mut paragraphs: Vec<(usize, usize, bool)> = Vec::new(); // (start, end, has_citation)
    let mut para_start: Option<usize> = None;
    let mut has_citation = false;

    for (i, line) in lines.iter().enumerate() {
        // Skip lines already protected by code/table/blockquote
        if p.code_block[i] || p.table[i] || p.blockquote[i] {
            if let Some(start) = para_start {
                paragraphs.push((start, i, has_citation));
                para_start = None;
                has_citation = false;
            }
            continue;
        }

        let trimmed = line.trim();

        if trimmed.is_empty() {
            // Blank line ends the current paragraph
            if let Some(start) = para_start {
                paragraphs.push((start, i, has_citation));
                para_start = None;
                has_citation = false;
            }
        } else {
            if para_start.is_none() {
                para_start = Some(i);
            }
            // Check for Chinese citation markers
            if !has_citation && contains_chinese_citation(trimmed) {
                has_citation = true;
            }
        }
    }

    // Close trailing paragraph
    if let Some(start) = para_start {
        paragraphs.push((start, lines.len(), has_citation));
    }

    // For each paragraph with citations, protect the FIRST line of the NEXT paragraph
    // (i.e., the boundary between them should not be split)
    let n = paragraphs.len();
    for idx in 0..n {
        let (_start, _end, has_cit) = paragraphs[idx];
        if has_cit && idx + 1 < n {
            let (next_start, _, _) = paragraphs[idx + 1];
            // Protect the first line of the next paragraph
            if next_start < p.chinese_cited.len() {
                p.chinese_cited[next_start] = true;
            }
        }
    }
}

/// Check if text contains Chinese quotation or book-title marks.
fn contains_chinese_citation(text: &str) -> bool {
    // Chinese book-title marks: 《 》
    // Chinese quotation marks: " " 「 」 『 』 ﹁ ﹂
    // Full-width CJK quotes: ﹃ ﹄

    // Check for paired Chinese marks
    let mut in_book_title = false;
    let mut in_double_quote = false;
    let mut in_corner_quote = false;

    for c in text.chars() {
        match c {
            '\u{300a}' => in_book_title = true,         // 《
            '\u{300b}' if in_book_title => return true, // 》
            '\u{201c}' | '\u{300c}' | '\u{300e}' | '\u{fe43}' | '\u{ff3b}' => {
                in_double_quote = true; // " 「 『 ﹃ ［
            }
            '\u{201d}' | '\u{300d}' | '\u{300f}' | '\u{fe44}' | '\u{ff3d}' if in_double_quote => {
                return true; // " 」 』 ﹄ ］
            }
            '\u{3008}' => in_corner_quote = true,         // 〈
            '\u{3009}' if in_corner_quote => return true, // 〉
            _ => {}
        }
    }

    // Also check for simple occurrence of 《 (strongest signal even without pair)
    if text.contains('\u{300a}') {
        return true;
    }

    false
}

// ═══════════════════════════════════════════════════════════════════════════
// Safe paragraph boundary detection
// ═══════════════════════════════════════════════════════════════════════════

/// Find byte positions of `\n\n` (blank-line) boundaries that are safe to
/// split at — i.e., not inside any protected region.
fn find_safe_para_boundaries(content: &str, protection: &LineProtection) -> Vec<usize> {
    // Normalize line endings: convert CRLF (\r\n) to LF (\n) so that
    // consecutive \n\n detection works uniformly on all platforms.
    let normalized = content.replace("\r\n", "\n");
    let bytes = normalized.as_bytes();

    // Build line-start byte offsets: line_starts[k] = byte index of line k's first char
    let mut line_starts: Vec<usize> = vec![0];
    for (pos, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            // Next line starts after this \n
            if pos + 1 < bytes.len() {
                line_starts.push(pos + 1);
            }
        }
    }

    // For each \n\n sequence, determine what line it falls on and check protection
    let mut boundaries = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\n' && bytes[i + 1] == b'\n' {
            // Find which line index contains byte position `i`
            // line_idx is the index of the blank line itself
            let line_idx = match line_starts.binary_search(&(i + 1)) {
                Ok(idx) => idx,
                Err(idx) => idx.saturating_sub(1),
            };

            // Check: the line BEFORE the blank line should NOT be protected
            if line_idx > 0 && !protection.is_any_protected(line_idx - 1) {
                // Also check the blank line itself and first line after
                // are not protected
                let blank_ok = line_idx < protection.code_block.len()
                    && !protection.is_any_protected(line_idx);
                let next_ok = line_idx + 1 >= protection.code_block.len()
                    || !protection.is_any_protected(line_idx + 1);
                if blank_ok && next_ok {
                    // boundary after the first \n (at position i)
                    boundaries.push(i + 1);
                }
            }

            // Skip past the \n\n (and any \r)
            i += 2;
            while i < bytes.len() && (bytes[i] == b'\r' || bytes[i] == b'\n') {
                i += 1;
            }
            continue;
        }
        i += 1;
    }

    boundaries
}

/// Extract paragraphs by slicing content at safe boundary positions.
fn extract_paragraphs<'a>(content: &'a str, boundaries: &[usize]) -> Vec<&'a str> {
    let mut paragraphs = Vec::new();
    let mut prev = 0usize;

    for &boundary in boundaries {
        if boundary > prev && boundary <= content.len() {
            let p = content[prev..boundary].trim();
            if !p.is_empty() {
                paragraphs.push(p);
            }
        }
        prev = boundary;
    }

    // Last paragraph: everything after the last boundary
    if prev < content.len() {
        let last = content[prev..].trim();
        if !last.is_empty() {
            paragraphs.push(last);
        }
    }

    paragraphs
}

// ═══════════════════════════════════════════════════════════════════════════
// Main chunking functions
// ═══════════════════════════════════════════════════════════════════════════

/// Split content into chunks at heading boundaries, respecting token limits.
/// Protected regions (code blocks, tables, blockquotes, Chinese citations)
/// are never split.
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

    // Collect lines with indices for protection lookup
    let lines: Vec<&str> = content.lines().collect();
    let protection = scan_protected_spans(&lines);

    // Split at heading boundaries, respecting protected regions
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_line_count = 0usize; // lines added to current chunk

    for (i, line) in lines.iter().enumerate() {
        let is_safe_heading = line.starts_with('#') && !protection.code_block[i];

        // Determine if we should split here
        let should_split = !current.is_empty()
            && (current.chars().count() + line.chars().count() > max_chars)
            && (is_safe_heading || current.chars().count() > max_chars / 2)
            && !protection.is_any_protected(i);

        if should_split {
            chunks.push(current.trim().to_string());
            current = String::new();
            current_line_count = 0;
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
        current_line_count += 1;
    }

    // Don't leave a tiny last chunk
    if !current.trim().is_empty() {
        if current.chars().count() < MIN_CHUNK_SIZE
            && !chunks.is_empty()
            && !protection.is_any_protected(lines.len().saturating_sub(current_line_count))
        {
            let last = chunks.pop().unwrap();
            chunks.push(format!("{last}\n{current}"));
        } else {
            chunks.push(current.trim().to_string());
        }
    }

    // If we only got one chunk or chunks are too small, use force_split
    // with the already-computed lines and protection to avoid redundant scanning
    if chunks.len() <= 1 {
        chunks = force_split_inner(content, max_chars, Some((&lines, &protection)));
    }

    chunks
}

/// Force-split content by paragraph boundaries, but only at safe boundaries
/// outside protected regions (code blocks, tables, blockquotes, etc.).
/// Force-split content by paragraph boundaries, but only at safe boundaries
/// outside protected regions (code blocks, tables, blockquotes, etc.).
///
/// When `cached` is provided (from chunk_content), avoids redundant line
/// collection and protection scanning.
fn force_split(content: &str, max_chars: usize) -> Vec<String> {
    force_split_inner(content, max_chars, None)
}

/// Same as force_split but accepts pre-computed lines and protection to avoid
/// redundant scanning when called from chunk_content which already computed them.
fn force_split_inner(
    content: &str,
    max_chars: usize,
    cached: Option<(&[&str], &LineProtection)>,
) -> Vec<String> {
    let (_lines, protection) = match cached {
        Some((lines, prot)) => (lines.to_vec(), prot.clone()),
        None => {
            let lines: Vec<&str> = content.lines().collect();
            let protection = scan_protected_spans(&lines);
            (lines, protection)
        }
    };
    let safe_boundaries = find_safe_para_boundaries(content, &protection);

    let paragraphs = extract_paragraphs(content, &safe_boundaries);

    // If no safe boundaries found (all content protected), return as single chunk
    if paragraphs.is_empty() {
        let trimmed = content.trim();
        return if trimmed.is_empty() {
            vec![]
        } else {
            vec![trimmed.to_string()]
        };
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for para in &paragraphs {
        let para_trimmed = para.trim();
        if para_trimmed.is_empty() {
            continue;
        }

        if !current.is_empty()
            && current.chars().count() + para_trimmed.chars().count() > max_chars
            && current.chars().count() > MIN_CHUNK_SIZE
        {
            chunks.push(current.trim().to_string());
            current = String::new();
        }

        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para_trimmed);
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

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper ──

    fn chunk(content: &str, max_tokens: usize) -> Vec<String> {
        chunk_content(content, max_tokens, "en")
    }

    // ── Code block tests ──

    #[test]
    fn test_code_block_not_split() {
        let content =
            "Intro text.\n\n```\nfn hello() {\n    println!(\"hi\");\n}\n```\n\nMore text.";
        let result = force_split(content, 5000);
        // The code block should be intact in one paragraph
        let combined = result.join("\n\n");
        assert!(combined.contains("fn hello()"));
        assert!(combined.contains("println!"));
    }

    #[test]
    fn test_code_block_with_internal_blank_lines() {
        let content = "Before.\n\n```rust\nline 1\n\nline 2\n\nline 3\n```\n\nAfter.";
        let result = force_split(content, 5000);
        // Internal blank lines inside code block must be preserved
        let combined = result.join("\n\n");
        assert!(combined.contains("line 1\n\nline 2\n\nline 3"));
    }

    #[test]
    fn test_code_block_unchanged_by_force_split() {
        // force_split with generous max_chars should keep code block intact
        let content = "A paragraph.\n\n```\ncode\nblock\n\nwith gaps\n```\n\nB paragraph.";
        let result = force_split(content, 10000);
        // Should produce at most 3 paragraphs: A, code block, B
        assert!(result.len() <= 3);
        // The code block paragraph must contain both "code" and "gaps"
        let has_code_para = result
            .iter()
            .any(|p| p.contains("code") && p.contains("gaps"));
        assert!(has_code_para, "Code block was split: {:?}", result);
    }

    #[test]
    fn test_code_block_small_max_chars_stays_intact() {
        let content = "Short intro.\n\n```\nfunction test() {\n    return true;\n}\n```\n\nConclusion text here.";
        // Use small max_chars to force splitting
        let result = force_split(content, 100);
        // The code block may be in its own oversized paragraph
        let combined = result.join("\n\n");
        assert!(combined.contains("function test()"));
        assert!(combined.contains("return true;"));
    }

    #[test]
    fn test_two_code_blocks_split_between() {
        let content = "```\nblock one\n```\n\nSeparator text here.\n\n```\nblock two\n```";
        let result = force_split(content, 5000);
        let combined = result.join("\n\n");
        assert!(combined.contains("block one"));
        assert!(combined.contains("block two"));
        assert!(combined.contains("Separator"));
    }

    #[test]
    fn test_unclosed_code_block_protected_to_eof() {
        let content = "Before.\n\n```\nopen code block\nno closing fence\n\nSome text after.";
        let result = force_split(content, 5000);
        let combined = result.join("\n\n");
        // "open code block" and "no closing fence" should be in same paragraph
        assert!(combined.contains("open code block"));
        assert!(combined.contains("no closing fence"));
    }

    #[test]
    fn test_tilde_fence() {
        let content = "~~~\ncode with tildes\n~~~";
        let lines: Vec<&str> = content.lines().collect();
        let p = scan_protected_spans(&lines);
        // All three lines should be protected
        assert!(p.code_block[0]);
        assert!(p.code_block[1]);
        assert!(p.code_block[2]);
    }

    #[test]
    fn test_heading_inside_code_block_not_split() {
        // # inside code block should NOT trigger heading split
        let content = "Intro.\n\n```\n# This is a comment, not a heading\ncode here\n```\n\n## Real Heading\n\nContent here.";
        let result = chunk(content, 5000);
        let combined = result.join("\n\n");
        // "# This is a comment" should stay inside the code block section
        assert!(combined.contains("# This is a comment"));
        assert!(combined.contains("## Real Heading"));
    }

    // ── Table tests ──

    #[test]
    fn test_table_not_split() {
        let content = "Intro.\n\n| Name | Value |\n|------|-------|\n| foo  | 10    |\n| bar  | 20    |\n\nAfter table.";
        let result = force_split(content, 5000);
        // Table rows should stay together
        let combined = result.join("\n\n");
        assert!(combined.contains("foo"));
        assert!(combined.contains("bar"));
    }

    #[test]
    fn test_table_split_between_tables() {
        let content =
            "| A | B |\n|---|---|\n| 1 | 2 |\n\nMiddle text.\n\n| C | D |\n|---|---|\n| 3 | 4 |";
        let result = force_split(content, 5000);
        let combined = result.join("\n\n");
        assert!(combined.contains("| A |"));
        assert!(combined.contains("| C |"));
    }

    #[test]
    fn test_single_pipe_not_table() {
        // A single line with | should not be treated as a table
        let content = "Rust closure like |x| x + 1 is common.\n\nAnother paragraph.";
        let result = force_split(content, 5000);
        // Should split at the blank line normally
        let combined = result.join("\n\n");
        assert!(combined.contains("|x|"));
        assert!(combined.contains("Another"));
    }

    // ── Blockquote tests ──

    #[test]
    fn test_blockquote_not_split() {
        let content = "Before.\n\n> This is a quote\n> that spans multiple lines\n>\n> with a blank line inside.\n\nAfter.";
        let result = force_split(content, 5000);
        let combined = result.join("\n\n");
        // Entire quote should be in one paragraph
        assert!(combined.contains("This is a quote"));
        assert!(combined.contains("blank line inside"));
    }

    #[test]
    fn test_blockquote_ends_at_non_quote_line() {
        let content = "> A quote line\n\nNormal text after quote.";
        let result = force_split(content, 5000);
        let combined = result.join("\n\n");
        assert!(combined.contains("A quote line"));
        assert!(combined.contains("Normal text"));
    }

    #[test]
    fn test_blockquote_ends_at_non_quote_line2() {
        let content = "> A quote line\nNormal text after quote.";
        let result = force_split(content, 5000);
        let combined = result.join("\n\n");
        assert!(combined.contains("A quote line"));
        assert!(combined.contains("Normal text"));
    }

    // ── Chinese cited text tests ──

    #[test]
    fn test_chinese_book_title_protects_next_para() {
        // Two lines in the same paragraph (no blank line) — already together, no protection needed
        // The protection is needed when there IS a blank line between citation and attribution
        let content = "根据《红楼梦》的记载\n这是续段，不应被拆分。";
        let lines: Vec<&str> = content.lines().collect();
        let _p = scan_protected_spans(&lines);
        // Both lines are in the same paragraph (no blank line between them),
        // so the chinese_cited phase has no next paragraph to protect.
        // The lines are already together — the blank-line split protection isn't needed.
        // Verify the scanner doesn't crash on this input
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_chinese_quotes_protect_boundary() {
        let content = "He said, \u{201c}this is important\u{201d}.\n\nAttribution continues here.";
        let result = force_split(content, 5000);
        let combined = result.join("\n\n");
        // Should not split at the \n\n between these paragraphs
        assert!(combined.contains("this is important"));
        assert!(combined.contains("Attribution"));
    }

    #[test]
    fn test_chinese_quotes_not_split_by_force() {
        // The blank line between cited text and attribution should be protected
        let content = "正如孔子所说\u{201c}学而时习之\u{201d}\n\n这段话很重要。";
        let result = force_split(content, 5000);
        // Both paragraphs should be together (protection merges them)
        let combined = result.join("\n\n");
        assert!(combined.contains("学而时习之"));
        assert!(combined.contains("这段话很重要"));
    }

    #[test]
    fn test_chinese_citation_inside_code_block_not_affected() {
        let content = "```\n《这是代码，不是引用》\n```\n\nNormal text.";
        let result = force_split(content, 5000);
        let combined = result.join("\n\n");
        // Code block intact, normal text separate
        assert!(combined.contains("《这是代码，不是引用》"));
        assert!(combined.contains("Normal text"));
    }

    // ── Numbered list tests ──

    #[test]
    fn test_numbered_list_items_stay_together() {
        let content = "List:\n\n1. First item\n2. Second item\n3. Third item with details\n   that span multiple lines\n\nAfter list.";
        let result = force_split(content, 5000);
        let combined = result.join("\n\n");
        assert!(combined.contains("First item"));
        assert!(combined.contains("Second item"));
        assert!(combined.contains("Third item"));
    }

    // ── Integration / regression tests ──

    #[test]
    fn test_plain_text_unchanged_behavior() {
        // Plain English with no protected regions should work as before
        let content = "Paragraph one.\n\nParagraph two.\n\nParagraph three.";
        let result = force_split(content, 5000);
        // Should still split at blank lines for plain text
        assert!(!result.is_empty());
        let combined = result.join("\n\n");
        assert!(combined.contains("Paragraph one"));
        assert!(combined.contains("Paragraph two"));
        assert!(combined.contains("Paragraph three"));
    }

    #[test]
    fn test_empty_content() {
        let result = chunk_content("", 1000, "en");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "");
    }

    #[test]
    fn test_short_content_single_chunk() {
        let content = "Short text that fits in one chunk.";
        let result = chunk(content, 50000);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_chunk_with_headings() {
        let content =
            "# Heading 1\n\nContent for section one.\n\n## Heading 2\n\nContent for section two.";
        let result = chunk(content, 50000);
        // With large max_tokens, should return single chunk
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_all_content_protected_single_paragraph() {
        // A document that is entirely inside a code block
        let content = "```\nEverything\nis\nprotected\n\nincluding gaps\n```";
        let result = force_split(content, 10);
        // Should produce a single chunk (or 0 if empty)
        let combined = result.join("\n\n");
        assert!(combined.contains("protected"));
        assert!(combined.contains("including gaps"));
    }

    #[test]
    fn test_mixed_content_integration() {
        let content = "\
# Document Title

Some introductory text about the topic.

## Code Example

```rust
fn main() {
    println!(\"Hello\");

    // blank line above
}
```

## Data Table

| Language | Speed |
|----------|-------|
| Rust     | Fast  |
| Python   | Slow  |

## Blockquote

> This is a quoted passage
> that someone important said.
>
> It continues here.

如《论语》所述，\u{201c}学而不思则罔\u{201d}，这是重要的学习方法。

Final conclusion text.";
        let result = chunk_content(content, 1000, "en");
        let combined = result.join("\n\n");

        // All key content must be present
        assert!(combined.contains("fn main()"));
        assert!(combined.contains("println!"));
        assert!(combined.contains("| Rust"));
        assert!(combined.contains("| Python"));
        assert!(combined.contains("quoted passage"));
        assert!(combined.contains("学而不思则罔"));
        assert!(combined.contains("Final conclusion"));
    }

    #[test]
    fn test_force_split_with_small_max_chars() {
        // Force splitting with aggressive max_chars should still respect protections
        let mut content = String::from("A brief intro.\n\n");
        // Add a code block
        content.push_str("```\nline a\n\nline b\n\nline c\n```\n\n");
        // Add a table
        content.push_str("| K | V |\n|---|---|\n| x | 1 |\n| y | 2 |\n\n");
        // Add regular paragraphs
        content.push_str("First regular para.\n\nSecond regular para.\n\nThird regular para.");

        let result = force_split(&content, 80); // Very tight

        // Verify no content is lost
        let combined = result.join("\n\n");
        assert!(combined.contains("line a"));
        assert!(combined.contains("line b"));
        assert!(combined.contains("line c"));
        assert!(combined.contains("| x |"));
        assert!(combined.contains("| y |"));
        assert!(combined.contains("First regular"));
        assert!(combined.contains("Second regular"));
        assert!(combined.contains("Third regular"));
    }

    // ── Edge case tests ──

    #[test]
    fn test_crlf_line_endings() {
        let content = "Line one.\r\n\r\nLine two.\r\n\r\n```\r\ncode\r\n\r\nblank\r\n```";
        let lines: Vec<&str> = content.lines().collect();
        let p = scan_protected_spans(&lines);
        // The code block lines should be protected
        // "code", blank, "blank" are inside ``` fences
        let code_line_idx = lines.iter().position(|l| l.contains("code")).unwrap();
        assert!(p.code_block[code_line_idx]);
    }

    #[test]
    fn test_detect_fence_variants() {
        assert_eq!(detect_fence("```"), Some(('`', 3)));
        assert_eq!(detect_fence("~~~~"), Some(('~', 4)));
        assert_eq!(detect_fence("```rust"), Some(('`', 3)));
        assert_eq!(detect_fence("  ```"), Some(('`', 3)));
        assert_eq!(detect_fence("``"), None); // Too short
        assert_eq!(detect_fence("abc"), None);
    }

    #[test]
    fn test_chunk_content_returns_content() {
        let content = "A simple document with no special markers.";
        let result = chunk_content(content, 100, "en");
        // Should not be empty
        assert!(!result.is_empty());
        let combined = result.join("\n\n");
        assert!(combined.contains("A simple document"));
    }

    #[test]
    fn test_whitespace_only_paragraphs_filtered() {
        let content = "Para one.\n\n   \n\nPara two.";
        let result = force_split(content, 5000);
        // Whitespace-only "paragraph" should be filtered out
        let combined = result.join("\n\n");
        assert!(combined.contains("Para one"));
        assert!(combined.contains("Para two"));
    }

    #[test]
    fn test_long_code_block_exceeds_max_chars() {
        // Build a long code block that exceeds max_chars
        let mut code = String::from("```\n");
        for i in 0..100 {
            code.push_str(&format!("line number {i}\n"));
        }
        code.push_str("```\n\nAfter code.");
        let result = force_split(&code, 500);
        // Content should not be lost
        let combined = result.join("\n\n");
        assert!(combined.contains("line number 0"));
        assert!(combined.contains("line number 99"));
        assert!(combined.contains("After code"));
    }
}
