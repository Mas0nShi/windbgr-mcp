//! cdb.exe integration layer: session actor, manager and break control.

pub mod cli;
pub mod constants;
pub mod control;
pub mod manager;
pub mod prompt;
pub mod ring;
pub mod session;

pub use manager::SessionManager;
pub use session::{
    AttachOptions, CommandOutcome, ControlAction, LaunchOptions, SessionKind, SessionState,
    SessionStatus, SessionSummary, StopMode,
};
