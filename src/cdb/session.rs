//! A single cdb.exe debugger session represented as a tokio actor.
//!
//! The actor owns the child process, its stdio, an output ring buffer and
//! the current session state. Interaction from the MCP layer happens
//! exclusively through [`Session`], which maps user-facing MCP tool calls
//! into typed requests dispatched over channels.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::cdb::cli::{self, attach_argv, launch_argv};
use crate::cdb::constants::{
    ACTOR_CANCEL_TICK, ACTOR_IDLE_BLOCK, EXIT_TAIL_BYTES, PROMPT_TAIL_BYTES, STOP_WAIT,
    STREAM_READ_BUF_BYTES, WAIT_READY_POLL,
};
use crate::cdb::control::generate_ctrl_break;
use crate::cdb::prompt::{clean_command_output, find_prompt};
use crate::cdb::ring::RingBuffer;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Attach,
    Launch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Starting,
    Idle,
    Executing,
    Running,
    Breaking,
    Stopped,
    Failed,
}

impl SessionState {
    /// True when the session still owns a live cdb child and can be
    /// driven by `debug_*` tools. `Stopped` and `Failed` are inactive
    /// because the underlying child process has exited (or been killed)
    /// and the session id can no longer service requests.
    pub fn is_active(self) -> bool {
        !matches!(self, SessionState::Stopped | SessionState::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session_id: String,
    pub kind: SessionKind,
    pub state: SessionState,
    pub target_pid: Option<u32>,
    pub cdb_pid: u32,
    pub started_at: String,
    pub last_activity_at: String,
    pub output_total_bytes: u64,
    pub output_earliest_offset: u64,
    pub last_error: Option<String>,
    pub exit_status: Option<String>,
}

/// Lightweight session view used by `debug_list_sessions`. Built
/// synchronously from `Session` + `SessionShared` so listing many
/// sessions never has to round-trip through each actor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub kind: SessionKind,
    pub state: SessionState,
    pub target_pid: Option<u32>,
    pub cdb_pid: u32,
    pub started_at: String,
    pub last_error: Option<String>,
    pub exit_status: Option<String>,
}

/// Returns the serde default `true` shared by every "do XYZ by default"
/// flag in this crate.
pub(crate) fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachOptions {
    pub pid: u32,
    #[serde(default)]
    pub noninvasive: bool,
    #[serde(default = "default_true")]
    pub initial_break: bool,
    #[serde(default)]
    pub symbol_path: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchOptions {
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default)]
    pub debug_children: bool,
    #[serde(default = "default_true")]
    pub initial_break: bool,
    #[serde(default)]
    pub symbol_path: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    Continue,
    Break,
    InterruptCommand,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopMode {
    /// `qd` — detach from target, leave target running.
    Detach,
    /// `q` — terminate target and debugger.
    TerminateTarget,
    /// Kill the cdb child process without sending a cdb command.
    KillDebugger,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandOutcome {
    pub state: SessionState,
    pub command: String,
    pub output: String,
    pub output_start_offset: u64,
    pub output_end_offset: u64,
    pub truncated: bool,
    pub duration_ms: u128,
}

// ---------- Internal request types ----------

enum Request {
    Command(String, u64, oneshot::Sender<Result<CommandOutcome>>),
    Control(ControlAction, oneshot::Sender<Result<SessionState>>),
    Stop(StopMode, oneshot::Sender<Result<()>>),
    Status(oneshot::Sender<SessionStatus>),
    ReadOutput(u64, usize, oneshot::Sender<OutputPage>),
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputPage {
    pub bytes: String,
    pub next_offset: u64,
    pub earliest_offset: u64,
    pub total_written: u64,
    pub truncated_from_start: bool,
}

// ---------- Session handle used from the manager ----------

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub kind: SessionKind,
    pub target_pid: Option<u32>,
    pub cdb_pid: u32,
    pub started_at: String,
    tx: mpsc::Sender<Request>,
    pub shared: Arc<SessionShared>,
}

#[derive(Debug)]
pub struct SessionShared {
    pub state: Mutex<SessionState>,
    pub last_error: Mutex<Option<String>>,
    pub exit_status: Mutex<Option<String>>,
}

impl Session {
    pub fn new_id() -> String {
        Uuid::new_v4().to_string()
    }

    pub async fn run_command(&self, command: String, timeout_ms: u64) -> Result<CommandOutcome> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Request::Command(command, timeout_ms, tx))
            .await
            .map_err(|_| Error::other("session actor gone"))?;
        rx.await
            .map_err(|_| Error::other("session actor dropped"))?
    }

