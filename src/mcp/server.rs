//! MCP `ServerHandler` implementation.

use std::sync::Arc;
use std::time::Instant;

use rmcp::handler::server::tool::parse_json_object;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Implementation, InitializeResult, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool, ToolsCapability,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer};
use rmcp::ErrorData as McpError;
use rmcp::ServerHandler;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

use crate::audit::{AuditEvent, AuditLog};
use crate::cdb::manager::SessionManager;
use crate::cdb::session::{AttachOptions, LaunchOptions};
use crate::config::Config;
use crate::error::Error;
use crate::mcp::registry::{build_tools, name};
use crate::mcp::tools::{
    DebugAttachArgs, DebugCommandArgs, DebugControlArgs, DebugLaunchArgs, DebugListSessionsArgs,
    DebugOutputArgs, DebugStatusArgs, DebugStopArgs, DebugWaitBreakArgs, ProcessFindByModuleArgs,
    ProcessListArgs,
};
use crate::privilege::PrivilegeLevel;
use crate::process::{find_processes_by_module, list_processes_result};

#[derive(Clone)]
pub struct WindbgrMcp {
    inner: Arc<Inner>,
}

struct Inner {
    sessions: SessionManager,
    audit: AuditLog,
    server_name: String,
    server_version: String,
    command_timeout_ms: u64,
    privilege: PrivilegeLevel,
    tools: Arc<[Tool]>,
}

impl WindbgrMcp {
    pub fn new(cfg: &Config, privilege: PrivilegeLevel) -> crate::Result<Self> {
        let sessions = SessionManager::new(cfg)?;
        let audit = AuditLog::new(cfg.audit.jsonl_path.as_deref())?;
        let tools: Arc<[Tool]> = build_tools().into();
        Ok(Self {
            inner: Arc::new(Inner {
                sessions,
                audit,
                server_name: env!("CARGO_PKG_NAME").into(),
                server_version: env!("CARGO_PKG_VERSION").into(),
                command_timeout_ms: cfg.debugger.command_timeout_ms,
                privilege,
                tools,
            }),
        })
    }

    pub fn sessions(&self) -> &SessionManager {
        &self.inner.sessions
    }
}

impl ServerHandler for WindbgrMcp {
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_tools_with(ToolsCapability {
                list_changed: Some(false),
            })
            .build();
        let server_info = Implementation::new(
            self.inner.server_name.clone(),
            self.inner.server_version.clone(),
        )
        .with_title("windbgr-mcp")
        .with_description("MCP server for Windows process discovery and cdb debugging");
        InitializeResult::new(capabilities)
            .with_server_info(server_info)
            .with_instructions(
                "Use `process_find_by_module` to locate target PIDs, then \
                 `debug_attach` or `debug_launch` to create a cdb session. \
                 Use `debug_command` at a prompt, `debug_control` to \
                 continue/break, `debug_output` to paginate output, and \
                 `debug_stop` to end the session.",
            )
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.inner.tools.iter().find(|t| t.name == name).cloned()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.inner.tools.to_vec()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let started = Instant::now();
        let name = request.name.clone();
        let args_obj = request.arguments.clone().unwrap_or_default();
        let args_value = serde_json::Value::Object(args_obj.clone());

        let result = self.dispatch(&name, args_obj).await;

        let status = if result.is_ok() { "ok" } else { "error" };
        let err_text = result.as_ref().err().map(|e| e.to_string());
        self.inner.audit.record(&AuditEvent {
            timestamp: crate::audit::now_ts(),
            tool: &name,
            status,
            session_id: None,
            pid: None,
            duration_ms: Some(started.elapsed().as_millis()),
            params: args_value,
            error: err_text,
        });

        match result {
            Ok(v) => Ok(CallToolResult::structured(v)),
            Err(e) => Ok(CallToolResult::structured_error(json!({
                "error": e.to_string(),
            }))),
        }
    }
}

/// Decode a tool's typed arguments, mapping deserialization failures into
/// our crate `Error`. Centralised so each dispatch arm stays a one-liner.
fn parse_args<T: DeserializeOwned>(
    args: serde_json::Map<String, serde_json::Value>,
) -> Result<T, Error> {
    parse_json_object(args).map_err(|e| Error::other(format!("invalid arguments: {e:?}")))
}

fn to_value<T: Serialize>(v: &T) -> Result<serde_json::Value, Error> {
    serde_json::to_value(v).map_err(|e| Error::other(format!("serialize response: {e}")))
}

