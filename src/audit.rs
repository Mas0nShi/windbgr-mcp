//! Structured JSONL audit logging for MCP tool invocations.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct AuditLog {
    inner: Arc<AuditInner>,
}

struct AuditInner {
    path: Option<PathBuf>,
    writer: Mutex<Option<std::fs::File>>,
}

impl std::fmt::Debug for AuditInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditInner")
            .field("path", &self.path)
            .finish()
    }
}

impl AuditLog {
    pub fn new(path: Option<&Path>) -> Result<Self> {
        let file = match path {
            Some(p) => Some(open_append(p)?),
            None => None,
        };
        Ok(Self {
            inner: Arc::new(AuditInner {
                path: path.map(PathBuf::from),
                writer: Mutex::new(file),
            }),
        })
    }

    pub fn record<T: Serialize>(&self, event: &T) {
        let line = match serde_json::to_string(event) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "failed to serialize audit event");
                return;
            }
        };
        tracing::info!(target: "windbgr_mcp::audit", "{}", line);
        if let Some(file) = self.inner.writer.lock().as_mut() {
            use std::io::Write;
            if let Err(e) = writeln!(file, "{line}") {
                tracing::warn!(error = %e, "failed to write audit event");
                return;
            }
            if let Err(e) = file.flush() {
                tracing::warn!(error = %e, "failed to flush audit log");
            }
        }
    }
}

fn open_append(path: &Path) -> Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?)
}

#[derive(Debug, Serialize)]
pub struct AuditEvent<'a> {
    pub timestamp: String,
    pub tool: &'a str,
    pub status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u128>,
    pub params: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn now_ts() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".into())
}
