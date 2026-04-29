//! Default values and well-known constants for [`super::Config`].
//!
//! Centralised so all "the default cdb path is X" / "the default bind is
//! Y" / "the default loopback set is …" knowledge lives in one place.

use std::path::PathBuf;

/// Locations probed when no explicit `cdb_path` is given. The list mirrors
/// the layout of an installed Windows Debugging Tools package (Windows 10
/// SDK / Windows 11 SDK).
pub(crate) const CDB_CANDIDATES: &[&str] = &[
    r"C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe",
    r"C:\Program Files\Windows Kits\10\Debuggers\x64\cdb.exe",
    r"C:\Program Files (x86)\Windows Kits\10\Debuggers\x86\cdb.exe",
];

/// Environment variable consulted before scanning [`CDB_CANDIDATES`].
pub(crate) const CDB_ENV_VAR: &str = "WINDBG_CDB_PATH";

/// Loopback hosts always permitted by the Streamable HTTP transport so the
/// MCP spec's DNS-rebinding protection is satisfied.
pub(crate) const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// Wildcard bind address — has no specific host to add to the allow list.
pub(crate) const WILDCARD_BIND: &str = "0.0.0.0";

pub(crate) fn default_bind() -> String {
    "127.0.0.1:8765".into()
}

pub(crate) fn default_max_sessions() -> usize {
    4
}

pub(crate) fn default_idle_secs() -> u64 {
    900
}

pub(crate) fn default_attach_timeout() -> u64 {
    15_000
}

pub(crate) fn default_command_timeout() -> u64 {
    30_000
}

pub(crate) fn default_output_ring_bytes() -> usize {
    512 * 1024
}

/// Best-effort `cdb.exe` discovery: TOML field → env var → known paths →
/// `PATH`. Returned `Some(_)` is guaranteed to point at an existing file.
pub fn detect_cdb() -> Option<PathBuf> {
    for p in CDB_CANDIDATES {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for entry in std::env::split_paths(&path) {
            let candidate = entry.join("cdb.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}
