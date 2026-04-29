//! `windbgr-mcp` command line entry point.
//!
//! Stays small on purpose: argument parsing, tracing initialisation, then
//! a hand-off to [`windbgr_mcp::app`] which owns the actual transport
//! lifecycles.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::{fmt, EnvFilter};

use windbgr_mcp::app::{check_env, enable_debug_privilege, serve_http, serve_stdio};
use windbgr_mcp::config::Config;

#[derive(Debug, Parser)]
#[command(
    name = "windbgr-mcp",
    version,
    about = "MCP server for Windows process discovery and cdb debugging"
)]
struct Cli {
    /// Path to a TOML configuration file.
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Override log level (e.g. `debug`, `info`, `trace`).
    #[arg(long, global = true, env = "WINDBGR_MCP_LOG")]
    log: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the MCP server on stdio. Use with MCP clients that spawn a
    /// child process.
    ServeStdio,
    /// Start the MCP server over Streamable HTTP (with optional Bearer
    /// token auth).
    ServeHttp {
        /// Bind address (overrides configuration).
        #[arg(long)]
        bind: Option<String>,
    },
    /// Inspect the local environment: cdb path, auth config, session
    /// limits.
    CheckEnv,
}

fn init_tracing(override_level: Option<&str>) {
    let default_filter = override_level.unwrap_or("info");
    let filter = EnvFilter::try_from_env("WINDBGR_MCP_LOG")
        .unwrap_or_else(|_| EnvFilter::new(default_filter));
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.log.as_deref());

    let privilege = enable_debug_privilege();
    let cfg = Config::load(cli.config.as_deref())?;

    match cli.command {
        Command::ServeStdio => serve_stdio(cfg, privilege).await,
        Command::ServeHttp { bind } => serve_http(cfg, privilege, bind).await,
        Command::CheckEnv => check_env(&cfg, privilege, cli.config.as_deref()),
    }
}
