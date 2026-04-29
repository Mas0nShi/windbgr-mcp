//! Typed parameter and response structs for every MCP tool exposed by
//! `windbgr-mcp`. These types are both (de)serialized through the MCP
//! protocol and used to generate JSON schemas for the tool definitions.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cdb::session::{default_true, ControlAction, StopMode};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProcessFindByModuleArgs {
    /// One or more module patterns. Matching is case-insensitive and accepts
    /// either the module file name (e.g. `ntdll.dll`) or a substring of the
    /// full module path.
    pub modules: Vec<String>,
}

/// Arguments for `process_list`. Currently parameterless: the tool is a
/// thin wrapper around toolhelp32 enumeration and intentionally does not
/// expose filters, paging, or full-image-path lookup. If those become
/// required later, prefer adding optional fields here over creating a
/// new tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProcessListArgs {}

/// Arguments for `debug_list_sessions`. The tool returns only sessions
/// the manager still considers active (`Starting / Idle / Executing /
/// Running / Breaking`); historical / failed sessions belong in the
/// audit log, not in this view.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DebugListSessionsArgs {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugAttachArgs {
    pub pid: u32,
    #[serde(default)]
    pub noninvasive: bool,
    #[serde(default = "default_true")]
    pub initial_break: bool,
    #[serde(default)]
    pub symbol_path: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugLaunchArgs {
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default)]
    pub debug_children: bool,
    #[serde(default = "default_true")]
    pub initial_break: bool,
    #[serde(default)]
    pub symbol_path: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugCommandArgs {
    pub session_id: String,
    pub command: String,
    /// Maximum time to wait for a cdb prompt. `0` uses the server's
    /// `debugger.command_timeout_ms` setting.
    #[serde(default)]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DebugControlAction {
    Continue,
    Break,
    InterruptCommand,
}

impl From<DebugControlAction> for ControlAction {
    fn from(v: DebugControlAction) -> Self {
        match v {
            DebugControlAction::Continue => ControlAction::Continue,
            DebugControlAction::Break => ControlAction::Break,
            DebugControlAction::InterruptCommand => ControlAction::InterruptCommand,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugControlArgs {
    pub session_id: String,
    pub action: DebugControlAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DebugStopMode {
    Detach,
    TerminateTarget,
    KillDebugger,
}

impl From<DebugStopMode> for StopMode {
    fn from(v: DebugStopMode) -> Self {
        match v {
            DebugStopMode::Detach => StopMode::Detach,
            DebugStopMode::TerminateTarget => StopMode::TerminateTarget,
            DebugStopMode::KillDebugger => StopMode::KillDebugger,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugStopArgs {
    pub session_id: String,
    pub mode: DebugStopMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugStatusArgs {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugWaitBreakArgs {
    pub session_id: String,
    /// Maximum time to wait for the next break (breakpoint hit, exception,
    /// CTRL+BREAK, target exit). Defaults to 60 000 ms.
    #[serde(default = "default_wait_timeout")]
    pub timeout_ms: u64,
}

fn default_wait_timeout() -> u64 {
    60_000
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugOutputArgs {
    pub session_id: String,
    #[serde(default)]
    pub since_offset: u64,
    #[serde(default = "default_output_bytes")]
    pub max_bytes: usize,
}

fn default_output_bytes() -> usize {
    16 * 1024
}
