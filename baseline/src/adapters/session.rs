//! The session family: agent working directories, so a frontend can
//! show what is running where. One built-in implementation, a
//! filesystem scan (Task 17).

use super::{AdapterError, ProjectContext};
use crate::adapters::artifact::{outermost_dirs, scan_glob};
use crate::freshness::Observed;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// One agent session directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// How long a session may go without a file changing before a frontend
/// should call it idle.
pub const DEFAULT_IDLE_AFTER: Duration = Duration::from_secs(300);

/// Reports agent session directories by scanning the manifest's
/// `sessions.watch` glob.
pub struct FilesystemSessionAdapter {
    watch: String,
}

impl FilesystemSessionAdapter {
    /// An adapter scanning `watch`, relative to the project root.
    pub fn new(watch: impl Into<String>) -> Self {
        Self {
            watch: watch.into(),
        }
    }
}

/// The newest modification time anywhere under `dir`, falling back to
/// the directory's own when it is empty or unreadable.
fn newest_mtime(dir: &std::path::Path) -> SystemTime {
    let own = std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .fold(own, |newest, m| newest.max(m))
}

impl SessionAdapter for FilesystemSessionAdapter {
    fn source_name(&self) -> String {
        "session:filesystem".into()
    }

    fn scan(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<Vec<Session>>, AdapterError> {
        let mut sessions = Vec::new();
        for path in outermost_dirs(scan_glob(&ctx.root, &self.watch)?) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            sessions.push(Session {
                name,
                last_activity: newest_mtime(&path),
                path,
            });
        }
        Ok(Observed::watched(sessions, now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::ProjectContext;
    use std::time::{Duration, SystemTime};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    /// Builds `.claude/worktrees/<name>/...` for each entry.
    fn tree(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for relative in files {
            let path = dir.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
        }
        dir
    }

    #[test]
    fn each_matching_directory_becomes_one_session_named_for_itself() {
        let dir = tree(&[
            ".claude/worktrees/parallax-baseline/src/lib.rs",
            ".claude/worktrees/widget-audit/notes.md",
        ]);
        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let mut sessions = a
            .scan(&ProjectContext::new("ttui", dir.path()), at(0))
            .unwrap()
            .value;
        sessions.sort_by(|x, y| x.name.cmp(&y.name));
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "parallax-baseline");
        assert_eq!(sessions[1].name, "widget-audit");
    }

    #[test]
    fn files_matching_the_glob_are_not_sessions() {
        let dir = tree(&[".claude/worktrees/stray.txt", ".claude/worktrees/real/a.rs"]);
        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let sessions = a
            .scan(&ProjectContext::new("ttui", dir.path()), at(0))
            .unwrap()
            .value;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "real");
    }

    /// A worktree's own mtime barely moves while an agent edits files
    /// inside it, so reading only the directory would report every
    /// active session as idle.
    #[test]
    fn last_activity_is_the_newest_mtime_anywhere_inside_the_session() {
        let dir = tree(&[".claude/worktrees/s/deep/nested/file.rs"]);
        let session_dir = dir.path().join(".claude/worktrees/s");
        let inner = session_dir.join("deep/nested/file.rs");
        let inner_mtime = std::fs::metadata(&inner).unwrap().modified().unwrap();

        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let sessions = a
            .scan(&ProjectContext::new("ttui", dir.path()), at(0))
            .unwrap()
            .value;
        assert!(
            sessions[0].last_activity >= inner_mtime,
            "activity must reflect the deepest file, not the directory"
        );
    }

    /// The same exposure the capture adapter has: a `**` watch matches
    /// at every depth, and a worktree is full of directories. A session
    /// is the worktree, not every folder inside it.
    #[test]
    fn a_directory_inside_a_session_is_not_a_session_of_its_own() {
        let dir = tree(&[".claude/worktrees/widget-audit/src/widgets/list.rs"]);
        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/**");
        let sessions = a
            .scan(&ProjectContext::new("ttui", dir.path()), at(0))
            .unwrap()
            .value;
        assert_eq!(sessions.len(), 1, "one worktree, not one per folder");
        assert_eq!(sessions[0].name, "widget-audit");
    }

    #[test]
    fn a_session_is_active_while_within_the_idle_window_and_not_after() {
        let session = Session {
            name: "s".into(),
            path: std::path::PathBuf::from("/tmp/s"),
            last_activity: at(100),
        };
        assert!(session.is_active(at(200), Duration::from_secs(300)));
        assert!(!session.is_active(at(500), Duration::from_secs(300)));
    }

    #[test]
    fn the_default_idle_window_is_five_minutes() {
        assert_eq!(DEFAULT_IDLE_AFTER, Duration::from_secs(300));
    }

    #[test]
    fn a_project_with_no_session_directory_yields_an_empty_scan_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let sessions = a
            .scan(&ProjectContext::new("ttui", dir.path()), at(0))
            .unwrap()
            .value;
        assert!(sessions.is_empty());
    }

    #[test]
    fn sessions_read_from_disk_are_live() {
        let dir = tree(&[".claude/worktrees/s/a.rs"]);
        let mut a = FilesystemSessionAdapter::new(".claude/worktrees/*");
        let observed = a
            .scan(&ProjectContext::new("ttui", dir.path()), at(0))
            .unwrap();
        assert_eq!(
            observed.freshness(at(9999)),
            crate::freshness::Freshness::Live
        );
    }
}
