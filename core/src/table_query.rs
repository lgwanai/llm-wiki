//! Natural language to SQL for ledger tables via LLM.

use crate::error::WikiResult;
use crate::ledger;
use crate::llm;

pub fn ask_table(table: &str, question: &str) -> WikiResult<String> {
    let info = ledger::show_table(table)?;
    let fields = info.get("fields").and_then(|v| v.as_str()).unwrap_or("[]");

    let system = "You are a DuckDB SQL expert. Generate only a valid SQL SELECT query. No explanations.";
    let user = format!(
        "Table: {table}\nSchema fields (JSON): {fields}\nQuestion: {question}\n\nGenerate a DuckDB SELECT query to answer this question:"
    );

    let sql = llm::call_llm_default(system, &user)?;
    let sql = sql.trim().trim_matches('`').trim();
    // Extract just the SELECT statement
    let sql = if let Some(pos) = sql.to_lowercase().find("select") {
        &sql[pos..]
    } else {
        sql
    };
    let sql = sql.split(';').next().unwrap_or(sql).trim().to_string() + ";";

    Ok(sql)
}

pub fn get_table_schema(table: &str) -> WikiResult<serde_json::Value> {
    ledger::show_table(table)
}

pub fn execute_sql(sql: &str) -> WikiResult<Vec<serde_json::Value>> {
    let db_path = crate::config::get_ledger_db_path();
    let conn = duckdb::Connection::open(&db_path)?;
    let mut stmt = conn.prepare(sql)?;
    let col_names: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let rows = stmt.query_map([], |row| {
        let mut obj = serde_json::Map::new();
        for (i, col) in col_names.iter().enumerate() {
            let val: String = row.get::<_, duckdb::types::Value>(i)
                .map(|v| format!("{v:?}"))
                .unwrap_or_else(|_| "NULL".to_string());
            obj.insert(col.clone(), serde_json::Value::String(val));
        }
        Ok(serde_json::Value::Object(obj))
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}
