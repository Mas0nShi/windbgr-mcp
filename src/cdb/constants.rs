//! Internal tunables for the cdb session actor.
//!
//! Centralised so that we can adjust them in one place and test against
//! known values, instead of having `16 * 1024`, `4096`, `50ms` and friends
//! scattered across the actor.

use std::time::Duration;

/// Bytes of buffered stdout/stderr inspected when looking for the next
/// prompt or for end-of-stream context.
pub const PROMPT_TAIL_BYTES: usize = 16 * 1024;

/// Bytes of buffered output reported in the "cdb exited" diagnostic.
pub const EXIT_TAIL_BYTES: usize = 8 * 1024;

/// Read buffer used by the stdout/stderr forwarder tasks.
pub const STREAM_READ_BUF_BYTES: usize = 4096;

/// Polling interval for `Session::wait_ready` when waiting for the next
/// prompt.
pub const WAIT_READY_POLL: Duration = Duration::from_millis(50);

/// Periodic tick used by the actor's `select!` loop to detect callers
/// dropping their command future.
pub const ACTOR_CANCEL_TICK: Duration = Duration::from_millis(250);

/// Fallback pseudo-deadline when no per-command deadline is set. Kept
/// large because the actor's `select!` arm uses it as a "no timeout"
/// stand-in.
pub const ACTOR_IDLE_BLOCK: Duration = Duration::from_secs(60 * 60);

/// Maximum time we wait for the cdb child to exit after a `Stop` request
/// before falling back to a forced kill.
pub const STOP_WAIT: Duration = Duration::from_secs(10);

/// Maximum time `SessionManager::shutdown` waits for an individual
/// session to terminate.
pub const SHUTDOWN_PER_SESSION: Duration = Duration::from_secs(5);