    pub async fn control(&self, action: ControlAction) -> Result<SessionState> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Request::Control(action, tx))
            .await
            .map_err(|_| Error::other("session actor gone"))?;
        rx.await
            .map_err(|_| Error::other("session actor dropped"))?
    }

    pub async fn stop(&self, mode: StopMode) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Request::Stop(mode, tx))
            .await
            .map_err(|_| Error::other("session actor gone"))?;
        rx.await
            .map_err(|_| Error::other("session actor dropped"))?
    }

    pub async fn status(&self) -> Result<SessionStatus> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Request::Status(tx))
            .await
            .map_err(|_| Error::other("session actor gone"))?;
        rx.await.map_err(|_| Error::other("session actor dropped"))
    }

    pub async fn read_output(&self, since_offset: u64, max_bytes: usize) -> Result<OutputPage> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Request::ReadOutput(since_offset, max_bytes, tx))
            .await
            .map_err(|_| Error::other("session actor gone"))?;
        rx.await.map_err(|_| Error::other("session actor dropped"))
    }

    /// Synchronous, non-blocking snapshot suitable for listing many
    /// sessions in one call. Reads only `Session` identity fields and
    /// the shared mutex-protected status; never touches the actor's
    /// `mpsc::Sender`, so a hung or dropped actor does not affect the
    /// list view.
    pub fn summary(&self) -> SessionSummary {
        SessionSummary {
            session_id: self.id.clone(),
            kind: self.kind,
            state: *self.shared.state.lock(),
            target_pid: self.target_pid,
            cdb_pid: self.cdb_pid,
            started_at: self.started_at.clone(),
            last_error: self.shared.last_error.lock().clone(),
            exit_status: self.shared.exit_status.lock().clone(),
        }
    }

    /// Block until the session returns to `Idle` — used both for "wait for
    /// initial attach prompt" and "wait for the next break (e.g. breakpoint
    /// hit, exception, ctrl+break)".
    pub async fn wait_ready(&self, within: Duration) -> Result<()> {
        let deadline = Instant::now() + within;
        loop {
            let state = *self.shared.state.lock();
            match state {
                SessionState::Idle => return Ok(()),
                SessionState::Failed | SessionState::Stopped => {
                    let err = self
                        .shared
                        .last_error
                        .lock()
                        .clone()
                        .unwrap_or_else(|| "cdb exited before ready".into());
                    return Err(Error::CdbExited(err));
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout(within.as_millis() as u64));
            }
            tokio::time::sleep(WAIT_READY_POLL).await;
        }
    }
}

// ---------- Spawning ----------

pub struct SpawnConfig {
    pub cdb_path: PathBuf,
    pub symbol_path: Option<String>,
    pub output_ring_bytes: usize,
    pub attach_timeout_ms: u64,
    pub launch_timeout_ms: u64,
}

pub async fn spawn_attach(cfg: &SpawnConfig, opts: AttachOptions) -> Result<Session> {
    let symbol_path = opts.symbol_path.as_deref().or(cfg.symbol_path.as_deref());
    let args = attach_argv(
        opts.pid,
        opts.noninvasive,
        opts.initial_break,
        symbol_path,
        &opts.extra_args,
    );
    spawn_common(SpawnArgs {
        cfg,
        kind: SessionKind::Attach,
        target_pid: Some(opts.pid),
        argv: args,
        wait_timeout: Duration::from_millis(cfg.attach_timeout_ms),
        cwd: None,
        env: Vec::new(),
    })
    .await
}

