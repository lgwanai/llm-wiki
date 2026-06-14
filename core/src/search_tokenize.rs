//! Tokenization and stemming for wiki search.
//! Uses jieba-rs for Chinese, regex for English, simple suffix stemming.

use std::sync::OnceLock;

/// Global Jieba instance — loading dictionary is expensive, do it once.
static JIEBA: OnceLock<jieba_rs::Jieba> = OnceLock::new();

fn get_jieba() -> &'static jieba_rs::Jieba {
    JIEBA.get_or_init(|| jieba_rs::Jieba::new())
}

/// Tokenize text: jieba for Chinese text, word-regex for English.
pub fn tokenize(text: &str) -> Vec<String> {
    let cjk_count = text.chars().filter(|c| {
        let cp = *c as u32;
        (0x4E00..=0x9FFF).contains(&cp) || (0x3400..=0x4DBF).contains(&cp)
    }).count();

    let mut tokens: Vec<String> = Vec::new();

    if cjk_count > 0 {
        let jieba = get_jieba();
        let words = jieba.cut(text, false);
        for w in words {
            let trimmed = w.trim();
            if trimmed.len() > 1 {
                tokens.push(trimmed.to_string());
            }
        }
        // Also extract English tokens if mixed content
        if text.chars().count() as f64 > cjk_count as f64 * 1.5 {
            let re = regex::Regex::new(r"[a-z0-9]+").unwrap();
            for m in re.find_iter(&text.to_lowercase()) {
                tokens.push(m.as_str().to_string());
            }
        }
    } else {
        let re = regex::Regex::new(r"[a-z0-9]+").unwrap();
        for m in re.find_iter(&text.to_lowercase()) {
            tokens.push(m.as_str().to_string());
        }
    }

    tokens
}

/// Simple suffix-stripping stemmer for English. CJK passed through unchanged.
pub fn stem(word: &str) -> String {
    if word.chars().any(|c| {
        let cp = c as u32;
        (0x4E00..=0x9FFF).contains(&cp) || (0x3400..=0x4DBF).contains(&cp)
    }) {
        return word.to_string();
    }

    let w = word.to_lowercase();
    if w.len() > 5 && w.ends_with("ing") { return w[..w.len() - 3].to_string(); }
    if w.len() > 4 && w.ends_with("ed") { return w[..w.len() - 2].to_string(); }
    if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") { return w[..w.len() - 1].to_string(); }
    if w.len() > 5 && w.ends_with("ion") { return w[..w.len() - 3].to_string(); }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stem_ing() { assert_eq!(stem("running"), "runn"); }
    #[test]
    fn test_stem_ed() { assert_eq!(stem("tested"), "test"); }
    #[test]
    fn test_stem_plural() { assert_eq!(stem("tests"), "test"); }
    #[test]
    fn test_tokenize_english() {
        let tokens = tokenize("The quick brown fox");
        assert!(tokens.contains(&"quick".to_string()));
    }
    #[test]
    fn test_tokenize_chinese() {
        let tokens = tokenize("深度学习模型训练");
        assert!(!tokens.is_empty());
    }
}
