//! Response DTOs for the process-discovery service.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessMatch {
    pub pid: u32,
    pub parent_pid: u32,
    pub image_name: String,
    pub image_path: Option<PathBuf>,
    pub threads: u32,
    pub matched_modules: Vec<ModuleMatch>,
    /// One of: `ok`, `partial`. Failure cases live in
    /// [`FindResult::skipped_processes`] instead.
    pub enumeration_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMatch {
    pub name: String,
    pub path: PathBuf,
    pub base: usize,
    pub size: usize,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindResult {
    pub matches: Vec<ProcessMatch>,
    pub total_processes: usize,
    pub skipped_processes: Vec<SkippedProcess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedProcess {
    pub pid: u32,
    pub image_name: String,
    pub reason: String,
    /// One of: `access_denied`, `enumeration_failed`. There is no separate
    /// "protected" classification — kernel minimal processes and PPL
    /// services surface as `access_denied` like any other permission
    /// failure.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSummary {
    pub pid: u32,
    pub image_name: String,
    pub parent_pid: u32,
    pub threads: u32,
}

/// Response wrapper for `process_list`. Carries the snapshot together
/// with its size so MCP clients can detect truncation or pagination
/// changes if the contract evolves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessListResult {
    pub processes: Vec<ProcessSummary>,
    pub total_processes: usize,
}
