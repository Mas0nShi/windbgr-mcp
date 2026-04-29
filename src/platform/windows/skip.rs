//! Reasons a particular PID could not be inspected by the discovery service.
//!
//! There is intentionally no separate "protected" classification. Kernel
//! minimal processes (Registry, System) and PPL services (csrss.exe,
//! lsass.exe, ...) all surface as [`SkipReason::AccessDenied`] when the
//! `OpenProcess` call fails, which is the same actionable signal a normal
//! permission failure would produce.

#[derive(Debug, Clone)]
pub enum SkipReason {
    /// `OpenProcess` (or any subsequent enumeration call) returned
    /// `ERROR_ACCESS_DENIED`. Almost always means either the service is not
    /// running with `SeDebugPrivilege` enabled, or the target process is
    /// kernel-protected.
    AccessDenied,
    /// Other Win32 error returned by toolhelp / psapi.
    Other(String),
}

impl SkipReason {
    /// Stable `kind` token surfaced to MCP clients.
    pub fn kind(&self) -> &'static str {
        match self {
            SkipReason::AccessDenied => "access_denied",
            SkipReason::Other(_) => "enumeration_failed",
        }
    }
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::AccessDenied => write!(
                f,
                "access denied — kernel-protected target or missing SeDebugPrivilege"
            ),
            SkipReason::Other(s) => f.write_str(s),
        }
    }
}