impl WindbgrMcp {
    async fn dispatch(
        &self,
        tool_name: &str,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> Result<serde_json::Value, Error> {
        match tool_name {
            name::PROCESS_FIND_BY_MODULE => {
                let args: ProcessFindByModuleArgs = parse_args(args)?;
                let res = find_processes_by_module(&args.modules)?;
                to_value(&res)
            }
            name::PROCESS_LIST => {
                let _args: ProcessListArgs = parse_args(args)?;
                let res = list_processes_result()?;
                to_value(&res)
            }
            name::DEBUG_ATTACH => {
                let args: DebugAttachArgs = parse_args(args)?;
                let opts = AttachOptions {
                    pid: args.pid,
                    noninvasive: args.noninvasive,
                    initial_break: args.initial_break,
                    symbol_path: args.symbol_path,
                    extra_args: args.extra_args,
                };
                let session = self
                    .inner
                    .sessions
                    .attach(opts)
                    .await
                    .map_err(|e| self.enrich_attach_error(e))?;
                to_value(&SessionResponse::from_session(&session))
            }
            name::DEBUG_LAUNCH => {
                let args: DebugLaunchArgs = parse_args(args)?;
                let opts = LaunchOptions {
                    executable: args.executable,
                    args: args.args,
                    cwd: args.cwd,
                    env: args.env,
                    debug_children: args.debug_children,
                    initial_break: args.initial_break,
                    symbol_path: args.symbol_path,
                    extra_args: args.extra_args,
                };
                let session = self.inner.sessions.launch(opts).await?;
                to_value(&SessionResponse::from_session(&session))
            }
            name::DEBUG_COMMAND => {
                let args: DebugCommandArgs = parse_args(args)?;
                let session = self.inner.sessions.get(&args.session_id)?;
                let timeout_ms = if args.timeout_ms == 0 {
                    self.inner.command_timeout_ms
                } else {
                    args.timeout_ms
                };
                let outcome = session.run_command(args.command, timeout_ms).await?;
                to_value(&outcome)
            }
            name::DEBUG_CONTROL => {
                let args: DebugControlArgs = parse_args(args)?;
                let session = self.inner.sessions.get(&args.session_id)?;
                let new_state = session.control(args.action.clone().into()).await?;
                Ok(json!({
                    "session_id": args.session_id,
                    "state": new_state,
                    "action": args.action,
                }))
            }
            name::DEBUG_OUTPUT => {
                let args: DebugOutputArgs = parse_args(args)?;
                let session = self.inner.sessions.get(&args.session_id)?;
                let page = session
                    .read_output(args.since_offset, args.max_bytes)
                    .await?;
                to_value(&page)
            }
            name::DEBUG_STATUS => {
                let args: DebugStatusArgs = parse_args(args)?;
                let session = self.inner.sessions.get(&args.session_id)?;
                let status = session.status().await?;
                to_value(&status)
            }
            name::DEBUG_LIST_SESSIONS => {
                let _args: DebugListSessionsArgs = parse_args(args)?;
                let sessions = self.inner.sessions.list_active();
                let total = sessions.len();
                Ok(json!({
                    "sessions": sessions,
                    "total_active": total,
                }))
            }
            name::DEBUG_WAIT_BREAK => {
                let args: DebugWaitBreakArgs = parse_args(args)?;
                let session = self.inner.sessions.get(&args.session_id)?;
                session
                    .wait_ready(std::time::Duration::from_millis(args.timeout_ms))
                    .await?;
                let status = session.status().await?;
                to_value(&status)
            }
            name::DEBUG_STOP => {
                let args: DebugStopArgs = parse_args(args)?;
                let session = self.inner.sessions.get(&args.session_id)?;
                let mode = args.mode.clone();
                session.stop(mode.clone().into()).await?;
                self.inner.sessions.remove(&args.session_id);
                Ok(json!({
                    "session_id": args.session_id,
                    "mode": mode,
                    "state": "stopped",
                }))
            }
            other => Err(Error::other(format!("unknown tool: {other}"))),
        }
    }
}

#[derive(Debug, Serialize)]
struct SessionResponse {
    session_id: String,
    kind: crate::cdb::session::SessionKind,
    state: crate::cdb::session::SessionState,
    target_pid: Option<u32>,
    cdb_pid: u32,
    started_at: String,
}

impl SessionResponse {
    fn from_session(s: &crate::cdb::session::Session) -> Self {
        Self {
            session_id: s.id.clone(),
            kind: s.kind,
            state: *s.shared.state.lock(),
            target_pid: s.target_pid,
            cdb_pid: s.cdb_pid,
            started_at: s.started_at.clone(),
        }
    }
}

impl WindbgrMcp {
    /// When running as a normal user, attach failures that look like
    /// privilege issues get an actionable hint appended so the LLM can
    /// relay the fix to the user.
    ///
    /// We classify by `Error` variant rather than by string matching on
    /// the rendered message — `WindowsApi` and `Timeout` are the two
    /// variants that reliably surface privilege-related failures from
    /// `attach`.
    fn enrich_attach_error(&self, err: Error) -> Error {
        if self.inner.privilege.is_admin() {
            return err;
        }
        let actionable = matches!(err, Error::WindowsApi(_) | Error::Timeout(_));
        if actionable {
            Error::other(format!(
                "{err}. This process likely requires elevated privileges. \
                 Re-launch windbgr-mcp from an elevated Administrator prompt."
            ))
        } else {
            err
        }
    }
}

// Allow the transport layers to handle incoming notifications silently.
impl WindbgrMcp {
    pub async fn on_custom_notification(
        &self,
        _: rmcp::model::CustomNotification,
        _: NotificationContext<RoleServer>,
    ) {
    }
}
