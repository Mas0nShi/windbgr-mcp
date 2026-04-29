//! `SeDebugPrivilege` adjustment for the current process token.

use std::ptr;

use windows_sys::Win32::Foundation::{GetLastError, HANDLE, LUID};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::error::{Error, Result};
use crate::platform::windows::handle::HandleGuard;

/// `AdjustTokenPrivileges` returns success even when the privilege was not
/// actually held — `GetLastError` reports `ERROR_NOT_ALL_ASSIGNED` in that
/// case. The constant is not re-exported by `windows-sys`.
const ERROR_NOT_ALL_ASSIGNED: u32 = 1300;

/// Enable `SeDebugPrivilege` on the current process token.
///
/// Returns `Ok(true)` when the privilege was successfully enabled,
/// `Ok(false)` when the token did not hold the privilege (e.g. running
/// non-elevated), and `Err` on Win32 failures. The caller normally
/// best-effort-logs the result.
pub fn enable_se_debug_privilege() -> Result<bool> {
    unsafe {
        let proc = GetCurrentProcess();
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(proc, TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) == 0 {
            return Err(Error::WindowsApi(format!(
                "OpenProcessToken failed: {}",
                GetLastError()
            )));
        }
        let _guard = HandleGuard(token);

        let name: Vec<u16> = "SeDebugPrivilege\0".encode_utf16().collect();
        let mut luid: LUID = std::mem::zeroed();
        if LookupPrivilegeValueW(ptr::null(), name.as_ptr(), &mut luid) == 0 {
            return Err(Error::WindowsApi(format!(
                "LookupPrivilegeValueW(SeDebugPrivilege) failed: {}",
                GetLastError()
            )));
        }
        let mut tp: TOKEN_PRIVILEGES = std::mem::zeroed();
        tp.PrivilegeCount = 1;
        tp.Privileges[0] = LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        };
        if AdjustTokenPrivileges(token, 0, &tp, 0, ptr::null_mut(), ptr::null_mut()) == 0 {
            return Err(Error::WindowsApi(format!(
                "AdjustTokenPrivileges failed: {}",
                GetLastError()
            )));
        }
        let last = GetLastError();
        Ok(last != ERROR_NOT_ALL_ASSIGNED)
    }
}
