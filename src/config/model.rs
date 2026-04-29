//! Public shape of the configuration tree. Loading and finalisation lives
//! in [`super::resolver`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::defaults::{
    default_attach_timeout, default_bind, default_command_timeout, default_idle_secs,
    default_max_sessions, default_output_ring_bytes,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub debugger: DebuggerConfig,
    #[serde(default)]
    pub audit: AuditConfig,
}

impl Config {
    pub fn cdb_path(&self) -> crate::error::Result<&Path> {
        self.debugger.cdb_path.as_deref().ok_or_else(|| {
            crate::error::Error::CdbNotFound("no cdb path configured or autodetected".into())
        })
    }

    /// Resolve the bearer token from inline config or environment variable.
    pub fn resolved_token(&self) -> Option<String> {
        if let Some(t) = &self.auth.bearer_token {
            if !t.is_empty() {
                return Some(t.clone());
            }
        }
        if let Some(var) = &self.auth.bearer_token_env {
            if let Ok(v) = std::env::var(var) {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
    #[serde(default = "default_idle_secs")]
    pub session_idle_timeout_secs: u64,
    /// Hostnames (or `host:port` authorities) accepted in the inbound
    /// `Host` header for the Streamable HTTP transport. The MCP spec
    /// mandates DNS rebinding protection — `rmcp` defaults to
    /// `localhost / 127.0.0.1 / ::1` only, so remote LLM clients
    /// connecting via the host's LAN address are rejected with 403 unless
    /// their Host is allow-listed here.
    ///
    /// - `None` (default) → keep rmcp's loopback list and add the bind
    ///   host if it differs.
    /// - `Some([])` → disable DNS rebinding protection entirely (NOT
    ///   recommended outside trusted intranets).
    /// - `Some([...])` → use the provided list verbatim.
    #[serde(default)]
    pub allowed_hosts: Option<Vec<String>>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            max_sessions: default_max_sessions(),
            session_idle_timeout_secs: default_idle_secs(),
            allowed_hosts: None,
        }
    }
}

impl ServerConfig {
    /// Resolve the final `allowed_hosts` list to feed into the rmcp
    /// `StreamableHttpServerConfig`.
    pub fn resolved_allowed_hosts(&self) -> Vec<String> {
        use crate::config::defaults::{LOOPBACK_HOSTS, WILDCARD_BIND};
        if let Some(list) = &self.allowed_hosts {
            return list.clone();
        }
        let mut hosts: Vec<String> = LOOPBACK_HOSTS.iter().map(|s| (*s).to_string()).collect();
        if let Some((host, _port)) = self.bind.rsplit_once(':') {
            let host = host.trim_matches(|c| c == '[' || c == ']');
            let bind_host = host.to_string();
            if !bind_host.is_empty() && bind_host != WILDCARD_BIND && !hosts.contains(&bind_host) {
                hosts.push(bind_host);
            }
        }
        hosts
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Read bearer token from this environment variable (HTTP transport).
    #[serde(default)]
    pub bearer_token_env: Option<String>,
    /// Inline token. Prefer `bearer_token_env` for production.
    #[serde(default)]
    pub bearer_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggerConfig {
    #[serde(default)]
    pub cdb_path: Option<PathBuf>,
    #[serde(default)]
    pub symbol_path: Option<String>,
    #[serde(default = "default_attach_timeout")]
    pub attach_timeout_ms: u64,
    #[serde(default = "default_attach_timeout")]
    pub launch_timeout_ms: u64,
    #[serde(default = "default_command_timeout")]
    pub command_timeout_ms: u64,
    #[serde(default = "default_output_ring_bytes")]
    pub output_ring_bytes: usize,
}

impl Default for DebuggerConfig {
    fn default() -> Self {
        Self {
            cdb_path: None,
            symbol_path: None,
            attach_timeout_ms: default_attach_timeout(),
            launch_timeout_ms: default_attach_timeout(),
            command_timeout_ms: default_command_timeout(),
            output_ring_bytes: default_output_ring_bytes(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditConfig {
    #[serde(default)]
    pub jsonl_path: Option<PathBuf>,
}
