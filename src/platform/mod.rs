//! Platform-specific FFI wrappers. The rest of the crate works with safe
//! Rust types and the `Result` defined in [`crate::error`].

#[cfg(windows)]
pub mod windows;
