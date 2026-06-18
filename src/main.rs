mod server;

use anyhow::Result;
use rmcp::{ServiceExt, transport::io::stdio};
use server::McpServer;

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("Pipetable MCP Server v0.1.0  —  https://pipetable.com");
    eprintln!("Free for individuals and teams ≤5 people.");
    eprintln!("Commercial license required for companies of 5+ people: https://pipetable.com/license");
    eprintln!();

    let server = McpServer::new()?;
    server.serve(stdio()).await?.waiting().await?;
    Ok(())
}
