//! RAII handle guard plus the wide-string helpers shared across Win32 calls.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};

pub(crate) struct HandleGuard(pub(crate) HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

pub(crate) fn wide_str_to_string(slice: &[u16]) -> String {
    OsString::from_wide(strip_nul(slice))
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn strip_nul(slice: &[u16]) -> &[u16] {
    let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    &slice[..end]
}
