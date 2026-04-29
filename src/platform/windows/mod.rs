//! Windows FFI layer. All `unsafe` calls to `windows-sys` are confined to
//! these submodules so that the rest of the crate can work with safe Rust
//! types (`String`, `PathBuf`, iterators, errors) and a single result type.

#![cfg(windows)]

mod handle;
pub mod modules;
pub mod privilege;
pub mod process;
pub mod skip;

pub use modules::{enum_modules, enum_modules_with_skip, ModuleInfo};
pub use privilege::enable_se_debug_privilege;
pub use process::{enum_processes, process_image_path, ProcessInfo};
pub use skip::SkipReason;
