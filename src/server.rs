use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use duckdb::{Connection, types::ValueRef};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use walkdir::WalkDir;

// ─── Domain types ─────────────────────────────────────────────────────────────

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
}

// ─── Worker state (lives on a single dedicated task) ──────────────────────────

struct State {
    conn: Connection,
    datasets: HashMap<String, DatasetInfo>,
}

type Job = Box<dyn FnOnce(&mut State) -> String + Send + 'static>;

// ─── Tool parameter types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ScanFolderParams {
    /// Absolute path to the folder to scan (max depth 3). Supports CSV, Parquet, JSON, TSV.
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetSchemaParams {
    /// Dataset name exactly as returned by list_datasets (e.g. "sales_2025")
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExecuteSqlParams {
    /// SQL query to run. Use dataset names as table names. Always include LIMIT.
    /// Example: SELECT region, SUM(revenue) FROM sales GROUP BY region ORDER BY 2 DESC LIMIT 20
    pub sql: String,
}

// ─── Server (Clone-friendly handle to the worker) ────────────────────────────

#[derive(Clone)]
pub struct McpServer {
    tx: Arc<mpsc::UnboundedSender<(Job, oneshot::Sender<String>)>>,
    #[allow(dead_code)] // held for rmcp routing machinery
    tool_router: ToolRouter<Self>,
}

impl McpServer {
    pub fn new() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut state = State { conn, datasets: HashMap::new() };

        let (tx, mut rx) = mpsc::unbounded_channel::<(Job, oneshot::Sender<String>)>();

        // Single worker task — all tool calls run here, FIFO, no concurrency
        tokio::spawn(async move {
            while let Some((job, reply)) = rx.recv().await {
                let result = job(&mut state);
                let _ = reply.send(result);
            }
        });

        Ok(Self { tx: Arc::new(tx), tool_router: Self::tool_router() })
    }

    async fn run(&self, f: impl FnOnce(&mut State) -> String + Send + 'static) -> String {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self.tx.send((Box::new(f), reply_tx)).is_err() {
            return "Worker unavailable".into();
        }
        reply_rx.await.unwrap_or_else(|_| "Worker dropped".into())
    }
}

// ─── Tools ────────────────────────────────────────────────────────────────────

#[tool_router]
impl McpServer {
    #[tool(description = "Scan a folder and register all data files for querying. Call this first. Supports CSV, Parquet, JSON, TSV up to 3 folders deep.")]
    async fn scan_folder(&self, Parameters(p): Parameters<ScanFolderParams>) -> String {
        self.run(move |s| do_scan(s, &p.path)).await
    }

    #[tool(description = "List all registered datasets with their column names and types. Run scan_folder first.")]
    async fn list_datasets(&self) -> String {
        self.run(|s| {
            if s.datasets.is_empty() {
                return "No datasets registered. Call scan_folder with a folder path first.".into();
            }
            let mut out = format!("{} dataset(s) available:\n\n", s.datasets.len());
            for ds in s.datasets.values() {
                let rows = if ds.row_count >= 0 { ds.row_count.to_string() } else { "unknown".into() };
                out.push_str(&format!(
                    "### {} ({} - {} rows - {})\n",
                    ds.name, ds.format.to_uppercase(), rows, fmt_bytes(ds.size_bytes)
                ));
                for col in &ds.columns {
                    out.push_str(&format!("  - {} : {}\n", col.name, col.col_type));
                }
                out.push('\n');
            }
            out
        })
        .await
    }

