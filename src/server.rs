use std::sync::Arc;

use anyhow::Result;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::engine::{State, do_scan, ensure_limit, fmt_bytes, fmt_table, run_query};

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

// ─── Server ───────────────────────────────────────────────────────────────────

type Job = Box<dyn FnOnce(&mut State) -> String + Send + 'static>;

#[derive(Clone)]
pub struct McpServer {
    tx: Arc<mpsc::UnboundedSender<(Job, oneshot::Sender<String>)>>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl McpServer {
    pub fn new() -> Result<Self> {
        let mut state = State::new()?;
        let (tx, mut rx) = mpsc::unbounded_channel::<(Job, oneshot::Sender<String>)>();
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
        self.run(move |s| do_scan(s, &p.path, None, false)).await
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
        self.run(move |s| s.schema(&p.name)).await
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
    name = "pipetable",
    version = "0.1.0",
    instructions = "Local data query engine. Files never leave the machine. Workflow: (1) scan_folder to register files, (2) list_datasets to see schemas, (3) execute_sql with real SQL. Results come from DuckDB - ground truth, no hallucination."
)]
impl ServerHandler for McpServer {}
