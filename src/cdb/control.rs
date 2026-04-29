//! Windows process-group break control for a spawned cdb.exe child.
//!
//! cdb requires a CTRL+C / CTRL+BREAK style break to interrupt a running
//! target. `tokio::process::Command` by default spawns the child in the same
//! console and process group as the parent; writing CTRL characters to stdin
//! is not equivalent.
//!
//! We spawn cdb with `CREATE_NEW_PROCESS_GROUP` and invoke
//! `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, group_pid)` to trigger a break.
//! Because the parent is also in the same console, cdb needs `CTRL_BREAK`
//! (not `CTRL_C`) — `CTRL_BREAK` cannot be disabled by `SetConsoleCtrlHandler`
//! and is always delivered to members of the process group.

#[cfg(windows)]
pub use win::{generate_ctrl_break, CREATE_NEW_PROCESS_GROUP};

#[cfg(not(windows))]
pub fn generate_ctrl_break(_pid: u32) -> crate::error::Result<()> {
    Err(crate::error::Error::Other(
        "break control is only implemented on Windows".into(),
    ))
}

#[cfg(not(windows))]
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0;

#[cfg(windows)]
mod win {
    use crate::error::{Error, Result};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
    pub use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

    pub fn generate_ctrl_break(pid: u32) -> Result<()> {
        unsafe {
            if GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) == 0 {
                return Err(Error::WindowsApi(format!(
                    "GenerateConsoleCtrlEvent(CTRL_BREAK, pid={pid}) failed: {}",
                    GetLastError()
                )));
            }
        }
        Ok(())
    }
}
