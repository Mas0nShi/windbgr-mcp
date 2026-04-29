//! Single source of truth for the MCP tools exposed by this server.
//!
//! Both `list_tools` (over the wire) and the `dispatch` switch in
//! [`super::server`] consult [`ToolName`], so adding or renaming a tool
//! requires touching exactly one place. The actual `rmcp::Tool`
//! definitions are lazily built once per `WindbgrMcp` instance.

use rmcp::handler::server::tool::schema_for_type;
use rmcp::model::{Tool, ToolAnnotations};

use crate::mcp::tools::{
    DebugAttachArgs, DebugCommandArgs, DebugControlArgs, DebugLaunchArgs, DebugListSessionsArgs,
    DebugOutputArgs, DebugStatusArgs, DebugStopArgs, DebugWaitBreakArgs, ProcessFindByModuleArgs,
    ProcessListArgs,
};

/// Stable wire-level tool names. Use these constants instead of bare
/// strings so a typo turns into a compile error.
pub mod name {
    pub const PROCESS_FIND_BY_MODULE: &str = "process_find_by_module";
    pub const PROCESS_LIST: &str = "process_list";
    pub const DEBUG_ATTACH: &str = "debug_attach";
    pub const DEBUG_LAUNCH: &str = "debug_launch";
    pub const DEBUG_COMMAND: &str = "debug_command";
    pub const DEBUG_CONTROL: &str = "debug_control";
    pub const DEBUG_OUTPUT: &str = "debug_output";
    pub const DEBUG_STATUS: &str = "debug_status";
    pub const DEBUG_LIST_SESSIONS: &str = "debug_list_sessions";
    pub const DEBUG_WAIT_BREAK: &str = "debug_wait_break";
    pub const DEBUG_STOP: &str = "debug_stop";
}

/// Build the static tool catalog. Called once at startup; the result is
/// cached by `WindbgrMcp`.
pub fn build_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            name::PROCESS_FIND_BY_MODULE,
            "Enumerate processes and modules via Windows APIs and return \
             matches whose loaded modules contain any of the requested name \
             patterns (case-insensitive substring match on module name or path).",
            schema_for_type::<ProcessFindByModuleArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(true).destructive(false)),
        Tool::new(
            name::PROCESS_LIST,
            "Enumerate every running Windows process via toolhelp32 and \
             return `pid`, `image_name`, `parent_pid`, and thread count. \
             Lighter weight than `process_find_by_module` because it does \
             not open each process to inspect modules; use this tool for \
             fast `pid` discovery before calling `debug_attach`.",
            schema_for_type::<ProcessListArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(true).destructive(false)),
        Tool::new(
            name::DEBUG_ATTACH,
            "Attach cdb.exe to an existing Windows process by PID and \
             create a new debugging session. Returns a `session_id` to use \
             with the other `debug_*` tools.",
            schema_for_type::<DebugAttachArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(false).destructive(true)),
        Tool::new(
            name::DEBUG_LAUNCH,
            "Launch a new process under cdb.exe and create a new debugging \
             session. Use for spawning Windows executables with full debug \
             control. Returns a `session_id`.",
            schema_for_type::<DebugLaunchArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(false).destructive(true)),
        Tool::new(
            name::DEBUG_COMMAND,
            "Send a cdb command to a session whose target is stopped at a \
             debugger prompt. Collects output until the next prompt or until \
             `timeout_ms` elapses. Do not use while the session is Running; \
             use `debug_control` with `break` first.",
            schema_for_type::<DebugCommandArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(false).destructive(true)),
        Tool::new(
            name::DEBUG_CONTROL,
            "Control a debugging session: `continue` resumes target \
             execution (equivalent to `g`); `break` and `interrupt_command` \
             fire CTRL+BREAK at the cdb process group to stop target/command.",
            schema_for_type::<DebugControlArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(false).destructive(true)),
        Tool::new(
            name::DEBUG_OUTPUT,
            "Read buffered cdb stdout/stderr for a session starting at \
             `since_offset`. Use this for paginating long command output.",
            schema_for_type::<DebugOutputArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(true).destructive(false)),
        Tool::new(
            name::DEBUG_STATUS,
            "Return the current state of a debugging session.",
            schema_for_type::<DebugStatusArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(true).destructive(false)),
        Tool::new(
            name::DEBUG_LIST_SESSIONS,
            "List every currently active debugging session managed by this \
             server. Sessions in `Stopped` or `Failed` state are pruned \
             before the list is returned, so only ids that can still \
             accept `debug_*` calls are reported. No automatic idle \
             timeout: use `debug_stop` to release a session.",
            schema_for_type::<DebugListSessionsArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(true).destructive(false)),
        Tool::new(
            name::DEBUG_WAIT_BREAK,
            "Block until the session returns to the debugger prompt \
             (breakpoint hit, exception, CTRL+BREAK or target exit). \
             Use after `debug_control(continue)` to wait for a breakpoint \
             to fire before issuing further `debug_command` calls.",
            schema_for_type::<DebugWaitBreakArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(true).destructive(false)),
        Tool::new(
            name::DEBUG_STOP,
            "Stop a debugging session. `detach` sends `qd` (leaves target \
             running); `terminate_target` sends `q` (kills target); \
             `kill_debugger` only kills the cdb child as a last-resort \
             cleanup.",
            schema_for_type::<DebugStopArgs>(),
        )
        .annotate(ToolAnnotations::new().read_only(false).destructive(true)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn tool_names_are_unique() {
        let tools = build_tools();
        let names: HashSet<_> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names.len(), tools.len(), "duplicate tool name in registry");
    }

    #[test]
    fn registry_covers_all_known_names() {
        let tools = build_tools();
        let names: HashSet<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        for n in [
            name::PROCESS_FIND_BY_MODULE,
            name::PROCESS_LIST,
            name::DEBUG_ATTACH,
            name::DEBUG_LAUNCH,
            name::DEBUG_COMMAND,
            name::DEBUG_CONTROL,
            name::DEBUG_OUTPUT,
            name::DEBUG_STATUS,
            name::DEBUG_LIST_SESSIONS,
            name::DEBUG_WAIT_BREAK,
            name::DEBUG_STOP,
        ] {
            assert!(names.contains(n), "missing tool: {n}");
        }
    }
}
