//! Ledger/台账 management — DuckDB-backed structured tables.
//! CRUD: create, insert, show, update-schema, delete, stats, import/export.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use crate::config::get_ledger_db_path;
use crate::error::{WikiError, WikiResult};

const VALID_TYPES: &[&str] = &[
    "string", "text", "integer", "number", "boolean", "date", "datetime",
];

const TYPE_MAP: &[(&str, &str)] = &[
    ("string", "VARCHAR"),
    ("text", "VARCHAR"),
    ("integer", "INTEGER"),
    ("number", "DOUBLE"),
    ("boolean", "BOOLEAN"),
    ("date", "DATE"),
    ("datetime", "TIMESTAMP"),
];

fn duck_type(t: &str) -> &str {
    TYPE_MAP
        .iter()
        .find(|(k, _)| *k == t)
        .map(|(_, v)| *v)
        .unwrap_or("VARCHAR")
}

pub fn slugify(name: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    let hash = format!("{:x}", hasher.finish());
    let slug = name
        .to_lowercase()
        .replace([' ', '_'], "-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if !slug.is_empty() {
        if name.chars().any(|c| !c.is_ascii()) {
            return format!("{slug}-{}", &hash[..12]);
        }
        return slug;
    }
    format!("table-{hash}")
}

fn get_conn(read_only: bool) -> WikiResult<duckdb::Connection> {
    let db_path = get_ledger_db_path();
    if let Some(p) = db_path.parent() {
        fs::create_dir_all(p)?;
    }
    if read_only && !db_path.exists() {
        return Err(WikiError::NotFound("No ledger database found.".into()));
    }
    let conn = duckdb::Connection::open(&db_path)?;
    if !read_only {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS _registry (
            actual_name VARCHAR PRIMARY KEY, display_name VARCHAR NOT NULL,
            description VARCHAR DEFAULT '', record_count INTEGER DEFAULT 0,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            fields_json VARCHAR DEFAULT '[]', unique_key VARCHAR DEFAULT '[]',
            auto_increment BOOLEAN DEFAULT FALSE, auto_increment_field VARCHAR DEFAULT NULL
        )",
            [],
        )?;
    }
    Ok(conn)
}

pub fn list_tables() -> WikiResult<Vec<serde_json::Value>> {
    let conn = get_conn(true)?;
    let mut stmt = conn.prepare("SELECT actual_name, display_name, description, record_count FROM _registry ORDER BY display_name")?;
    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "table": row.get::<_, String>(0)?,
            "display_name": row.get::<_, String>(1)?,
            "description": row.get::<_, String>(2)?,
            "record_count": row.get::<_, i64>(3)?,
        }))
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn create_table(
    display_name: &str,
    fields_json: &str,
    unique: Option<&str>,
    auto_increment: bool,
    table_name: Option<&str>,
    description: &str,
) -> WikiResult<String> {
    let fields: Vec<serde_json::Value> = serde_json::from_str(fields_json)
        .map_err(|e| WikiError::Validation(format!("Invalid fields JSON: {e}")))?;

    let safe_name = table_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| slugify(display_name));
    if safe_name.is_empty() {
        return Err(WikiError::Validation(
            "Table name is empty after slugify".into(),
        ));
    }

    let conn = get_conn(false)?;

    // Check if exists
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM _registry WHERE actual_name = ?",
        [&safe_name],
        |r| r.get(0),
    )?;
    if count > 0 {
        return Err(WikiError::Validation(format!(
            "Table '{safe_name}' already exists"
        )));
    }

    // Build column DDL
    let mut col_defs = Vec::new();
    let mut field_descs = Vec::new();
    if auto_increment {
        col_defs.push("\"_id\" INTEGER PRIMARY KEY".to_string());
    }
    for f in &fields {
        let name = f["name"].as_str().unwrap_or("field");
        let ftype = f["type"].as_str().unwrap_or("string");
        if !VALID_TYPES.contains(&ftype) {
            return Err(WikiError::Validation(format!(
                "Invalid field type: {ftype}"
            )));
        }
        col_defs.push(format!("\"{}\" {}", name, duck_type(ftype)));
        field_descs.push(f.clone());
    }

    let ddl = format!(
        "CREATE TABLE IF NOT EXISTS \"{}\" ({})",
        safe_name,
        col_defs.join(", ")
    );
    conn.execute(&ddl, [])?;

    // Register
    conn.execute(
        "INSERT INTO _registry (actual_name, display_name, description, fields_json, unique_key, auto_increment) VALUES (?, ?, ?, ?, ?, ?)",
        duckdb::params![safe_name, display_name, description, serde_json::to_string(&field_descs)?, unique.unwrap_or(""), auto_increment],
    )?;

    Ok(safe_name)
}

