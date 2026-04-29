//! Application entry points (`serve_stdio`, `serve_http`, `check_env`)
//! plus startup helpers shared by `main.rs`. Keeping this in `lib.rs`
//! makes it testable and trims `main.rs` down to CLI parsing and tracing
//! initialisation.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use axum::{middleware, Router};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{stdio, StreamableHttpServerConfig, StreamableHttpService};
use rmcp::ServiceExt;
use tracing::info;

use crate::config::Config;
use crate::mcp::WindbgrMcp;
use crate::privilege::PrivilegeLevel;
use crate::security::{bearer_auth, TokenState};

/// Run the MCP server on stdio. Used by clients that spawn `windbgr-mcp`
/// as a child process.
pub async fn serve_stdio(cfg: Config, privilege: PrivilegeLevel) -> anyhow::Result<()> {
    info!("starting MCP stdio transport");
    let handler = WindbgrMcp::new(&cfg, privilege).context("initialise mcp service")?;
    let transport = stdio();
    let running = handler
        .clone()
        .serve(transport)
        .await
        .context("serve stdio")?;
    let reason = running.waiting().await.context("stdio wait")?;
    info!(?reason, "stdio session ended");
    handler.sessions().shutdown().await;
    Ok(())
}

/// Run the MCP server over Streamable HTTP. `bind_override` lets the CLI
/// take precedence over `cfg.server.bind`.
pub async fn serve_http(
    cfg: Config,
    privilege: PrivilegeLevel,
    bind_override: Option<String>,
) -> anyhow::Result<()> {
    let bind: SocketAddr = bind_override
        .as_deref()
        .unwrap_or(&cfg.server.bind)
        .parse()
        .context("invalid bind address")?;

    let token = cfg.resolved_token();
    let token_state = TokenState {
        expected: token.clone(),
    };
    if token.is_none() {
        tracing::warn!(
            "HTTP transport started without Bearer token — anyone with network \
             access to {bind} can drive the debugger. Set `auth.bearer_token` or \
             `auth.bearer_token_env` in the configuration file."
        );
    }

    // IMPORTANT: build the handler ONCE and share it across all MCP
    // sessions. rmcp's `StreamableHttpService` calls `service_factory()`
    // every time a new MCP session is created (i.e. on every POST without
    // `Mcp-Session-Id`). If we constructed a fresh `WindbgrMcp` per call,
    // each MCP session would get its own empty `SessionManager`, so cdb
    // session ids minted in one MCP session would be invisible to the
    // next one — resulting in spurious "cdb session not found" errors
    // when the client reconnects between tool calls (HTTP/2 RST_STREAM,
    // SSE timeout, per-turn reconnects from chat clients, etc.). Cloning
    // a single shared handler keeps the same SessionManager (and thus
    // the live cdb child processes it owns) alive across MCP session
    // restarts.
    let handler = WindbgrMcp::new(&cfg, privilege).context("initialise mcp service")?;
    let service_factory = {
        let handler = handler.clone();
        move || Ok::<_, std::io::Error>(handler.clone())
    };
    let allowed_hosts = cfg.server.resolved_allowed_hosts();
    if allowed_hosts.is_empty() {
        tracing::warn!(
            "DNS rebinding protection is disabled (allowed_hosts = []) — \
             ANY Host header is accepted. Only safe inside a fully trusted intranet."
        );
    } else {
        tracing::info!(?allowed_hosts, "Streamable HTTP allowed Host headers");
    }
    let http_cfg = if allowed_hosts.is_empty() {
        StreamableHttpServerConfig::default().disable_allowed_hosts()
    } else {
        StreamableHttpServerConfig::default().with_allowed_hosts(allowed_hosts.clone())
    };
    let mcp_service = StreamableHttpService::new(
        service_factory,
        Arc::new(LocalSessionManager::default()),
        http_cfg,
    );

    let protected = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(token_state, bearer_auth));

    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .merge(protected);

    info!(%bind, "starting MCP streamable HTTP transport");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("axum serve");

    handler.sessions().shutdown().await;

    serve_result?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received");
}

/// Detect runtime privilege and emit the appropriate startup log line.
pub fn enable_debug_privilege() -> PrivilegeLevel {
    let level = crate::privilege::detect_privilege();
    match level {
        PrivilegeLevel::Admin => tracing::info!("SeDebugPrivilege enabled (admin)"),
        PrivilegeLevel::User => tracing::warn!(
            "running as normal user — can only debug own processes. \
             Re-launch from an elevated Administrator prompt to debug \
             system services."
        ),
    }
    level
}

/// Implementation of the `check-env` CLI subcommand.
pub fn check_env(
    cfg: &Config,
    privilege: PrivilegeLevel,
    config_path: Option<&Path>,
) -> anyhow::Result<()> {
    println!("windbgr-mcp v{}", env!("CARGO_PKG_VERSION"));
    println!("privilege: {} ({})", privilege, privilege.description());
    if let Some(p) = config_path {
        println!("config: {}", p.display());
    } else {
        println!("config: <defaults>");
    }
    match cfg.cdb_path() {
        Ok(p) => println!("cdb: {}", p.display()),
        Err(e) => println!("cdb: NOT FOUND ({e})"),
    }
    println!(
        "bind: {}  max_sessions: {}  session_idle_timeout_secs: {}",
        cfg.server.bind, cfg.server.max_sessions, cfg.server.session_idle_timeout_secs
    );
    let hosts = cfg.server.resolved_allowed_hosts();
    if hosts.is_empty() {
        println!("allowed_hosts: <empty> (DNS rebinding protection DISABLED)");
    } else {
        println!("allowed_hosts: {}", hosts.join(", "));
    }
    println!(
        "auth_bearer_token: {}",
        if cfg.resolved_token().is_some() {
            "set"
        } else {
            "not set"
        }
    );
    println!(
        "symbol_path: {}",
        cfg.debugger
            .symbol_path
            .as_deref()
            .unwrap_or("<not configured>")
    );
    println!(
        "attach_timeout_ms: {}  launch_timeout_ms: {}  command_timeout_ms: {}  output_ring_bytes: {}",
        cfg.debugger.attach_timeout_ms,
        cfg.debugger.launch_timeout_ms,
        cfg.debugger.command_timeout_ms,
        cfg.debugger.output_ring_bytes
    );
    if let Some(p) = &cfg.audit.jsonl_path {
        println!("audit_log: {}", p.display());
    } else {
        println!("audit_log: <stdout only>");
    }
    Ok(())
}
