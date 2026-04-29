//! Real-cdb integration tests.
//!
//! These tests spawn `cmd.exe /c "ping -n 30 127.0.0.1 >nul"` as a disposable
//! Windows-built-in target process and drive a real `cdb.exe` against it. They
//! are skipped automatically when `cdb.exe` cannot be located on the host —
//! see [`windbgr_mcp::config::detect_cdb`] and the `WINDBG_CDB_PATH` env var.
//!

#![cfg(windows)]

use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio};
use std::time::Duration;

use windbgr_mcp::cdb::manager::SessionManager;
use windbgr_mcp::cdb::session::{AttachOptions, ControlAction, LaunchOptions, StopMode};
use windbgr_mcp::config::{detect_cdb, Config};

fn maybe_cdb() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WINDBG_CDB_PATH") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    detect_cdb()
}

fn make_manager(cdb: PathBuf) -> SessionManager {
    let mut cfg = Config::default();
    cfg.debugger.cdb_path = Some(cdb);
    cfg.debugger.attach_timeout_ms = 30_000;
    cfg.debugger.launch_timeout_ms = 30_000;
    cfg.debugger.command_timeout_ms = 30_000;
    cfg.server.max_sessions = 8;
    SessionManager::new(&cfg).expect("manager")
}

fn spawn_disposable_target() -> std::process::Child {
    StdCommand::new("cmd.exe")
        .args(["/c", "ping -n 30 127.0.0.1 >nul"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn target")
}

#[tokio::test(flavor = "multi_thread")]
async fn attach_real_cdb_to_disposable_target() {
    let Some(cdb) = maybe_cdb() else {
        eprintln!(
            "skipping: cdb.exe not found; set WINDBG_CDB_PATH or install Windows Debugging Tools"
        );
        return;
    };
    let mut target = spawn_disposable_target();
    let target_pid = target.id();

    let mgr = make_manager(cdb);
    let session = mgr
        .attach(AttachOptions {
            pid: target_pid,
            noninvasive: false,
            initial_break: true,
            symbol_path: None,
            extra_args: vec![],
        })
        .await
        .expect("attach");

    // Registers register dump should mention an instruction-pointer style
    // register on either x86 or x64.
    let outcome = session
        .run_command("r".into(), 30_000)
        .await
        .expect("r command");
    let lower = outcome.output.to_lowercase();
    assert!(
        lower.contains("eax")
            || lower.contains("rax")
            || lower.contains("eip")
            || lower.contains("rip"),
        "registers output should contain a CPU register; got: {}",
        outcome.output
    );

    // List loaded modules — every Win32 process loads ntdll.dll.
    let outcome = session
        .run_command("lm".into(), 30_000)
        .await
        .expect("lm command");
    assert!(
        outcome.output.to_lowercase().contains("ntdll"),
        "module list should mention ntdll; got: {}",
        outcome.output
    );

    // Continue, then break — verifies CTRL_BREAK works against the cdb group.
    session
        .control(ControlAction::Continue)
        .await
        .expect("continue");
    tokio::time::sleep(Duration::from_millis(500)).await;
    session.control(ControlAction::Break).await.expect("break");
    session
        .wait_ready(Duration::from_secs(10))
        .await
        .expect("wait ready after break");

    // Detach and keep the target alive briefly to confirm it is not killed.
    session.stop(StopMode::Detach).await.expect("detach");

    let alive = target.try_wait().ok().flatten().is_none();
    assert!(alive, "detach should leave the ping target alive");

    let _ = target.kill();
    let _ = target.wait();
}

#[tokio::test(flavor = "multi_thread")]
async fn launch_real_cdb_terminate_target() {
    let Some(cdb) = maybe_cdb() else {
        eprintln!(
            "skipping: cdb.exe not found; set WINDBG_CDB_PATH or install Windows Debugging Tools"
        );
        return;
    };
    let mgr = make_manager(cdb);
    let cmd = std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
    let session = mgr
        .launch(LaunchOptions {
            executable: PathBuf::from(cmd),
            args: vec!["/c".into(), "ping -n 30 127.0.0.1 >nul".into()],
            cwd: None,
            env: vec![],
            debug_children: false,
            initial_break: true,
            symbol_path: None,
            extra_args: vec![],
        })
        .await
        .expect("launch");

    let outcome = session
        .run_command("lm".into(), 30_000)
        .await
        .expect("lm command after launch");
    assert!(
        !outcome.output.trim().is_empty(),
        "lm output should not be empty"
    );

    session
        .stop(StopMode::TerminateTarget)
        .await
        .expect("terminate target");
}

/// Regression test for the stuck-session bug observed in the chatwise/MCP flow:
///
///   1. attach to process
///   2. `x ntdll!*` (a symbol-listing command that produces output + prompt)
///   3. `bp <addr>` (breakpoint-set command that produces NO visible output)
///
/// Before the fix, step 3 timed out because cdb emits the next prompt
/// back-to-back after the previous one (no newline separator), and the prompt
/// regex failed to match. This test ensures `bp` completes promptly.
#[tokio::test(flavor = "multi_thread")]
async fn bp_no_output_command_completes_promptly() {
    let Some(cdb) = maybe_cdb() else {
        eprintln!("skipping: cdb.exe not found");
        return;
    };
    let mut target = spawn_disposable_target();
    let target_pid = target.id();
    let mgr = make_manager(cdb);
    let session = mgr
        .attach(AttachOptions {
            pid: target_pid,
            noninvasive: false,
            initial_break: true,
            symbol_path: None,
            extra_args: vec![],
        })
        .await
        .expect("attach");

    // Step 1: run a command that produces output (mirrors the `x` call).
    let outcome = session
        .run_command("x ntdll!NtCreateFile".into(), 30_000)
        .await
        .expect("x command");
    assert!(
        !outcome.output.trim().is_empty(),
        "x command should produce output; got empty"
    );

    // Step 2: set a breakpoint — this produces NO visible output from cdb.
    // Before the fix this would time out (30s) because the prompt regex could
    // not match back-to-back prompts.
    let bp_start = std::time::Instant::now();
    let outcome = session
        .run_command("bp ntdll!NtCreateFile".into(), 10_000)
        .await
        .expect("bp command must not time out");
    let bp_elapsed = bp_start.elapsed();

    // The bp command should complete in well under 1 second.
    assert!(
        bp_elapsed < Duration::from_secs(5),
        "bp command took {bp_elapsed:?} — likely hit timeout; should be < 5s"
    );
    // bp output should be empty or very short (cdb may echo a confirmation).
    assert!(
        outcome.output.len() < 200,
        "bp should produce little/no output; got: {}",
        outcome.output
    );

    // Step 3: verify the session is still usable after bp.
    let outcome = session
        .run_command("bl".into(), 10_000)
        .await
        .expect("bl (list breakpoints) should work after bp");
    assert!(
        outcome.output.to_lowercase().contains("ntcreatefile"),
        "breakpoint list should show our bp; got: {}",
        outcome.output
    );

    session.stop(StopMode::Detach).await.expect("detach");
    let _ = target.kill();
    let _ = target.wait();
}

/// Ensure that issuing multiple no-output commands in sequence does not wedge
/// the session. This covers the scenario where consecutive prompts accumulate.
#[tokio::test(flavor = "multi_thread")]
async fn multiple_no_output_commands_stay_responsive() {
    let Some(cdb) = maybe_cdb() else {
        eprintln!("skipping: cdb.exe not found");
        return;
    };
    let mut target = spawn_disposable_target();
    let target_pid = target.id();
    let mgr = make_manager(cdb);
    let session = mgr
        .attach(AttachOptions {
            pid: target_pid,
            noninvasive: false,
            initial_break: true,
            symbol_path: None,
            extra_args: vec![],
        })
        .await
        .expect("attach");

    // Issue several bp commands in sequence — each produces no output.
    for i in 0..5 {
        let cmd = format!("bp{i} ntdll!NtCreateFile");
        let outcome = session
            .run_command(cmd.clone(), 10_000)
            .await
            .unwrap_or_else(|e| panic!("command #{i} ({cmd}) failed: {e}"));
        assert_eq!(
            outcome.state,
            windbgr_mcp::cdb::session::SessionState::Idle,
            "session should be idle after command #{i}"
        );
    }

    // Final sanity: session is still usable.
    let outcome = session
        .run_command("bl".into(), 10_000)
        .await
        .expect("bl should work after multiple bp commands");
    assert!(
        !outcome.output.trim().is_empty(),
        "breakpoint list should not be empty"
    );

    session.stop(StopMode::Detach).await.expect("detach");
    let _ = target.kill();
    let _ = target.wait();
}

#[tokio::test(flavor = "multi_thread")]
async fn debug_command_rejects_non_idle_state() {
    let Some(cdb) = maybe_cdb() else {
        eprintln!("skipping: cdb.exe not found");
        return;
    };
    let mut target = spawn_disposable_target();
    let target_pid = target.id();
    let mgr = make_manager(cdb);
    let session = mgr
        .attach(AttachOptions {
            pid: target_pid,
            noninvasive: false,
            initial_break: true,
            symbol_path: None,
            extra_args: vec![],
        })
        .await
        .expect("attach");

    session
        .control(ControlAction::Continue)
        .await
        .expect("continue");
    let err = session
        .run_command("r".into(), 5_000)
        .await
        .expect_err("commands must fail while target is running");
    let msg = err.to_string();
    assert!(
        msg.contains("Running") || msg.contains("Breaking") || msg.contains("Executing"),
        "expected invalid-state error, got: {msg}"
    );

    session.control(ControlAction::Break).await.expect("break");
    let _ = session.wait_ready(Duration::from_secs(5)).await;
    session.stop(StopMode::Detach).await.expect("detach");
    let _ = target.kill();
    let _ = target.wait();
}
