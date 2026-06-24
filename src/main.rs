mod cli;
mod engine;
mod ollama;
mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};
use rmcp::{ServiceExt, transport::io::stdio};
use server::McpServer;

#[derive(Parser)]
#[command(
    name = "pipetable",
    about = "Query local data files with natural language or SQL",
    long_about = None,
)]
struct Cli {
    /// Data file or folder to load into the REPL
    path: Option<String>,

    /// Ollama model to use for natural language queries
    #[arg(long, short, default_value = "qwen2.5-coder:1.5b")]
    model: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start as MCP server (for RooCode, Cursor, Copilot, Claude Code)
    Mcp,
    /// Ask a one-shot natural language question and exit
    Ask {
        /// Your question in plain English, or a SQL query
        question: String,
        /// Data file or folder to load
        path: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    match args.command {
        Some(Command::Mcp) => {
            // MCP speaks JSON over stdio — no ANSI in output
            colored::control::set_override(false);
            eprintln!("Pipetable MCP Server — MIT — pipetable.com");
            McpServer::new()?.serve(stdio()).await?.waiting().await?;
        }
        Some(Command::Ask { question, path }) => {
            cli::ask(&question, path.as_deref(), Some(&args.model)).await?;
        }
        None => {
            cli::run(args.path.as_deref(), Some(&args.model)).await?;
        }
    }
    Ok(())
}
