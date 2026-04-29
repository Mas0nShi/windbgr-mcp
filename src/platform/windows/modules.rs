//! Module enumeration. Tries the toolhelp snapshot first and falls back to
//! `EnumProcessModulesEx` + `GetModuleFileNameExW`.
//!
//! Failure cases are normalised by inspecting the underlying Win32 error
//! code (in particular `ERROR_ACCESS_DENIED`), not by parsing error
//! messages.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_ACCESS_DENIED, HMODULE, INVALID_HANDLE_VALUE, MAX_PATH,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32,
};
use windows_sys::Win32::System::ProcessStatus::{
    EnumProcessModulesEx, GetModuleFileNameExW, LIST_MODULES_ALL,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};

use crate::error::{Error, Result};
use crate::platform::windows::handle::{strip_nul, wide_str_to_string, HandleGuard};
use crate::platform::windows::skip::SkipReason;

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub path: PathBuf,
    pub base: usize,
    pub size: usize,
}

/// Enumerate modules loaded by a given PID.
///
/// Returns `Err(Error::WindowsApi)` for callers that don't need the
/// structured skip reason. Prefer [`enum_modules_with_skip`] in production.
pub fn enum_modules(pid: u32) -> Result<Vec<ModuleInfo>> {
    enum_modules_with_skip(pid).map_err(|s| Error::WindowsApi(s.to_string()))
}

/// Same as [`enum_modules`] but returns a structured [`SkipReason`] when the
/// process cannot be inspected.
pub fn enum_modules_with_skip(pid: u32) -> std::result::Result<Vec<ModuleInfo>, SkipReason> {
    match enum_modules_toolhelp(pid) {
        Ok(list) => Ok(list),
        Err(EnumOutcome::AccessDenied) => Err(SkipReason::AccessDenied),
        Err(EnumOutcome::Failed(first)) => match enum_modules_psapi(pid) {
            Ok(list) => Ok(list),
            Err(EnumOutcome::AccessDenied) => Err(SkipReason::AccessDenied),
            Err(EnumOutcome::Failed(second)) => Err(SkipReason::Other(format!(
                "toolhelp={first}; psapi={second}"
            ))),
        },
    }
}

enum EnumOutcome {
    AccessDenied,
    Failed(String),
}

fn enum_modules_toolhelp(pid: u32) -> std::result::Result<Vec<ModuleInfo>, EnumOutcome> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        if snap == INVALID_HANDLE_VALUE {
            let code = GetLastError();
            return Err(classify_error(
                code,
                format!("CreateToolhelp32Snapshot(MODULE, {pid}) failed: {code}"),
            ));
        }
        let guard = HandleGuard(snap);
        let mut entry: MODULEENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        let mut out = Vec::new();
        if Module32FirstW(guard.0, &mut entry) == 0 {
            return Ok(out);
        }
        loop {
            out.push(ModuleInfo {
                name: wide_str_to_string(&entry.szModule),
                path: PathBuf::from(OsString::from_wide(strip_nul(&entry.szExePath))),
                base: entry.modBaseAddr as usize,
                size: entry.modBaseSize as usize,
            });
            if Module32NextW(guard.0, &mut entry) == 0 {
                break;
            }
        }
        Ok(out)
    }
}

fn enum_modules_psapi(pid: u32) -> std::result::Result<Vec<ModuleInfo>, EnumOutcome> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if h.is_null() {
            let code = GetLastError();
            return Err(classify_error(
                code,
                format!("OpenProcess(pid={pid}) failed: {code}"),
            ));
        }
        let guard = HandleGuard(h);

        let mut needed: u32 = 0;
        let mut mods: Vec<HMODULE> = vec![ptr::null_mut(); 1024];
        loop {
            let size = (mods.len() * std::mem::size_of::<HMODULE>()) as u32;
            if EnumProcessModulesEx(
                guard.0,
                mods.as_mut_ptr(),
                size,
                &mut needed,
                LIST_MODULES_ALL,
            ) == 0
            {
                let code = GetLastError();
                return Err(classify_error(
                    code,
                    format!("EnumProcessModulesEx failed: {code}"),
                ));
            }
            if needed as usize <= size as usize {
                mods.truncate(needed as usize / std::mem::size_of::<HMODULE>());
                break;
            }
            mods.resize(
                needed as usize / std::mem::size_of::<HMODULE>(),
                ptr::null_mut(),
            );
        }

        let mut out = Vec::with_capacity(mods.len());
        for m in mods {
            let mut buf = vec![0u16; MAX_PATH as usize * 2];
            let len = GetModuleFileNameExW(guard.0, m, buf.as_mut_ptr(), buf.len() as u32);
            if len == 0 {
                continue;
            }
            buf.truncate(len as usize);
            let path = PathBuf::from(OsString::from_wide(&buf));
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(ModuleInfo {
                name,
                path,
                base: m as usize,
                size: 0,
            });
        }
        Ok(out)
    }
}

fn classify_error(code: u32, msg: String) -> EnumOutcome {
    if code == ERROR_ACCESS_DENIED {
        EnumOutcome::AccessDenied
    } else {
        EnumOutcome::Failed(msg)
    }
}
