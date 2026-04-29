//! Tracks all active cdb sessions and enforces global limits.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{info, warn};

use crate::cdb::constants::SHUTDOWN_PER_SESSION;
use crate::cdb::session::{
    spawn_attach, spawn_launch, AttachOptions, LaunchOptions, Session, SessionState,
    SessionSummary, SpawnConfig,
};
use crate::config::Config;
use crate::error::{Error, Result};

#[derive(Clone)]
pub struct SessionManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    max_sessions: usize,
    spawn_cfg: SpawnConfig,
    sessions: RwLock<HashMap<String, Session>>,
}

impl SessionManager {
    pub fn new(cfg: &Config) -> Result<Self> {
        let cdb_path = cfg.cdb_path()?.to_path_buf();
        let spawn_cfg = SpawnConfig {
            cdb_path,
            symbol_path: cfg.debugger.symbol_path.clone(),
            output_ring_bytes: cfg.debugger.output_ring_bytes,
            attach_timeout_ms: cfg.debugger.attach_timeout_ms,
            launch_timeout_ms: cfg.debugger.launch_timeout_ms,
        };
        Ok(Self {
            inner: Arc::new(ManagerInner {
                max_sessions: cfg.server.max_sessions,
                spawn_cfg,
                sessions: RwLock::new(HashMap::new()),
            }),
        })
    }

    pub async fn attach(&self, opts: AttachOptions) -> Result<Session> {
        self.check_capacity()?;
        let session = spawn_attach(&self.inner.spawn_cfg, opts).await?;
        self.register(session.clone());
        Ok(session)
    }

    pub async fn launch(&self, opts: LaunchOptions) -> Result<Session> {
        self.check_capacity()?;
        let session = spawn_launch(&self.inner.spawn_cfg, opts).await?;
        self.register(session.clone());
        Ok(session)
    }

    fn check_capacity(&self) -> Result<()> {
        let mut guard = self.inner.sessions.write();
        guard.retain(|_, s| s.shared.state.lock().is_active());
        if guard.len() >= self.inner.max_sessions {
            return Err(Error::SessionLimit(self.inner.max_sessions));
        }
        Ok(())
    }

    /// Drop any sessions whose state is no longer active (`Stopped` or
    /// `Failed`). Returns the number of sessions removed. Used both
    /// before listing sessions and before accepting a new spawn so
    /// dead entries never accumulate in the map.
    pub fn prune_inactive(&self) -> usize {
        let mut guard = self.inner.sessions.write();
        let before = guard.len();
        guard.retain(|_, s| s.shared.state.lock().is_active());
        before - guard.len()
    }

    /// Return a synchronous snapshot of every active session. Calls
    /// `prune_inactive()` first so callers do not see ids that the
    /// debugger child already exited on. Snapshot is built without
    /// touching session actors so a hung child cannot stall the list.
    pub fn list_active(&self) -> Vec<SessionSummary> {
        self.prune_inactive();
        self.inner
            .sessions
            .read()
            .values()
            .map(|s| s.summary())
            .collect()
    }

    fn register(&self, session: Session) {
        let id = session.id.clone();
        self.inner.sessions.write().insert(id.clone(), session);
        info!(session = %id, "session registered");
    }

    pub fn get(&self, id: &str) -> Result<Session> {
        let guard = self.inner.sessions.read();
        guard
            .get(id)
            .cloned()
            .ok_or_else(|| Error::SessionNotFound(id.into()))
    }

    pub fn remove(&self, id: &str) {
        self.inner.sessions.write().remove(id);
    }

    /// Stop all active sessions (used on graceful shutdown).
    pub async fn shutdown(&self) {
        let sessions: Vec<Session> = self.inner.sessions.read().values().cloned().collect();
        for s in sessions {
            let state = *s.shared.state.lock();
            if matches!(state, SessionState::Stopped | SessionState::Failed) {
                continue;
            }
            if let Err(e) = tokio::time::timeout(
                SHUTDOWN_PER_SESSION,
                s.stop(crate::cdb::session::StopMode::KillDebugger),
            )
            .await
            {
                warn!(session = %s.id, error = %e, "shutdown stop timed out");
            }
        }
    }
}
