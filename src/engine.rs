use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use colored::Colorize;
use duckdb::{Connection, types::ValueRef};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub col_type: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DatasetInfo {
    pub name: String,
    pub path: String,
    pub format: String,
    pub columns: Vec<ColumnInfo>,
    pub row_count: i64,
    pub size_bytes: u64,
    pub modified_secs: u64,
}

// ─── State ────────────────────────────────────────────────────────────────────

pub struct State {
    pub conn: Connection,
    pub datasets: HashMap<String, DatasetInfo>,
}

impl State {
    pub fn new() -> Result<Self> {
        Ok(Self { conn: Connection::open_in_memory()?, datasets: HashMap::new() })
    }

    pub fn scan(&mut self, folder: &str) -> String {
        do_scan(self, folder, None, false)
    }

    pub fn scan_verbose(&mut self, folder: &str, interactive: bool) {
        do_scan(self, folder, Some(&|msg: &str| eprintln!("{msg}")), interactive);
    }

    pub fn list(&self) -> String {
        if self.datasets.is_empty() {
            return "No datasets registered. Scan a folder first.".into();
        }
        let mut out = format!("{} dataset(s):\n\n", self.datasets.len());
        for ds in self.datasets.values() {
            let rows = if ds.row_count >= 0 { ds.row_count.to_string() } else { "?".into() };
            out.push_str(&format!(
                "  {} ({} - {} rows - {})\n",
                ds.name, ds.format.to_uppercase(), rows, fmt_bytes(ds.size_bytes)
            ));
            for col in &ds.columns {
                out.push_str(&format!("    {} : {}\n", col.name, col.col_type));
            }
        }
        out
    }

    pub fn schema(&self, name: &str) -> String {
        let ds = match self.datasets.get(name) {
            Some(d) => d.clone(),
            None => {
                let known: Vec<&String> = self.datasets.keys().collect();
                return format!(
                    "Dataset '{}' not found. Available: {}",
                    name,
                    known.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
                );
            }
        };
        let rows_str = if ds.row_count >= 0 { ds.row_count.to_string() } else { "unknown".into() };
        let mut out = format!(
            "Dataset:  {}\nFormat:   {}\nPath:     {}\nRows:     {}\nSize:     {}\n\nColumns:\n",
            ds.name, ds.format.to_uppercase(), ds.path, rows_str, fmt_bytes(ds.size_bytes)
        );
        for col in &ds.columns {
            out.push_str(&format!("  {} : {}\n", col.name, col.col_type));
        }
        let sample_sql = format!("SELECT * FROM \"{}\" LIMIT 3", ds.name.replace('"', "\"\""));
        match run_query(&self.conn, &sample_sql) {
            Ok((cols, rows)) => {
                out.push_str("\nSample rows:\n");
                out.push_str(&fmt_table(&cols, &rows));
            }
            Err(e) => out.push_str(&format!("\n(could not fetch samples: {e})")),
        }
        out
    }

    pub fn query(&self, sql: &str) -> String {
        if self.datasets.is_empty() {
            return "No datasets loaded. Scan a folder first.".into();
        }
        let sql = ensure_limit(sql, 500);
        match run_query(&self.conn, &sql) {
            Ok((cols, rows)) => {
                let count = format!("{} row(s)", rows.len()).dimmed().to_string();
                format!("{count}\n\n{}", fmt_table(&cols, &rows))
            }
            Err(e) => format!("{} {e}", "SQL error:".red().bold()),
        }
    }

    /// Compact schema string for LLM prompts.
    /// If `only` is non-empty, only include those dataset names.
    pub fn schema_prompt(&self, only: &[&str]) -> String {
        if self.datasets.is_empty() { return String::new(); }
        let mut out = String::new();
        for ds in self.datasets.values() {
            if !only.is_empty() && !only.iter().any(|n| n.eq_ignore_ascii_case(&ds.name)) {
                continue;
            }
            out.push_str(&format!("Table: {}\nColumns:", ds.name));
            for col in &ds.columns {
                out.push_str(&format!(" {} ({}),", col.name, col.col_type));
            }
            out.push('\n');
        }
        out
    }
}

