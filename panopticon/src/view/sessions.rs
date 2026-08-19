//! The sessions pane: which agent worktrees exist and which are live.

use parallax_baseline::adapters::session::DEFAULT_IDLE_AFTER;
use parallax_baseline::state::ProjectState;
use std::time::{Duration, SystemTime};

/// One row of the sessions pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    /// The session directory's name.
    pub name: String,
    /// Whether it showed activity inside the idle window.
    pub active: bool,
    /// How long since anything inside it changed.
    pub idle_for: Duration,
}

/// Every session the feed reported, in the order it reported them.
pub fn session_rows(project: &ProjectState, now: SystemTime) -> Vec<SessionRow> {
    let Some(sessions) = &project.sessions else {
        return Vec::new();
    };
    sessions
        .value
        .iter()
        .map(|s| SessionRow {
            name: s.name.clone(),
            active: s.is_active(now, DEFAULT_IDLE_AFTER),
            idle_for: now
                .duration_since(s.last_activity)
                .unwrap_or(Duration::ZERO),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::test_support::*;
    use parallax_baseline::adapters::session::Session;

    fn session(name: &str, last_activity: SystemTime) -> Session {
        Session {
            name: name.to_string(),
            path: std::path::PathBuf::from("/tmp").join(name),
            last_activity,
        }
    }

    #[test]
    fn a_project_with_no_session_feed_has_no_rows() {
        assert!(session_rows(&bare_project("p"), at(0)).is_empty());
    }

    #[test]
    fn a_session_inside_the_idle_window_is_active() {
        let p = project_with(|p| {
            p.sessions = Some(watched(vec![session("phases-slice2", at(0))], at(0)));
        });
        let rows = session_rows(&p, at(60));
        assert!(rows[0].active);
        assert_eq!(rows[0].idle_for, Duration::from_secs(60));
    }

    #[test]
    fn a_session_past_the_idle_window_is_not_active() {
        let p = project_with(|p| {
            p.sessions = Some(watched(vec![session("stale", at(0))], at(0)));
        });
        assert!(
            !session_rows(&p, at(601))[0].active,
            "the idle window is five minutes"
        );
    }

    /// A declared feed that found nothing is not the same as no feed,
    /// and both are legitimate.
    #[test]
    fn a_declared_feed_with_no_sessions_yields_no_rows_without_erroring() {
        let p = project_with(|p| p.sessions = Some(watched(Vec::new(), at(0))));
        assert!(session_rows(&p, at(0)).is_empty());
    }
}