pub async fn spawn_launch(cfg: &SpawnConfig, opts: LaunchOptions) -> Result<Session> {
    let symbol_path = opts.symbol_path.as_deref().or(cfg.symbol_path.as_deref());
    let args = launch_argv(
        &opts.executable,
        &opts.args,
        opts.debug_children,
        opts.initial_break,
        symbol_path,
        &opts.extra_args,
    )?;
    spawn_common(SpawnArgs {
        cfg,
        kind: SessionKind::Launch,
        target_pid: None,
        argv: args,
        wait_timeout: Duration::from_millis(cfg.launch_timeout_ms),
        cwd: opts.cwd,
        env: opts.env,
    })
    .await
}

struct SpawnArgs<'a> {
    cfg: &'a SpawnConfig,
    kind: SessionKind,
    target_pid: Option<u32>,
    argv: Vec<String>,
    wait_timeout: Duration,
    cwd: Option<PathBuf>,
    env: Vec<(String, String)>,
}

async fn spawn_common(s: SpawnArgs<'_>) -> Result<Session> {
    let SpawnArgs {
        cfg,
        kind,
        target_pid,
        argv,
        wait_timeout,
        cwd,
        env,
    } = s;
    if !cfg.cdb_path.exists() {
        return Err(Error::CdbNotFound(cfg.cdb_path.display().to_string()));
    }
    let mut cmd = Command::new(&cfg.cdb_path);
    cmd.args(&argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = &cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    apply_creation_flags(&mut cmd);

    info!(
        kind = ?kind,
        args = ?argv,
        cdb = %cfg.cdb_path.display(),
        "spawning cdb"
    );
    let mut child = cmd.spawn()?;
    let cdb_pid = child
        .id()
        .ok_or_else(|| Error::other("child pid missing"))?;
    let stdin = child.stdin.take().ok_or_else(|| Error::other("no stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::other("no stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::other("no stderr"))?;

    let id = Session::new_id();
    let shared = Arc::new(SessionShared {
        state: Mutex::new(SessionState::Starting),
        last_error: Mutex::new(None),
        exit_status: Mutex::new(None),
    });

    let (tx, rx) = mpsc::channel(64);
    let started_at = crate::audit::now_ts();

    let actor = SessionActor {
        id: id.clone(),
        kind,
        cdb_pid,
        target_pid,
        started_at: started_at.clone(),
        child,
        stdin,
        shared: shared.clone(),
        ring: RingBuffer::new(cfg.output_ring_bytes),
        executing: None,
        last_activity: Instant::now(),
    };
    tokio::spawn(actor.run(rx, stdout, stderr));

    let session = Session {
        id: id.clone(),
        kind,
        target_pid,
        cdb_pid,
        started_at,
        tx,
        shared,
    };
    session.wait_ready(wait_timeout).await.inspect_err(|e| {
        *session.shared.state.lock() = SessionState::Failed;
        *session.shared.last_error.lock() = Some(e.to_string());
    })?;
    Ok(session)
}

#[cfg(windows)]
fn apply_creation_flags(cmd: &mut Command) {
    cmd.creation_flags(crate::cdb::control::CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(windows))]
fn apply_creation_flags(_cmd: &mut Command) {}

// ---------- Actor ----------

struct SessionActor {
    id: String,
    kind: SessionKind,
    cdb_pid: u32,
    target_pid: Option<u32>,
    started_at: String,
    child: Child,
    stdin: ChildStdin,
    shared: Arc<SessionShared>,
    ring: RingBuffer,
    executing: Option<PendingCommand>,
    last_activity: Instant,
}

struct PendingCommand {
    command: String,
    start_offset: u64,
    started: Instant,
    deadline: Option<Instant>,
    tx: oneshot::Sender<Result<CommandOutcome>>,
}

impl SessionActor {
    async fn run(
        mut self,
        mut rx: mpsc::Receiver<Request>,
        stdout: ChildStdout,
        stderr: ChildStderr,
    ) {
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
        let out_tx_err = out_tx.clone();
        let id_stdout = self.id.clone();
        let id_stderr = self.id.clone();
        tokio::spawn(read_stream(stdout, out_tx, id_stdout, "stdout"));
        tokio::spawn(read_stream(stderr, out_tx_err, id_stderr, "stderr"));

        loop {
            let now = Instant::now();
            let deadline = self
                .executing
                .as_ref()
                .and_then(|p| p.deadline)
                .map(|d| d.saturating_duration_since(now))
                .unwrap_or(ACTOR_IDLE_BLOCK);
            tokio::select! {
                chunk = out_rx.recv() => {
                    match chunk {
                        Some(bytes) => self.on_output(&bytes),
                        None => {
                            self.on_child_exit().await;
                            break;
                        }
                    }
                }
                Some(req) = rx.recv() => {
                    if self.handle_request(req).await {
                        break;
                    }
                }
                exit = self.child.wait() => {
                    match exit {
                        Ok(status) => {
                            *self.shared.exit_status.lock() = Some(status.to_string());
                            info!(session = %self.id, status = ?status, "cdb exited");
                        }
                        Err(e) => {
                            *self.shared.last_error.lock() = Some(e.to_string());
                        }
                    }
                    self.on_child_exit().await;
                    break;
                }
                _ = tokio::time::sleep(ACTOR_CANCEL_TICK), if self.pending_command_cancelled() => {
                    self.interrupt_cancelled_command();
                }
                _ = tokio::time::sleep(deadline) => {
                    if let Some(p) = self.executing.take() {
                        let elapsed_ms = p.started.elapsed().as_millis() as u64;
                        let command = p.command.clone();
                        let _ = p.tx.send(Err(Error::Timeout(elapsed_ms)));
                        self.interrupt_cdb_command(&command, "debug_command timed out");
                    }
                }
            }
        }
    }

    fn pending_command_cancelled(&self) -> bool {
        self.executing
            .as_ref()
            .map(|p| p.tx.is_closed())
            .unwrap_or(false)
    }

    fn interrupt_cancelled_command(&mut self) {
        if let Some(p) = self.executing.take() {
            self.interrupt_cdb_command(&p.command, "debug_command caller was dropped");
        }
    }

    fn interrupt_cdb_command(&mut self, command: &str, reason: &'static str) {
        let state = *self.shared.state.lock();
        if matches!(
            state,
            SessionState::Idle
                | SessionState::Breaking
                | SessionState::Stopped
                | SessionState::Failed
        ) {
            return;
        }

        // Safety belt: the command may have already completed (prompt
        // sitting in the buffer) but was missed by on_output due to a race
        // with the timeout. Re-check the buffer now to avoid sending a
        // spurious CTRL+BREAK to a cdb that is already at its prompt.
        let tail = self.ring.snapshot_tail(PROMPT_TAIL_BYTES);
        if find_prompt(&tail).is_some() {
            *self.shared.state.lock() = SessionState::Idle;
            debug!(
                session = %self.id,
                command = %command,
                reason,
                "prompt already in buffer; recovering to Idle without CTRL+BREAK"
            );
            return;
        }

        match generate_ctrl_break(self.cdb_pid) {
            Ok(()) => {
                *self.shared.state.lock() = SessionState::Breaking;
                warn!(
                    session = %self.id,
                    command = %command,
                    reason,
                    "interrupting cdb command"
                );
            }
            Err(e) => {
                *self.shared.last_error.lock() = Some(format!(
                    "failed to interrupt cdb command after {reason}: {e}"
                ));
                warn!(
                    session = %self.id,
                    command = %command,
                    reason,
                    error = %e,
                    "failed to interrupt cdb command"
                );
            }
        }
    }

    fn on_output(&mut self, bytes: &[u8]) {
        self.ring.push(bytes);
        self.last_activity = Instant::now();

        let current_state = *self.shared.state.lock();
        let tail = self.ring.snapshot_tail(PROMPT_TAIL_BYTES);
        let prompt = find_prompt(&tail);

        if prompt.is_some() {
            match current_state {
                SessionState::Starting => {
                    *self.shared.state.lock() = SessionState::Idle;
                }
                // Either the prompt returned because the command finished
                // normally (Executing) or because a CTRL+BREAK interrupted
                // it (Breaking). In both cases we must resolve any
                // in-flight PendingCommand — otherwise its `tx` lingers,
                // the deadline timer fires later and reports a bogus
                // timeout, and a stale `executing` slot can be silently
                // overwritten by the next command. We treat a
                // break-induced prompt as a successful (partial)
                // completion so the caller still gets whatever output cdb
                // produced before being interrupted.
                SessionState::Breaking | SessionState::Executing => {
                    if let Some(p) = self.executing.take() {
                        let start = p.start_offset;
                        let total = self.ring.total_written();
                        let (bytes, _next, truncated) = self.ring.read_since(start);
                        let text = String::from_utf8_lossy(&bytes).into_owned();
                        let cleaned = clean_command_output(&text, &p.command);
                        let outcome = CommandOutcome {
                            state: SessionState::Idle,
                            command: p.command.clone(),
                            output: cleaned,
                            output_start_offset: start,
                            output_end_offset: total,
                            truncated,
                            duration_ms: p.started.elapsed().as_millis(),
                        };
                        let _ = p.tx.send(Ok(outcome));
                    }
                    *self.shared.state.lock() = SessionState::Idle;
                }
                SessionState::Running => {
                    // Unsolicited prompt — target likely hit a breakpoint
                    // or exited. Transition to Idle.
                    *self.shared.state.lock() = SessionState::Idle;
                }
                _ => {}
            }
        }
    }

    async fn on_child_exit(&mut self) {
        *self.shared.state.lock() = SessionState::Stopped;
        let tail = self.ring.snapshot_tail(EXIT_TAIL_BYTES);
        let trimmed = tail.trim();
        if !trimmed.is_empty() {
            let mut err = self.shared.last_error.lock();
            if err.is_none() {
                *err = Some(format!("cdb output: {trimmed}"));
            }
        }
        if let Some(p) = self.executing.take() {
            let msg = if trimmed.is_empty() {
                "cdb exited during command".to_string()
            } else {
                format!("cdb exited during command. Last output:\n{trimmed}")
            };
            let _ = p.tx.send(Err(Error::CdbExited(msg)));
        }
    }

    async fn handle_request(&mut self, req: Request) -> bool {
        match req {
            Request::Command(cmd, timeout_ms, tx) => {
                let state = *self.shared.state.lock();
                if state != SessionState::Idle {
                    let _ = tx.send(Err(Error::InvalidState {
                        current: format!("{state:?}"),
                        action: "debug_command".into(),
                    }));
                    return false;
                }
                let start_offset = self.ring.total_written();
                let line = format!("{}\n", cmd.trim_end());
                if let Err(e) = self.stdin.write_all(line.as_bytes()).await {
                    let _ = tx.send(Err(Error::Io(e)));
                    return false;
                }
                let _ = self.stdin.flush().await;
                *self.shared.state.lock() = SessionState::Executing;
                let deadline = if timeout_ms == 0 {
                    None
                } else {
                    Some(Instant::now() + Duration::from_millis(timeout_ms))
                };
                self.executing = Some(PendingCommand {
                    command: cmd,
                    start_offset,
                    started: Instant::now(),
                    deadline,
                    tx,
                });
            }
            Request::Control(action, tx) => {
                let state = *self.shared.state.lock();
                match action {
                    ControlAction::Continue => {
                        if state != SessionState::Idle {
                            let _ = tx.send(Err(Error::InvalidState {
                                current: format!("{state:?}"),
                                action: "continue".into(),
                            }));
                            return false;
                        }
                        if let Err(e) = self.stdin.write_all(cli::cmd::GO).await {
                            let _ = tx.send(Err(Error::Io(e)));
                            return false;
                        }
                        let _ = self.stdin.flush().await;
                        *self.shared.state.lock() = SessionState::Running;
                        let _ = tx.send(Ok(SessionState::Running));
                    }
                    ControlAction::Break | ControlAction::InterruptCommand => {
                        if state == SessionState::Stopped || state == SessionState::Failed {
                            let _ = tx.send(Err(Error::InvalidState {
                                current: format!("{state:?}"),
                                action: "break".into(),
                            }));
                            return false;
                        }
                        // If we're already at the prompt there is nothing
                        // to interrupt. Sending CTRL+BREAK to a cdb that
                        // is sitting at its prompt produces no output,
                        // which would leave the session wedged in
                        // `Breaking` (waiting for a prompt that never
                        // arrives).
                        if state == SessionState::Idle {
                            let _ = tx.send(Ok(SessionState::Idle));
                            return false;
                        }
                        match generate_ctrl_break(self.cdb_pid) {
                            Ok(()) => {
                                *self.shared.state.lock() = SessionState::Breaking;
                                let _ = tx.send(Ok(SessionState::Breaking));
                            }
                            Err(e) => {
                                let _ = tx.send(Err(e));
                            }
                        }
                    }
                }
            }
            Request::Stop(mode, tx) => {
                match mode {
                    StopMode::Detach => {
                        let _ = self.stdin.write_all(cli::cmd::QUIT_DETACH).await;
                        let _ = self.stdin.flush().await;
                    }
                    StopMode::TerminateTarget => {
                        let _ = self.stdin.write_all(cli::cmd::QUIT_TERMINATE).await;
                        let _ = self.stdin.flush().await;
                    }
                    StopMode::KillDebugger => {
                        let _ = self.child.start_kill();
                    }
                }
                let wait = tokio::time::timeout(STOP_WAIT, self.child.wait()).await;
                match wait {
                    Ok(Ok(status)) => {
                        *self.shared.exit_status.lock() = Some(status.to_string());
                    }
                    Ok(Err(e)) => {
                        warn!(error = %e, "child wait after stop failed");
                    }
                    Err(_) => {
                        warn!("stop timeout; force-killing cdb");
                        let _ = self.child.start_kill();
                    }
                }
                *self.shared.state.lock() = SessionState::Stopped;
                let _ = tx.send(Ok(()));
                return true;
            }
            Request::Status(tx) => {
                let status = self.current_status();
                let _ = tx.send(status);
            }
            Request::ReadOutput(since, max_bytes, tx) => {
                let (bytes, next, truncated) = self.ring.read_since(since);
                let mut slice = bytes;
                if max_bytes > 0 && slice.len() > max_bytes {
                    let skip = slice.len() - max_bytes;
                    slice.drain(..skip);
                }
                let page = OutputPage {
                    bytes: String::from_utf8_lossy(&slice).into_owned(),
                    next_offset: next,
                    earliest_offset: self.ring.earliest_offset(),
                    total_written: self.ring.total_written(),
                    truncated_from_start: truncated,
                };
                let _ = tx.send(page);
            }
        }
        false
    }

    fn current_status(&self) -> SessionStatus {
        SessionStatus {
            session_id: self.id.clone(),
            kind: self.kind,
            state: *self.shared.state.lock(),
            target_pid: self.target_pid,
            cdb_pid: self.cdb_pid,
            started_at: self.started_at.clone(),
            last_activity_at: format!("{}ms ago", self.last_activity.elapsed().as_millis()),
            output_total_bytes: self.ring.total_written(),
            output_earliest_offset: self.ring.earliest_offset(),
            last_error: self.shared.last_error.lock().clone(),
            exit_status: self.shared.exit_status.lock().clone(),
        }
    }
}

async fn read_stream<R>(mut stream: R, tx: mpsc::Sender<Vec<u8>>, id: String, kind: &'static str)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buf = [0u8; STREAM_READ_BUF_BYTES];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                if tx.send(buf[..n].to_vec()).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                warn!(session = %id, stream = kind, error = %e, "read error");
                break;
            }
        }
    }
    debug!(session = %id, stream = kind, "stream closed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_active_classification() {
        for s in [
            SessionState::Starting,
            SessionState::Idle,
            SessionState::Executing,
            SessionState::Running,
            SessionState::Breaking,
        ] {
            assert!(s.is_active(), "{s:?} should be active");
        }
        for s in [SessionState::Stopped, SessionState::Failed] {
            assert!(!s.is_active(), "{s:?} should be inactive");
        }
    }
}
