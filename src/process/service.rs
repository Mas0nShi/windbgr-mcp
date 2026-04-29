//! Top-level discovery pipeline.

use crate::error::Result;
use crate::process::matcher::ModuleMatcher;
use crate::process::model::{
    FindResult, ModuleMatch, ProcessListResult, ProcessMatch, ProcessSummary, SkippedProcess,
};

#[cfg(windows)]
use crate::platform::windows as winapi;

/// Pseudo-PIDs always skipped by the discovery service.
///
/// `0` is the System Idle process; `4` is the kernel `System` process.
/// Neither has user-mode modules to enumerate.
const SYSTEM_IDLE_PID: u32 = 0;
const SYSTEM_KERNEL_PID: u32 = 4;

/// Return all processes that load any of the provided module patterns.
#[cfg(windows)]
pub fn find_processes_by_module(patterns: &[String]) -> Result<FindResult> {
    let processes = winapi::enum_processes()?;
    let total = processes.len();
    let matcher = ModuleMatcher::from_raw(patterns);

    if matcher.is_empty() {
        return Ok(FindResult {
            matches: Vec::new(),
            total_processes: total,
            skipped_processes: Vec::new(),
        });
    }

    let mut matches = Vec::new();
    let mut skipped = Vec::new();

    for proc in processes {
        if proc.pid == SYSTEM_IDLE_PID || proc.pid == SYSTEM_KERNEL_PID {
            continue;
        }
        let modules = match winapi::enum_modules_with_skip(proc.pid) {
            Ok(m) => m,
            Err(reason) => {
                skipped.push(SkippedProcess {
                    pid: proc.pid,
                    image_name: proc.image_name.clone(),
                    reason: reason.to_string(),
                    kind: reason.kind().to_string(),
                });
                continue;
            }
        };

        let mut matched = Vec::new();
        for m in &modules {
            let name_lc = m.name.to_lowercase();
            let path_lc = m.path.to_string_lossy().to_lowercase();
            if let Some(pat) = matcher.first_match(&name_lc, &path_lc) {
                matched.push(ModuleMatch {
                    name: m.name.clone(),
                    path: m.path.clone(),
                    base: m.base,
                    size: m.size,
                    pattern: pat.original.clone(),
                });
            }
        }

        if matched.is_empty() {
            continue;
        }
        matches.push(ProcessMatch {
            pid: proc.pid,
            parent_pid: proc.parent_pid,
            image_name: proc.image_name.clone(),
            image_path: winapi::process_image_path(proc.pid),
            threads: proc.threads,
            matched_modules: matched,
            enumeration_status: "ok".into(),
        });
    }

    Ok(FindResult {
        matches,
        total_processes: total,
        skipped_processes: skipped,
    })
}

#[cfg(not(windows))]
pub fn find_processes_by_module(_patterns: &[String]) -> Result<FindResult> {
    Err(crate::error::Error::Other(
        "process discovery is only implemented on Windows".into(),
    ))
}

/// Lightweight process listing for debugging / tests.
#[cfg(windows)]
pub fn list_processes() -> Result<Vec<ProcessSummary>> {
    let processes = winapi::enum_processes()?;
    Ok(processes
        .into_iter()
        .map(|p| ProcessSummary {
            pid: p.pid,
            image_name: p.image_name,
            parent_pid: p.parent_pid,
            threads: p.threads,
        })
        .collect())
}

#[cfg(not(windows))]
pub fn list_processes() -> Result<Vec<ProcessSummary>> {
    Err(crate::error::Error::Other(
        "process listing is only implemented on Windows".into(),
    ))
}

/// Convenience wrapper around [`list_processes`] used by the
/// `process_list` MCP tool. Returns the snapshot together with
/// `total_processes` so the response payload is self-describing
/// without requiring callers to count entries client-side.
pub fn list_processes_result() -> Result<ProcessListResult> {
    let processes = list_processes()?;
    let total = processes.len();
    Ok(ProcessListResult {
        processes,
        total_processes: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn list_processes_returns_non_empty() {
        let list = list_processes().expect("enum_processes should succeed");
        assert!(
            !list.is_empty(),
            "at least System + current process should exist"
        );
        assert!(
            list.iter().any(|p| p.pid == std::process::id()),
            "current pid must be present"
        );
    }

    #[test]
    #[cfg(windows)]
    fn find_self_by_ntdll() {
        let res = find_processes_by_module(&["ntdll.dll".into()]).unwrap();
        assert!(
            res.matches.iter().any(|m| m.pid == std::process::id()),
            "current process should match ntdll.dll"
        );
    }
}
