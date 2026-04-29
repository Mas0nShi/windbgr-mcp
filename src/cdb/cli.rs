//! cdb command-line argv assembly.
//!
//! Centralises every cdb switch we ever issue (`-p`, `-y`, `-pv`, `-g`,
//! `-o`) so that the session actor and integration tests don't have to
//! agree on the spelling separately. Also centralises the in-band REPL
//! commands (`g`, `qd`, `q`).

use std::path::Path;

use crate::error::{Error, Result};

/// Argv builder for an attach session (`cdb -p <pid>`).
pub fn attach_argv(
    pid: u32,
    noninvasive: bool,
    initial_break: bool,
    symbol_path: Option<&str>,
    extra_args: &[String],
) -> Vec<String> {
    let mut args = Vec::new();
    if noninvasive {
        args.push("-pv".into());
    }
    if !initial_break {
        args.push("-g".into());
    }
    args.push("-p".into());
    args.push(pid.to_string());
    if let Some(sp) = symbol_path {
        args.push("-y".into());
        args.push(sp.to_string());
    }
    args.extend(extra_args.iter().cloned());
    args
}

/// Argv builder for a launch session (`cdb <executable> <args...>`).
pub fn launch_argv(
    executable: &Path,
    target_args: &[String],
    debug_children: bool,
    initial_break: bool,
    symbol_path: Option<&str>,
    extra_args: &[String],
) -> Result<Vec<String>> {
    let mut args = Vec::new();
    if !initial_break {
        args.push("-g".into());
    }
    if debug_children {
        args.push("-o".into());
    }
    if let Some(sp) = symbol_path {
        args.push("-y".into());
        args.push(sp.to_string());
    }
    args.extend(extra_args.iter().cloned());
    args.push(
        executable
            .to_str()
            .ok_or_else(|| Error::other("non-utf8 executable path"))?
            .to_string(),
    );
    args.extend(target_args.iter().cloned());
    Ok(args)
}

/// In-band cdb commands (sent over stdin) used by [`crate::cdb::session`].
pub mod cmd {
    /// Continue execution.
    pub const GO: &[u8] = b"g\n";
    /// Detach from the target, leaving it running.
    pub const QUIT_DETACH: &[u8] = b"qd\n";
    /// Terminate target and debugger.
    pub const QUIT_TERMINATE: &[u8] = b"q\n";
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn attach_basic() {
        let argv = attach_argv(1234, false, true, None, &[]);
        assert_eq!(argv, vec!["-p", "1234"]);
    }

    #[test]
    fn attach_with_symbol_path_and_no_initial_break() {
        let argv = attach_argv(
            42,
            false,
            false,
            Some("srv*c:/sym"),
            &["-srcpath".into(), "X".into()],
        );
        assert_eq!(
            argv,
            vec!["-g", "-p", "42", "-y", "srv*c:/sym", "-srcpath", "X"]
        );
    }

    #[test]
    fn attach_noninvasive() {
        let argv = attach_argv(7, true, true, None, &[]);
        assert_eq!(argv, vec!["-pv", "-p", "7"]);
    }

    #[test]
    fn launch_with_debug_children() {
        let argv = launch_argv(
            &PathBuf::from("notepad.exe"),
            &["a.txt".into()],
            true,
            true,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(argv, vec!["-o", "notepad.exe", "a.txt"]);
    }

    #[test]
    fn launch_with_no_initial_break_and_symbols() {
        let argv = launch_argv(
            &PathBuf::from("c:/x.exe"),
            &[],
            false,
            false,
            Some("srv*c:/sym"),
            &[],
        )
        .unwrap();
        assert_eq!(argv, vec!["-g", "-y", "srv*c:/sym", "c:/x.exe"]);
    }
}
