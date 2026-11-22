//! agent-kanban — multi-agent project management kanban.
//! One binary: HTTP kanban UI + REST API, plus a stdio MCP server for agents.
//! Each agent (identified by AGENT_ID / --agent / cwd) gets its own isolated workspace.

mod http;
mod mcp;
mod model;
mod store;
mod webui;

use clap::{Parser, Subcommand};
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "agent-kanban", version, about = "Multi-agent project kanban (HTTP + MCP)")]
struct Cli {
    /// SQLite database path
    #[arg(long, global = true, default_value = "agent-kanban.db")]
    db: String,

    /// Agent workspace identity (used by MCP mode). Overrides AGENT_ID env.
    #[arg(long, global = true)]
    agent: Option<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the HTTP kanban server
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8096)]
        port: u16,
    },
    /// Run the MCP stdio server (for one agent)
    Mcp,
}

fn resolve_agent(explicit: Option<String>) -> String {
    if let Some(a) = explicit {
        if !a.trim().is_empty() { return a.trim().to_string(); }
    }
    if let Ok(a) = std::env::var("AGENT_ID") {
        if !a.trim().is_empty() { return a.trim().to_string(); }
    }
    // fall back to current working directory basename → a stable per-agent identity
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(name) = cwd.file_name().and_then(|s| s.to_str()) {
            if !name.is_empty() { return name.to_string(); }
        }
    }
    "default".to_string()
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let store = Arc::new(store::Store::open(&cli.db).expect("open db"));

    match cli.cmd {
        Cmd::Serve { host, port } => {
            http::serve(store, &host, port).await;
        }
        Cmd::Mcp => {
            let agent = resolve_agent(cli.agent.clone());
            let ws = store.ensure_workspace(&agent);
            eprintln!("[agent-kanban] MCP stdio; agent='{}' workspace='{}'", agent, ws);
            mcp::run(store, ws);
        }
    }
}