// ─── Scan ─────────────────────────────────────────────────────────────────────

const SUPPORTED: &[&str] = &["csv", "parquet", "json", "ndjson", "tsv"];

pub fn do_scan(s: &mut State, folder: &str, progress: Option<&dyn Fn(&str)>, interactive: bool) -> String {
    use std::collections::BTreeMap;
    use std::io::Write;

    if !Path::new(folder).exists() {
        return format!("Path does not exist: {folder}");
    }

    let emit = |msg: String| { if let Some(f) = progress { f(&msg); } };

    // ── Phase 1: pre-scan count ───────────────────────────────────────────────
    let mut type_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;
    for entry in WalkDir::new(folder).max_depth(3).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() { continue; }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !SUPPORTED.contains(&ext.as_str()) { continue; }
        *type_counts.entry(ext.to_uppercase()).or_insert(0) += 1;
        total += 1;
    }

    if total == 0 {
        emit(format!("{}", "No supported data files found (CSV, Parquet, JSON, TSV).".yellow()));
        return format!("No supported files in {folder}");
    }

    let type_summary = type_counts.iter()
        .map(|(ext, n)| format!("{}×{}", n.to_string().bright_cyan(), ext))
        .collect::<Vec<_>>()
        .join("  ");
    emit(format!(
        "Found {}  ({})",
        format!("{} files", total).bold(),
        type_summary
    ));

    // ── Phase 2: confirm if many ──────────────────────────────────────────────
    if interactive && total > 5 {
        eprint!("{}", format!("Scan all {}? [Y/n] ", total).bright_yellow());
        let _ = std::io::stderr().flush();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
        if input.trim().eq_ignore_ascii_case("n") {
            emit("Cancelled.".yellow().to_string());
            return "Scan cancelled.".to_string();
        }
    }

    // ── Phase 3: register each file with timing ───────────────────────────────
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut unchanged = 0usize;
    let mut skip = 0usize;
    const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

    for entry in WalkDir::new(folder).max_depth(3).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() { continue; }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !SUPPORTED.contains(&ext.as_str()) { continue; }

        let name = sanitize(&path.file_stem().and_then(|s| s.to_str()).unwrap_or("file"));
        let path_str = path.to_string_lossy().to_string();
        let meta = std::fs::metadata(path).ok();
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified_secs = meta.as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if size > MAX_FILE_SIZE {
            emit(format!("  {}  {}  {}", "✗".red(), truncate_pad(&name, 36).dimmed(), "too large".dimmed()));
            skip += 1;
            continue;
        }

        // Skip if already loaded with identical size + mtime
        if let Some(existing) = s.datasets.get(&name) {
            if existing.size_bytes == size && existing.modified_secs == modified_secs {
                unchanged += 1;
                continue;
            }
        }

        let is_update = s.datasets.contains_key(&name);
        let t = std::time::Instant::now();
        let view_result = match ext.as_str() {
            "csv" | "tsv" => create_csv_view(&s.conn, &path_str, &name),
            "parquet" => {
                let safe = path_str.replace('\'', "''");
                s.conn.execute_batch(&format!(
                    "CREATE OR REPLACE VIEW \"{name}\" AS SELECT * FROM parquet_scan('{safe}')"
                )).map_err(|e| e.to_string())
            }
            "json" | "ndjson" => {
                let safe = path_str.replace('\'', "''");
                s.conn.execute_batch(&format!(
                    "CREATE OR REPLACE VIEW \"{name}\" AS SELECT * FROM read_json_auto('{safe}')"
                )).map_err(|e| e.to_string())
            }
            _ => continue,
        };
        let ms = t.elapsed().as_millis();

        match view_result {
            Ok(()) => {
                let columns = describe(&s.conn, &name).unwrap_or_default();
                let ncols = columns.len();
                let tag = if is_update { "updated".yellow() } else { "new".green() };
                emit(format!(
                    "  {}  {}  {}  {}  {}  {}",
                    "✓".green(),
                    truncate_pad(&name, 36).bold(),
                    format!("{:>4} cols", ncols).dimmed(),
                    format!("{:>8}", fmt_bytes(size)).dimmed(),
                    format!("{:>6}ms", ms).dimmed(),
                    tag,
                ));
                s.datasets.insert(name.clone(), DatasetInfo {
                    name, path: path_str, format: ext, columns, row_count: -1,
                    size_bytes: size, modified_secs,
                });
                if is_update { updated += 1; } else { added += 1; }
            }
            Err(e) => {
                emit(format!("  {}  {}  {}", "✗".red(), truncate_pad(&name, 36).dimmed(), e.dimmed()));
                skip += 1;
            }
        }
    }

    let mut parts = vec![];
    if added > 0   { parts.push(format!("{} new", added)); }
    if updated > 0 { parts.push(format!("{} updated", updated)); }
    if unchanged > 0 { parts.push(format!("{} unchanged", unchanged).dimmed().to_string()); }
    if skip > 0    { parts.push(format!("{} skipped", skip).yellow().to_string()); }
    emit(format!("\n{}  {}", "Done.".green().bold(), parts.join("  ")));

    let ok = added + updated;
    let mut out = format!("Scanned {folder}. {ok} dataset(s) registered.\n");
    for ds in s.datasets.values() {
        out.push_str(&format!("  {} ({}, {})\n", ds.name, ds.format.to_uppercase(), fmt_bytes(ds.size_bytes)));
    }
    out
}

