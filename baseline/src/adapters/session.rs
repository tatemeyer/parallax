//! The session family: agent working directories, so a frontend can
//! show what is running where. One built-in implementation, a
//! filesystem scan (Task 17).

use super::{AdapterError, ProjectContext};
use crate::freshness::Observed;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// One agent session directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The directory's name.
    pub name: String,
    /// Absolute path to the directory.
    pub path: PathBuf,
    /// The most recent modification time anywhere inside it.
    pub last_activity: SystemTime,
}

impl Session {
    /// Whether this session showed activity within `idle_after` of `now`.
    pub fn is_active(&self, now: SystemTime, idle_after: Duration) -> bool {
        now.duration_since(self.last_activity)
            .unwrap_or(Duration::ZERO)
            < idle_after
    }
}

/// A source of agent sessions.
pub trait SessionAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Scans the session `watch` glob as of `now`.
    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Session>>, AdapterError>;
}
