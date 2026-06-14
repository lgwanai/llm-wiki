//! URL to Markdown conversion via ReaderLM API.

use crate::error::WikiResult;

pub fn url_to_markdown(url: &str) -> WikiResult<String> {
    // Use a simple HTTP fetch with HTML-to-text fallback
    let client = reqwest::blocking::Client::builder()
        .user_agent("llm-wiki/2.0")
        .build()?;
    let resp = client.get(url).send()?;
    let html = resp.text()?;

    // Simple HTML to markdown conversion
    let md = html2md(&html);
    Ok(format!("# Source: {url}\n\n{md}"))
}

fn html2md(html: &str) -> String {
    // Strip scripts and styles
    let re_script = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let re_style = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let mut text = re_script.replace_all(html, "").to_string();
    text = re_style.replace_all(&text, "").to_string();

    // Strip remaining tags
    let re_tag = regex::Regex::new(r"<[^>]+>").unwrap();
    text = re_tag.replace_all(&text, " ").to_string();

    // Decode entities
    text = text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ");

    // Clean up whitespace
    let re_ws = regex::Regex::new(r"\n{3,}").unwrap();
    text = re_ws.replace_all(&text.trim(), "\n\n").to_string();

    text
}
