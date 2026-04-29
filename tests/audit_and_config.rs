//! Cross-platform unit-style integration tests for the audit log, config
//! parsing and session-limit enforcement.

use std::path::PathBuf;

use serde_json::json;
use windbgr_mcp::audit::{AuditEvent, AuditLog};
use windbgr_mcp::cdb::manager::SessionManager;
use windbgr_mcp::config::Config;
use windbgr_mcp::error::Error;

#[test]
fn audit_writes_jsonl_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.jsonl");
    let log = AuditLog::new(Some(&path)).expect("audit log");
    log.record(&AuditEvent {
        timestamp: "now".into(),
        tool: "process_find_by_module",
        status: "ok",
        session_id: None,
        pid: None,
        duration_ms: Some(7),
        params: json!({"modules": ["ntdll.dll"]}),
        error: None,
    });
    log.record(&AuditEvent {
        timestamp: "now".into(),
        tool: "debug_attach",
        status: "error",
        session_id: None,
        pid: Some(4),
        duration_ms: Some(11),
        params: json!({"pid": 4}),
        error: Some("nope".into()),
    });
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "expected two JSONL lines, got: {text}");
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["tool"], "process_find_by_module");
    assert_eq!(first["status"], "ok");
    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(second["status"], "error");
    assert_eq!(second["pid"], 4);
}

#[test]
fn config_loads_from_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.toml");
    std::fs::write(
        &path,
        r#"
[server]
bind = "0.0.0.0:9000"
max_sessions = 2

[auth]
bearer_token = "secret"

[debugger]
cdb_path = "C:/cdb/cdb.exe"
attach_timeout_ms = 12345
"#,
    )
    .unwrap();
    let cfg = Config::load(Some(&path)).expect("load");
    assert_eq!(cfg.server.bind, "0.0.0.0:9000");
    assert_eq!(cfg.server.max_sessions, 2);
    assert_eq!(cfg.resolved_token().as_deref(), Some("secret"));
    assert_eq!(cfg.debugger.attach_timeout_ms, 12345);
    assert_eq!(
        cfg.debugger.cdb_path.as_deref(),
        Some(PathBuf::from("C:/cdb/cdb.exe").as_path())
    );
}

#[test]
fn list_active_is_empty_for_fresh_manager() {
    let mut cfg = Config::default();
    cfg.debugger.cdb_path = Some(PathBuf::from("does-not-exist.exe"));
    let mgr = SessionManager::new(&cfg).expect("manager");
    let pruned = mgr.prune_inactive();
    assert_eq!(pruned, 0, "no sessions to prune in a fresh manager");
    let sessions = mgr.list_active();
    assert!(
        sessions.is_empty(),
        "fresh manager should have no active sessions"
    );
}

#[test]
fn session_limit_check_runs_without_cdb_when_zero_sessions() {
    // Build a config with a non-existing cdb path; SessionManager::new only
    // requires the path to be set, not to exist.
    let mut cfg = Config::default();
    cfg.server.max_sessions = 0;
    cfg.debugger.cdb_path = Some(PathBuf::from("does-not-exist.exe"));
    let mgr = SessionManager::new(&cfg).expect("manager");
    // Trying to attach with limit=0 should immediately yield SessionLimit.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt
        .block_on(async {
            mgr.attach(windbgr_mcp::cdb::session::AttachOptions {
                pid: 0,
                noninvasive: false,
                initial_break: true,
                symbol_path: None,
                extra_args: vec![],
            })
            .await
        })
        .expect_err("should hit session limit");
    assert!(matches!(err, Error::SessionLimit(0)), "got: {err:?}");
}