pub fn insert_data(table: &str, data_json: &str, batch: bool) -> WikiResult<usize> {
    let conn = get_conn(false)?;
    let data: serde_json::Value = serde_json::from_str(data_json)
        .map_err(|e| WikiError::Validation(format!("Invalid data JSON: {e}")))?;

    let rows: Vec<serde_json::Value> = if data.is_array() {
        data.as_array().unwrap().clone()
    } else {
        vec![data]
    };

    let mut inserted = 0;
    for row in &rows {
        let obj = row
            .as_object()
            .ok_or_else(|| WikiError::Validation("Row must be an object".into()))?;
        let cols: Vec<&str> = obj.keys().map(|s| s.as_str()).collect();
        let vals: Vec<String> = obj
            .values()
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            })
            .collect();
        let placeholders: Vec<&str> = vec!["?"; cols.len()];
        let sql = format!(
            "INSERT INTO \"{}\" ({}) VALUES ({})",
            table,
            cols.iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", "),
            placeholders.join(", ")
        );
        match conn.execute(&sql, duckdb::params_from_iter(vals.iter())) {
            Ok(_) => inserted += 1,
            Err(e) if batch => eprintln!("Row error: {e}"),
            Err(e) => return Err(e.into()),
        }
    }

    conn.execute("UPDATE _registry SET record_count = record_count + ?, updated_at = CURRENT_TIMESTAMP WHERE actual_name = ?", duckdb::params![inserted as i64, table])?;
    Ok(inserted)
}

pub fn show_table(table: &str) -> WikiResult<serde_json::Value> {
    let conn = get_conn(true)?;
    let info: Result<_, _> = conn.query_row(
        "SELECT display_name, description, fields_json, record_count FROM _registry WHERE actual_name = ?",
        [table],
        |row| Ok(serde_json::json!({
            "table": table,
            "display_name": row.get::<_, String>(0)?,
            "description": row.get::<_, String>(1)?,
            "fields": row.get::<_, String>(2)?,
            "record_count": row.get::<_, i64>(3)?,
        })),
    );
    match info {
        Ok(v) => Ok(v),
        Err(duckdb::Error::QueryReturnedNoRows) => {
            Err(WikiError::NotFound(format!("Table '{table}' not found")))
        }
        Err(e) => Err(e.into()),
    }
}

pub fn delete_table(table: &str) -> WikiResult<()> {
    let conn = get_conn(false)?;
    conn.execute(&format!("DROP TABLE IF EXISTS \"{table}\""), [])?;
    conn.execute("DELETE FROM _registry WHERE actual_name = ?", [table])?;
    Ok(())
}

pub fn table_stats(table: Option<&str>) -> WikiResult<serde_json::Value> {
    let conn = get_conn(true)?;
    if let Some(t) = table {
        let count: i64 =
            conn.query_row(&format!("SELECT COUNT(*) FROM \"{t}\""), [], |r| r.get(0))?;
        Ok(serde_json::json!({"table": t, "row_count": count}))
    } else {
        let mut stmt = conn.prepare("SELECT actual_name, record_count FROM _registry")?;
        let rows: Vec<serde_json::Value> = stmt.query_map([], |row| {
            Ok(serde_json::json!({"table": row.get::<_, String>(0)?, "row_count": row.get::<_, i64>(1)?}))
        })?.filter_map(|r| r.ok()).collect();
        Ok(serde_json::json!(rows))
    }
}

