//! Entry point: serves the Vibez MCP tools over stdio.

use rmcp::transport::stdio;
use rmcp::ServiceExt;
use vibez_mcp::server::VibezServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = VibezServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
