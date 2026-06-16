//! Chart generation from query results (mermaid.js and ASCII).

pub fn gen_mermaid_graph(edges: &[(String, String, String)]) -> String {
    let mut out = String::from("```mermaid\ngraph TD\n");
    for (_i, (src, tgt, label)) in edges.iter().enumerate() {
        out.push_str(&format!(
            "    {src}[\"{src}\"] -->|\"{label}\"| {tgt}[\"{tgt}\"]\n"
        ));
    }
    out.push_str("```\n");
    out
}

pub fn gen_ascii_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut col_widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }
    let mut out = String::new();
    // Header
    for (i, h) in headers.iter().enumerate() {
        out.push_str(&format!("| {:<width$} ", h, width = col_widths[i]));
    }
    out.push_str("|\n");
    // Separator
    for w in &col_widths {
        out.push_str(&format!("|{:-<width$}", "", width = w + 1));
    }
    out.push_str("|\n");
    // Rows
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                out.push_str(&format!("| {:<width$} ", cell, width = col_widths[i]));
            }
        }
        out.push_str("|\n");
    }
    out
}