pub fn export_csv(table: &str, output: Option<&str>) -> WikiResult<String> {
    let conn = get_conn(true)?;
    // Get columns
    let stmt = conn.prepare(&format!("SELECT * FROM \"{table}\" LIMIT 0"))?;
    let cols: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();

    let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\""))?;
    let mut csv_out = cols.join(",") + "\n";
    let rows = stmt.query_map([], |row| {
        let vals: Vec<String> = (0..row.as_ref().column_count())
            .map(|i| {
                let raw = row
                    .get::<_, duckdb::types::Value>(i)
                    .map(|v| {
                        // Strip Debug variant prefix for cleaner CSV output
                        let dbg = format!("{:?}", v);
                        csv_strip_debug_prefix(&dbg)
                    })
                    .unwrap_or_default();
                // CSV-escape: wrap in quotes if the value contains comma, quote, or newline
                if raw.contains(',') || raw.contains('"') || raw.contains('\n') {
                    format!("\"{}\"", raw.replace('"', "\"\""))
                } else {
                    raw
                }
            })
            .collect();
        Ok(vals.join(","))
    })?;
    for row in rows {
        csv_out.push_str(&row?);
        csv_out.push('\n');
    }

    if let Some(path) = output {
        fs::write(path, &csv_out)?;
    }
    Ok(csv_out)
}

/// Strip the Debug variant prefix from duckdb Value Debug output.
/// e.g. `Text("hello")` → `hello`, `Int(42)` → `42`, `Null` → ``
fn csv_strip_debug_prefix(dbg: &str) -> String {
    if dbg == "Null" {
        return String::new();
    }
    if let Some(paren) = dbg.find('(') {
        let inner = &dbg[paren + 1..];
        // Strip trailing `)`
        let inner = inner.strip_suffix(')').unwrap_or(inner);
        // Strip surrounding quotes from Text/Blob variants
        if (inner.starts_with('"') && inner.ends_with('"'))
            || (inner.starts_with('\'') && inner.ends_with('\''))
        {
            inner[1..inner.len() - 1].to_string()
        } else {
            inner.to_string()
        }
    } else {
        dbg.to_string()
    }
}

/// Import a JSON array file as a ledger table. Returns the table name.
pub fn import_json(file: &str, name: Option<&str>) -> WikiResult<String> {
    let path = PathBuf::from(file);
    if !path.exists() {
        return Err(WikiError::NotFound(format!("File not found: {file}")));
    }
    let content = fs::read_to_string(&path)?;
    let data: Vec<serde_json::Value> = serde_json::from_str(&content)
        .map_err(|e| WikiError::Parse(format!("Invalid JSON: {e}")))?;
    if data.is_empty() {
        return Err(WikiError::Parse("JSON array is empty".into()));
    }

    let table_name = name
        .map(|n| slugify(n))
        .unwrap_or_else(|| slugify(&path.file_stem().unwrap_or_default().to_string_lossy()));
    let _ = delete_table(&table_name);

    // Infer column types from first row
    let first = &data[0];
    let mut fields = Vec::new();
    if let Some(obj) = first.as_object() {
        for (k, v) in obj {
            let ftype = infer_json_type(v);
            fields.push(serde_json::json!({"name": k, "type": ftype}));
        }
    }

    let fields_json = serde_json::to_string(&fields)?;
    let safe_name = create_table(
        name.unwrap_or(&table_name),
        &fields_json,
        None,
        false,
        Some(&table_name),
        "",
    )?;

    // Insert rows
    for row in &data {
        let json_str = serde_json::to_string(row)?;
        insert_data(&safe_name, &json_str, true)?;
    }

    Ok(format!("{safe_name} ({} rows)", data.len()))
}