    #[tool(description = "Get full schema and 3 sample rows for one dataset. Use the exact name from list_datasets.")]
    async fn get_schema(&self, Parameters(p): Parameters<GetSchemaParams>) -> String {
        self.run(move |s| {
            let ds = match s.datasets.get(&p.name) {
                Some(d) => d.clone(),
                None => {
                    let known: Vec<&String> = s.datasets.keys().collect();
                    return format!(
                        "Dataset '{}' not found. Available: {}",
                        p.name,
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
            match run_query(&s.conn, &sample_sql) {
                Ok((cols, rows)) => {
                    out.push_str("\nSample rows:\n");
                    out.push_str(&fmt_table(&cols, &rows));
                }
                Err(e) => out.push_str(&format!("\n(could not fetch samples: {e})")),
            }
            out
        })
        .await
    }

    #[tool(description = "Execute SQL against registered datasets. Dataset names are table names. Results are real data from DuckDB - not generated. Cap: 500 rows / 5MB.")]
    async fn execute_sql(&self, Parameters(p): Parameters<ExecuteSqlParams>) -> String {
        self.run(move |s| {
            if s.datasets.is_empty() {
                return "No datasets loaded. Call scan_folder first.".into();
            }
            let sql = ensure_limit(&p.sql, 500);
            match run_query(&s.conn, &sql) {
                Ok((cols, rows)) => format!("{} row(s)\n\n{}", rows.len(), fmt_table(&cols, &rows)),
                Err(e) => format!("SQL error: {e}"),
            }
        })
        .await
    }
}

#[tool_handler(
    name = "pipetable-mcp",
    version = "0.1.0",
    instructions = "Local data query engine. Files never leave the machine. Workflow: (1) scan_folder to register files, (2) list_datasets to see schemas, (3) execute_sql with real SQL. Results come from DuckDB - ground truth, no hallucination."
)]
impl ServerHandler for McpServer {}

// ─── Scan logic ───────────────────────────────────────────────────────────────

const SUPPORTED: &[&str] = &["csv", "parquet", "json", "ndjson", "tsv"];

fn do_scan(s: &mut State, folder: &str) -> String {
    if !Path::new(folder).exists() {
        return format!("Path does not exist: {folder}");
    }
    let mut ok = 0usize;
    let mut skipped: Vec<String> = Vec::new();

    for entry in WalkDir::new(folder).max_depth(3).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() { continue; }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !SUPPORTED.contains(&ext.as_str()) { continue; }

        let name = sanitize(&path.file_stem().and_then(|s| s.to_str()).unwrap_or("file"));
        let path_str = path.to_string_lossy().to_string();
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        let view_result = match ext.as_str() {
            "csv" | "tsv" => create_csv_view(&s.conn, &path_str, &name),
            "parquet" => {
                let safe = path_str.replace('\'', "''");
                s.conn
                    .execute_batch(&format!(
                        "CREATE OR REPLACE VIEW \"{name}\" AS SELECT * FROM parquet_scan('{safe}')"
                    ))
                    .map_err(|e| e.to_string())
            }
            "json" | "ndjson" => {
                let safe = path_str.replace('\'', "''");
                s.conn
                    .execute_batch(&format!(
                        "CREATE OR REPLACE VIEW \"{name}\" AS SELECT * FROM read_json_auto('{safe}')"
                    ))
                    .map_err(|e| e.to_string())
            }
            _ => continue,
        };

        if let Err(e) = view_result {
            skipped.push(format!("  [skip] {} - {}", path.display(), e));
            continue;
        }

        let columns = describe(&s.conn, &name).unwrap_or_default();
        let row_count = if size <= 200 * 1024 * 1024 {
            s.conn
                .query_row(&format!("SELECT COUNT(*) FROM \"{name}\""), [], |r| r.get(0))
                .unwrap_or(-1)
        } else {
            -1
        };

        s.datasets.insert(name.clone(), DatasetInfo {
            name, path: path_str, format: ext, columns, row_count, size_bytes: size,
        });
        ok += 1;
    }

    let mut out = format!("Scanned: {folder}\nRegistered {ok} dataset(s).\n\n");
    for ds in s.datasets.values() {
        let rows = if ds.row_count >= 0 { ds.row_count.to_string() } else { "?".into() };
        out.push_str(&format!("  [ok] {} - {} rows, {}\n", ds.name, rows, fmt_bytes(ds.size_bytes)));
    }
    if !skipped.is_empty() {
        out.push_str(&format!("\nSkipped {} file(s):\n", skipped.len()));
        for s in &skipped { out.push_str(&format!("{s}\n")); }
    }
    out
}

// ─── DuckDB helpers ───────────────────────────────────────────────────────────

fn create_csv_view(conn: &Connection, path: &str, name: &str) -> Result<(), String> {
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

fn describe(conn: &Connection, name: &str) -> Result<Vec<ColumnInfo>> {
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

fn run_query(conn: &Connection, sql: &str) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>)> {
    let mut stmt = conn.prepare(sql)?;

    // Phase 1: execute and collect rows (rows_iter borrows stmt)
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
    }; // rows_iter dropped — stmt borrow released

    // Phase 2: column names after execution
    let n = stmt.column_count();
    let col_names: Vec<String> = (0..n)
        .filter_map(|i| stmt.column_name(i).ok().map(|s| s.to_string()))
        .collect();

    Ok((col_names, raw_rows))
}

fn val_to_json(v: ValueRef<'_>) -> serde_json::Value {
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

fn fmt_table(cols: &[String], rows: &[Vec<serde_json::Value>]) -> String {
    if cols.is_empty() || rows.is_empty() { return "(no rows)\n".into(); }
    let mut widths: Vec<usize> = cols.iter().map(|c| c.len()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() { widths[i] = widths[i].max(json_str(val).len().min(40)); }
        }
    }
    let mut out = String::new();
    for (i, col) in cols.iter().enumerate() {
        out.push_str(&format!("{:width$}  ", col, width = widths[i].min(40)));
    }
    out.push('\n');
    for &w in &widths { out.push_str(&"-".repeat(w.min(40) + 2)); }
    out.push('\n');
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            let w = if i < widths.len() { widths[i].min(40) } else { 10 };
            let s = json_str(val);
            let cell = if s.len() > w { format!("{}~", &s[..w.saturating_sub(1)]) } else { s };
            out.push_str(&format!("{:width$}  ", cell, width = w));
        }
        out.push('\n');
    }
    out
}

fn json_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "NULL".into(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn ensure_limit(sql: &str, cap: usize) -> String {
    let upper = sql.trim().to_uppercase();
    if (upper.starts_with("SELECT") || upper.starts_with("WITH")) && !upper.contains("LIMIT") {
        format!("{} LIMIT {cap}", sql.trim().trim_end_matches(';'))
    } else {
        sql.to_string()
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1 << 30 { format!("{:.1}GB", b as f64 / (1 << 30) as f64) }
    else if b >= 1 << 20 { format!("{:.1}MB", b as f64 / (1 << 20) as f64) }
    else if b >= 1 << 10 { format!("{:.0}KB", b as f64 / (1 << 10) as f64) }
    else { format!("{b}B") }
}
