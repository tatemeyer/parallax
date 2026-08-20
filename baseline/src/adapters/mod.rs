//! The four adapter families. Each family is a trait; the built-in
//! implementations live behind it, and a frontend may register its own.
//! Every method takes an injected `now` so nothing here needs a wall
//! clock, and every trait is object-safe so aggregation can hold them
//! as `Box<dyn _>`.

pub mod artifact;
pub mod factory;
pub mod http;
pub mod session;
pub mod verification;
pub mod work;

use std::path::PathBuf;

/// Anything that can go wrong reaching a source.
#[derive(Debug)]
pub enum AdapterError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// A non-success HTTP response.
    Http {
        /// The status code.
        status: u16,
        /// The body or reason phrase, trimmed.
        message: String,
    },
    /// The source responded but the response could not be understood.
    Parse(String),
    /// The manifest asked for something with no implementation.
    Unsupported(String),
    /// The source did not respond in time.
    Timeout(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdapterError::Io(e) => write!(f, "reading source: {e}"),
            // A body is usual and worth showing; an empty one is not a
            // reason to print a colon with nothing after it. Tailscale's
            // own 502 arrives with `Content-Length: 0`, which is exactly
            // the case an operator most needs a sentence for.
            AdapterError::Http { status, message } if message.is_empty() => {
                write!(f, "http {status}")
            }
            AdapterError::Http { status, message } => write!(f, "http {status}: {message}"),
            AdapterError::Parse(m) => write!(f, "unreadable response: {m}"),
            AdapterError::Unsupported(m) => write!(f, "no implementation for {m}"),
            AdapterError::Timeout(m) => write!(f, "timed out: {m}"),
        }
    }
}
impl std::error::Error for AdapterError {}

impl From<std::io::Error> for AdapterError {
    fn from(e: std::io::Error) -> Self {
        AdapterError::Io(e)
    }
}

/// What every adapter needs to know about the project it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    /// The project's short name.
    pub name: String,
    /// Absolute path to the project root.
    pub root: PathBuf,
    /// The work adapter's `owner/name`, when one is declared.
    pub repo: Option<String>,
}

impl ProjectContext {
    /// A context for a project rooted at `root`.
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            root: root.into(),
            repo: None,
        }
    }

    /// Attaches the work adapter's repository argument.
    pub fn with_repo(mut self, repo: impl Into<String>) -> Self {
        self.repo = Some(repo.into());
        self
    }

    /// Resolves a manifest-relative path against the project root.
    pub fn resolve(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::work::{ChecksSummary, WorkAdapter, WorkSnapshot};
    use crate::freshness::Observed;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_project_context_resolves_relative_paths_against_the_project_root() {
        let ctx = ProjectContext::new("ttui", "<projects-root>/TTUI");
        assert_eq!(
            ctx.resolve(".plumb/config.yaml"),
            std::path::PathBuf::from("<projects-root>/TTUI/.plumb/config.yaml")
        );
    }

    #[test]
    fn a_project_context_carries_an_optional_repo() {
        let ctx = ProjectContext::new("ttui", "/tmp").with_repo("tatemeyer/ttui");
        assert_eq!(ctx.repo.as_deref(), Some("tatemeyer/ttui"));
        assert_eq!(ProjectContext::new("x", "/tmp").repo, None);
    }

    #[test]
    fn checks_are_green_only_when_something_passed_and_nothing_failed_or_pends() {
        assert!(ChecksSummary {
            passed: 4,
            failed: 0,
            pending: 0
        }
        .is_green());
        assert!(!ChecksSummary {
            passed: 4,
            failed: 1,
            pending: 0
        }
        .is_green());
        assert!(!ChecksSummary {
            passed: 4,
            failed: 0,
            pending: 1
        }
        .is_green());
        assert!(
            !ChecksSummary::none().is_green(),
            "no checks at all is not green"
        );
        assert_eq!(
            ChecksSummary {
                passed: 2,
                failed: 1,
                pending: 3
            }
            .total(),
            6
        );
    }

    #[test]
    fn adapter_errors_render_a_one_line_message_naming_the_cause() {
        let e = AdapterError::Http {
            status: 403,
            message: "rate limit exceeded".into(),
        };
        assert!(e.to_string().contains("403"));
        assert!(e.to_string().contains("rate limit exceeded"));
        assert!(AdapterError::Unsupported("window capture".into())
            .to_string()
            .contains("window capture"));
    }

    /// Tailscale's 502 arrives with `Content-Length: 0`, so this is not
    /// hypothetical: it is what a stopped probe behind a live proxy
    /// actually renders as, and it read `http 502:` — a colon promising
    /// a reason that never came.
    #[test]
    fn a_status_with_no_body_does_not_render_a_dangling_colon() {
        let e = AdapterError::Http {
            status: 502,
            message: String::new(),
        };
        assert_eq!(e.to_string(), "http 502");
    }

    struct StubWork;
    impl WorkAdapter for StubWork {
        fn source_name(&self) -> String {
            "stub".into()
        }
        fn poll(
            &mut self,
            _ctx: &ProjectContext,
            now: SystemTime,
        ) -> Result<Observed<WorkSnapshot>, AdapterError> {
            Ok(Observed::polled(
                WorkSnapshot { items: vec![] },
                now,
                Duration::from_secs(30),
            ))
        }
    }

    /// Aggregation stores adapters as trait objects and a frontend may
    /// register one this crate has never heard of, so object safety is a
    /// contract, not an accident.
    #[test]
    fn a_work_adapter_is_usable_as_a_trait_object() {
        let mut adapters: Vec<Box<dyn WorkAdapter>> = vec![Box::new(StubWork)];
        let ctx = ProjectContext::new("x", "/tmp");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let observed = adapters[0].poll(&ctx, now).unwrap();
        assert_eq!(observed.observed_at, now);
        assert!(observed.value.items.is_empty());
    }
}