/// Import Excel file by converting to JSON via Python
pub fn import_excel(file: &str, name: Option<&str>) -> WikiResult<String> {
    let path = PathBuf::from(file);
    if !path.exists() {
        return Err(WikiError::NotFound(format!("File not found: {file}")));
    }

    // Use Python to convert xlsx to JSON. openpyxl is preferred for normal files; the XML
    // fallback ignores styles, which keeps data import working when a workbook has invalid style
    // metadata that openpyxl refuses to load.
    let path_json = serde_json::to_string(&path.to_string_lossy().to_string())?;
    let python_script = format!(
        r#"import json, re, zipfile, xml.etree.ElementTree as ET
path = {path_json}

def clean_header(value, index):
    if value is None or str(value).strip() == "":
        return f"col_{{index + 1}}"
    return str(value).strip()

def normalize_rows(raw_rows):
    raw_rows = [row for row in raw_rows if any(v is not None and str(v).strip() for v in row)]
    if len(raw_rows) < 2:
        return []
    headers = [clean_header(v, i) for i, v in enumerate(raw_rows[0])]
    rows = []
    for raw in raw_rows[1:]:
        obj = {{}}
        for i, header in enumerate(headers):
            value = raw[i] if i < len(raw) else None
            obj[header] = str(value) if value is not None else None
        if any(v is not None and str(v).strip() for v in obj.values()):
            rows.append(obj)
    return rows

def read_with_openpyxl():
    import openpyxl
    wb = openpyxl.load_workbook(path, data_only=True)
    ws = wb.active
    raw_rows = []
    for row in ws.iter_rows(values_only=True):
        raw_rows.append(list(row))
    return normalize_rows(raw_rows)

def col_index(cell_ref):
    letters = re.sub(r"[^A-Z]", "", cell_ref.upper())
    n = 0
    for ch in letters:
        n = n * 26 + ord(ch) - ord("A") + 1
    return max(n - 1, 0)

def xml_text(node):
    return "".join(node.itertext()) if node is not None else None

def read_with_xlsx_xml():
    with zipfile.ZipFile(path) as zf:
        shared = []
        if "xl/sharedStrings.xml" in zf.namelist():
            root = ET.fromstring(zf.read("xl/sharedStrings.xml"))
            for si in root.findall(".//{{*}}si"):
                shared.append(xml_text(si) or "")

        sheet_path = "xl/worksheets/sheet1.xml"
        if "xl/workbook.xml" in zf.namelist() and "xl/_rels/workbook.xml.rels" in zf.namelist():
            wb = ET.fromstring(zf.read("xl/workbook.xml"))
            rel_id = None
            first_sheet = wb.find(".//{{*}}sheet")
            if first_sheet is not None:
                rel_id = first_sheet.attrib.get("{{http://schemas.openxmlformats.org/officeDocument/2006/relationships}}id")
            if rel_id:
                rels = ET.fromstring(zf.read("xl/_rels/workbook.xml.rels"))
                for rel in rels.findall(".//{{*}}Relationship"):
                    if rel.attrib.get("Id") == rel_id:
                        target = rel.attrib.get("Target", "")
                        sheet_path = "xl/" + target.lstrip("/")
                        break

        root = ET.fromstring(zf.read(sheet_path))
        raw_rows = []
        for row in root.findall(".//{{*}}sheetData/{{*}}row"):
            values = []
            for cell in row.findall("{{*}}c"):
                idx = col_index(cell.attrib.get("r", "A1"))
                while len(values) <= idx:
                    values.append(None)
                value = xml_text(cell.find("{{*}}v"))
                if cell.attrib.get("t") == "s" and value not in (None, ""):
                    value = shared[int(value)]
                elif cell.attrib.get("t") == "inlineStr":
                    value = xml_text(cell.find("{{*}}is"))
                values[idx] = value
            raw_rows.append(values)
        return normalize_rows(raw_rows)

try:
    rows = read_with_openpyxl()
except Exception:
    rows = read_with_xlsx_xml()

print(json.dumps(rows, ensure_ascii=False))
"#
    );

    let output = duct::cmd(python_command(), ["-c".to_string(), python_script])
        .stdout_capture()
        .stderr_capture()
        .run()
        .map_err(|e| WikiError::Parse(format!("Python/Excel error: {e}")))?;

    let json_str = String::from_utf8_lossy(&output.stdout);
    let data: Vec<serde_json::Value> = serde_json::from_str(&json_str).map_err(|e| {
        WikiError::Parse(format!(
            "Excel conversion failed: {e}\n{}",
            String::from_utf8_lossy(&output.stderr)
        ))
    })?;
    if data.is_empty() {
        return Err(WikiError::Parse("Excel file has no data rows".into()));
    }

    let table_name = name
        .map(|n| slugify(n))
        .unwrap_or_else(|| slugify(&path.file_stem().unwrap_or_default().to_string_lossy()));
    let _ = delete_table(&table_name);

    let first = &data[0];
    let mut fields = Vec::new();
    if let Some(obj) = first.as_object() {
        for (k, v) in obj {
            fields.push(serde_json::json!({"name": k, "type": infer_json_type(v)}));
        }
    }

    let fields_json = serde_json::to_string(&fields)?;
    let safe_name = create_table(
        name.unwrap_or(&table_name),
        &fields_json,
        None,
        false,
        Some(&table_name),
        "",
    )?;

    for row in &data {
        let json_str = serde_json::to_string(row)?;
        insert_data(&safe_name, &json_str, true)?;
    }

    Ok(format!("{safe_name} ({} rows)", data.len()))
}

