//! Process enumeration via `CreateToolhelp32Snapshot` and
//! `QueryFullProcessImageNameW`.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::{GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::error::{Error, Result};
use crate::platform::windows::handle::{wide_str_to_string, HandleGuard};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub image_name: String,
    pub parent_pid: u32,
    pub threads: u32,
}

/// Enumerate all processes using `CreateToolhelp32Snapshot`.
pub fn enum_processes() -> Result<Vec<ProcessInfo>> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Err(Error::WindowsApi(format!(
                "CreateToolhelp32Snapshot(PROCESS) failed: {}",
                GetLastError()
            )));
        }
        let guard = HandleGuard(snap);
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut out = Vec::new();
        if Process32FirstW(guard.0, &mut entry) == 0 {
            return Ok(out);
        }
        loop {
            out.push(ProcessInfo {
                pid: entry.th32ProcessID,
                image_name: wide_str_to_string(&entry.szExeFile),
                parent_pid: entry.th32ParentProcessID,
                threads: entry.cntThreads,
            });
            if Process32NextW(guard.0, &mut entry) == 0 {
                break;
            }
        }
        Ok(out)
    }
}

/// Return the full path of the executable image for a given PID, using
/// `QueryFullProcessImageNameW`. Returns `None` when access is denied.
pub fn process_image_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let guard = HandleGuard(h);
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        if QueryFullProcessImageNameW(
            guard.0,
            PROCESS_NAME_FORMAT::default(),
            buf.as_mut_ptr(),
            &mut size,
        ) == 0
        {
            return None;
        }
        buf.truncate(size as usize);
        Some(PathBuf::from(OsString::from_wide(&buf)))
    }
}