// ─── DuckDB helpers ───────────────────────────────────────────────────────────

pub fn create_csv_view(conn: &Connection, path: &str, name: &str) -> Result<(), String> {
    let norm = path.trim().trim_start_matches("file://").trim_start_matches("file:");
    if !Path::new(norm).exists() { return Err(format!("File not found: {norm}")); }
    let safe = norm.replace('\'', "''");
    let attempts = [
        format!("read_csv('{safe}', auto_detect=true)"),
        format!("read_csv('{safe}', auto_detect=true, strict_mode=false)"),
        format!("read_csv('{safe}', auto_detect=true, strict_mode=false, null_padding=true)"),
        format!("read_csv('{safe}', auto_detect=true, ignore_errors=true, all_varchar=true, null_padding=true, strict_mode=false)"),
    ];
    let mut last = String::new();
    for expr in &attempts {
        match conn.execute_batch(&format!("CREATE OR REPLACE VIEW \"{name}\" AS SELECT * FROM {expr}")) {
            Ok(()) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

pub fn describe(conn: &Connection, name: &str) -> Result<Vec<ColumnInfo>> {
    let mut stmt = conn.prepare(&format!("DESCRIBE \"{name}\""))?;
    let mut rows = stmt.query([])?;
    let mut cols = Vec::new();
    while let Some(row) = rows.next()? {
        cols.push(ColumnInfo {
            name: row.get::<_, String>(0).unwrap_or_default(),
            col_type: row.get::<_, String>(1).unwrap_or_default(),
        });
    }
    Ok(cols)
}

pub fn run_query(conn: &Connection, sql: &str) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>)> {
    let mut stmt = conn.prepare(sql)?;
    let raw_rows: Vec<Vec<serde_json::Value>> = {
        let mut rows_iter = stmt.query([])?;
        let mut rows = Vec::new();
        let mut bytes = 0usize;
        let mut width: Option<usize> = None;
        while let Some(row) = rows_iter.next()? {
            let vals: Vec<serde_json::Value> = if width.is_none() {
                let mut v = Vec::new();
                let mut i = 0usize;
                loop {
                    match row.get_ref(i) {
                        Ok(val) => { v.push(val_to_json(val)); i += 1; }
                        Err(_) => break,
                    }
                }
                width = Some(v.len());
                v
            } else {
                let w = width.unwrap();
                (0..w).map(|i| row.get_ref(i).map(val_to_json).unwrap_or(serde_json::Value::Null)).collect()
            };
            bytes += serde_json::to_string(&vals).map(|s| s.len()).unwrap_or(256);
            if bytes > 5 * 1024 * 1024 { break; }
            rows.push(vals);
        }
        rows
    };
    let n = stmt.column_count();
    let col_names: Vec<String> = (0..n)
        .filter_map(|i| stmt.column_name(i).ok().map(|s| s.to_string()))
        .collect();
    Ok((col_names, raw_rows))
}

pub fn val_to_json(v: ValueRef<'_>) -> serde_json::Value {
    match v {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Boolean(b) => serde_json::Value::Bool(b),
        ValueRef::TinyInt(n) => serde_json::json!(n),
        ValueRef::SmallInt(n) => serde_json::json!(n),
        ValueRef::Int(n) => serde_json::json!(n),
        ValueRef::BigInt(n) => serde_json::json!(n),
        ValueRef::HugeInt(n) => serde_json::json!(n),
        ValueRef::UTinyInt(n) => serde_json::json!(n),
        ValueRef::USmallInt(n) => serde_json::json!(n),
        ValueRef::UInt(n) => serde_json::json!(n),
        ValueRef::UBigInt(n) => serde_json::json!(n),
        ValueRef::Float(f) => serde_json::Number::from_f64(f as f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        ValueRef::Double(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        ValueRef::Text(b) => serde_json::Value::String(std::str::from_utf8(b).unwrap_or("").to_string()),
        ValueRef::Blob(b) => serde_json::Value::String(format!("<blob {} bytes>", b.len())),
        other => serde_json::Value::String(format!("{other:?}")),
    }
}

// ─── Formatting ───────────────────────────────────────────────────────────────

pub fn fmt_table(cols: &[String], rows: &[Vec<serde_json::Value>]) -> String {
    if cols.is_empty() || rows.is_empty() { return "(no rows)\n".into(); }
    let mut widths: Vec<usize> = cols.iter().map(|c| c.len()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() { widths[i] = widths[i].max(json_str(val).len().min(40)); }
        }
    }
    let mut out = String::new();

    // Header — plain padded first, then dim the whole line
    let mut hdr = String::new();
    for (i, col) in cols.iter().enumerate() {
        hdr.push_str(&format!("{:width$}  ", col, width = widths[i].min(40)));
    }
    out.push_str(&format!("{}\n", hdr.bold().dimmed()));

    // Separator
    let mut sep = String::new();
    for &w in &widths { sep.push_str(&"─".repeat(w.min(40) + 2)); }
    out.push_str(&format!("{}\n", sep.dimmed()));

    // Data rows
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            let w = if i < widths.len() { widths[i].min(40) } else { 10 };
            let s = json_str(val);
            let cell = if s.len() > w { format!("{}~", &s[..w.saturating_sub(1)]) } else { s };
            let padded = format!("{:width$}  ", cell, width = w);
            let colored = match val {
                serde_json::Value::Number(_) => padded.bright_cyan().to_string(),
                serde_json::Value::Null => padded.dimmed().to_string(),
                serde_json::Value::Bool(_) => padded.bright_yellow().to_string(),
                _ => padded,
            };
            out.push_str(&colored);
        }
        out.push('\n');
    }
    out
}

pub fn json_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "NULL".into(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn ensure_limit(sql: &str, cap: usize) -> String {
    let upper = sql.trim().to_uppercase();
    if (upper.starts_with("SELECT") || upper.starts_with("WITH")) && !upper.contains("LIMIT") {
        format!("{} LIMIT {cap}", sql.trim().trim_end_matches(';'))
    } else {
        sql.to_string()
    }
}

pub fn truncate_pad(s: &str, width: usize) -> String {
    if s.len() > width {
        format!("{}~", &s[..width - 1])
    } else {
        format!("{:<width$}", s)
    }
}

pub fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

pub fn fmt_bytes(b: u64) -> String {
    if b >= 1 << 30 { format!("{:.1}GB", b as f64 / (1 << 30) as f64) }
    else if b >= 1 << 20 { format!("{:.1}MB", b as f64 / (1 << 20) as f64) }
    else if b >= 1 << 10 { format!("{:.0}KB", b as f64 / (1 << 10) as f64) }
    else { format!("{b}B") }
}
