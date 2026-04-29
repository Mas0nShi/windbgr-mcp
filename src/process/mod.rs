//! Process-and-module discovery for the `process_find_by_module` MCP tool.
//!
//! The Windows-specific enumeration lives under [`crate::platform::windows`].
//! This module is responsible for the language-level pipeline: normalise
//! request patterns, walk processes, and turn raw module data into a
//! structured response.

pub mod matcher;
pub mod model;
pub mod service;

pub use matcher::{ModuleMatcher, ModulePattern};
pub use model::{
    FindResult, ModuleMatch, ProcessListResult, ProcessMatch, ProcessSummary, SkippedProcess,
};
pub use service::{find_processes_by_module, list_processes, list_processes_result};