fn python_command() -> String {
    if let Ok(path) = std::env::var("LLM_WIKI_PYTHON") {
        if !path.trim().is_empty() {
            return path;
        }
    }
    if cfg!(windows) {
        "python".into()
    } else {
        "python3".into()
    }
}

fn infer_json_type(v: &serde_json::Value) -> &str {
    match v {
        serde_json::Value::Number(_) => "number",
        serde_json::Value::Bool(_) => "boolean",
        _ => "string",
    }
}

pub fn import_csv(file: &str, name: Option<&str>) -> WikiResult<String> {
    let path = PathBuf::from(file);
    if !path.exists() {
        return Err(WikiError::NotFound(format!("File not found: {file}")));
    }

    let display_name = name.map(|n| n.to_string()).unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });
    let table_name = slugify(&display_name);
    let _ = delete_table(&table_name);

    let delimiter = if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("tsv"))
        .unwrap_or(false)
    {
        b'\t'
    } else {
        b','
    };
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_path(&path)
        .map_err(|e| WikiError::Parse(format!("CSV open failed: {e}")))?;
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| WikiError::Parse(format!("CSV header failed: {e}")))?
        .iter()
        .map(|h| h.trim_start_matches('\u{feff}').trim().to_string())
        .collect();
    if headers.is_empty() {
        return Err(WikiError::Parse("CSV has no header".into()));
    }

    let mut records = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| WikiError::Parse(format!("CSV row failed: {e}")))?;
        records.push(rec);
    }
    if records.is_empty() {
        return Err(WikiError::Parse("CSV has no data rows".into()));
    }

    let mut type_hints: Vec<&str> = vec!["string"; headers.len()];
    if let Some(first) = records.first() {
        for (i, v) in first.iter().enumerate().take(headers.len()) {
            if v.parse::<i64>().is_ok() {
                type_hints[i] = "integer";
            } else if v.parse::<f64>().is_ok() {
                type_hints[i] = "number";
            }
        }
    }

    let fields: Vec<serde_json::Value> = headers
        .iter()
        .enumerate()
        .map(|(i, c)| serde_json::json!({"name": c, "type": type_hints[i]}))
        .collect();

    let table_name = create_table(
        &display_name,
        &serde_json::to_string(&fields)?,
        None,
        false,
        Some(&table_name),
        "",
    )?;

    for rec in records {
        let mut obj = serde_json::Map::new();
        for (i, header) in headers.iter().enumerate() {
            let value = rec.get(i).unwrap_or("").trim();
            let json_value = if value.is_empty() {
                serde_json::Value::Null
            } else if let Ok(n) = value.parse::<i64>() {
                serde_json::json!(n)
            } else if let Ok(n) = value.parse::<f64>() {
                serde_json::json!(n)
            } else {
                serde_json::json!(value)
            };
            obj.insert(header.clone(), json_value);
        }
        let json_str = serde_json::to_string(&serde_json::Value::Object(obj))?;
        insert_data(&table_name, &json_str, true)?;
    }

    Ok(table_name)
}
