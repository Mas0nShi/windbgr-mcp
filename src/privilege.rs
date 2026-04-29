//! Runtime privilege-level detection.
//!
//! Windows processes running non-elevated can only debug processes owned by the
//! same user session.  Elevated Administrator processes with `SeDebugPrivilege`
//! can additionally debug system services, other users' processes, etc.
//!
//! The privilege level is detected once at startup and never changes during the
//! lifetime of the server — the LLM cannot elevate; only the human user can
//! restart the server from an elevated prompt.

use serde::{Deserialize, Serialize};

/// Describes what the server process is allowed to debug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegeLevel {
    /// Normal (non-elevated) user.  Can debug processes owned by the same
    /// user in the same session.
    User,
    /// Elevated Administrator with `SeDebugPrivilege`.  Can debug nearly any
    /// user-mode process including system services (`svchost.exe`, etc.).
    Admin,
}

impl PrivilegeLevel {
    pub fn is_admin(self) -> bool {
        self == PrivilegeLevel::Admin
    }

    pub fn description(self) -> &'static str {
        match self {
            PrivilegeLevel::User => {
                "normal user \u{2014} can debug own processes only"
            }
            PrivilegeLevel::Admin => {
                "elevated administrator \u{2014} can debug services and other users\u{2019} processes"
            }
        }
    }
}

impl std::fmt::Display for PrivilegeLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrivilegeLevel::User => f.write_str("user"),
            PrivilegeLevel::Admin => f.write_str("admin"),
        }
    }
}

/// Detect the current privilege level by attempting to enable
/// `SeDebugPrivilege`.  Returns [`PrivilegeLevel::Admin`] only when the
/// privilege was successfully enabled.
#[cfg(windows)]
pub fn detect_privilege() -> PrivilegeLevel {
    match crate::platform::windows::enable_se_debug_privilege() {
        Ok(true) => PrivilegeLevel::Admin,
        _ => PrivilegeLevel::User,
    }
}

#[cfg(not(windows))]
pub fn detect_privilege() -> PrivilegeLevel {
    PrivilegeLevel::User
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_user() {
        assert_eq!(PrivilegeLevel::User.to_string(), "user");
    }

    #[test]
    fn display_admin() {
        assert_eq!(PrivilegeLevel::Admin.to_string(), "admin");
    }

    #[test]
    fn is_admin() {
        assert!(!PrivilegeLevel::User.is_admin());
        assert!(PrivilegeLevel::Admin.is_admin());
    }

    #[test]
    fn description_not_empty() {
        assert!(!PrivilegeLevel::User.description().is_empty());
        assert!(!PrivilegeLevel::Admin.description().is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let json = serde_json::to_string(&PrivilegeLevel::Admin).unwrap();
        assert_eq!(json, "\"admin\"");
        let back: PrivilegeLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(back, PrivilegeLevel::Admin);

        let json = serde_json::to_string(&PrivilegeLevel::User).unwrap();
        assert_eq!(json, "\"user\"");
        let back: PrivilegeLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(back, PrivilegeLevel::User);
    }
}
