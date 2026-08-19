//! The work family: issues, pull requests, their labels, and their
//! check status. One built-in implementation (`github`, Task 12).

use super::http::{HttpRequest, HttpResponse, HttpTransport};
use super::{AdapterError, ProjectContext};
use crate::freshness::{Observed, DEFAULT_POLL_INTERVAL};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Whether a work item is an issue or a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkKind {
    /// An issue.
    Issue,
    /// A pull request.
    PullRequest,
}

/// Where a work item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkState {
    /// Open and ready.
    Open,
    /// Open but marked draft.
    Draft,
    /// Closed without merging.
    Closed,
    /// Merged.
    Merged,
}

/// How a work item's checks stand. Deliberately a count, not a verdict —
/// what "green enough" means is a policy question, and the manifest's
/// autonomy axes are where policy lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChecksSummary {
    /// Checks that succeeded.
    pub passed: usize,
    /// Checks that failed.
    pub failed: usize,
    /// Checks still running or queued.
    pub pending: usize,
}

impl ChecksSummary {
    /// A summary for an item with no checks reported.
    pub fn none() -> Self {
        Self::default()
    }

    /// How many checks were reported in total.
    pub fn total(&self) -> usize {
        self.passed + self.failed + self.pending
    }

    /// Whether every reported check passed and at least one ran.
    pub fn is_green(&self) -> bool {
        self.passed > 0 && self.failed == 0 && self.pending == 0
    }
}

/// One issue or pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// The item's number in its repository.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Issue or pull request.
    pub kind: WorkKind,
    /// Where it stands.
    pub state: WorkState,
    /// Its labels, verbatim — projection happens in `autonomy`.
    pub labels: Vec<String>,
    /// Its check status.
    pub checks: ChecksSummary,
    /// A link a frontend can open.
    pub url: String,
    /// The source's own last-updated string, carried opaquely for
    /// display. Freshness of the *observation* lives in `Observed`.
    pub updated_at: String,
}

/// Every work item one poll returned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkSnapshot {
    /// The items, in the order the source returned them.
    pub items: Vec<WorkItem>,
}

/// A source of work items.
pub trait WorkAdapter {
    /// A short label naming this source, for degradation reporting.
    fn source_name(&self) -> String;

    /// Fetches the current work items as of `now`.
    fn poll(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<WorkSnapshot>, AdapterError>;
}

/// The issues endpoint for a repository. GitHub returns pull requests
/// here too; they carry a `pull_request` key and are skipped, because
/// `pulls_url` returns them with their head SHA.
pub fn issues_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/issues?state=all&per_page=100")
}

/// The pull requests endpoint for a repository.
pub fn pulls_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/pulls?state=all&per_page=100")
}

/// The check-runs endpoint for one commit.
pub fn check_runs_url(repo: &str, sha: &str) -> String {
    format!("https://api.github.com/repos/{repo}/commits/{sha}/check-runs")
}

/// Reads issues, pull requests, and check runs from GitHub, polling
/// with ETag-conditional requests so an unchanged feed costs no rate
/// limit.
pub struct GithubWorkAdapter<T: HttpTransport> {
    transport: T,
    interval: Duration,
    etags: HashMap<String, String>,
    cached: Option<WorkSnapshot>,
    /// Last-seen check summary per head SHA. A conditional request that
    /// comes back `304` proves the summary is current; without somewhere
    /// to keep it, every poll after the first would report an open pull
    /// request as having no checks at all.
    checks: HashMap<String, ChecksSummary>,
}

impl<T: HttpTransport> GithubWorkAdapter<T> {
    /// A GitHub adapter polling at `DEFAULT_POLL_INTERVAL`.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            interval: DEFAULT_POLL_INTERVAL,
            etags: HashMap::new(),
            cached: None,
            checks: HashMap::new(),
        }
    }

    /// Overrides the poll interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// The transport, for asserting what was requested.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Fetches a URL conditionally. `Ok(None)` means "not modified".
    fn fetch(&mut self, url: &str) -> Result<Option<String>, AdapterError> {
        let request = HttpRequest {
            url: url.to_string(),
            etag: self.etags.get(url).cloned(),
        };
        match self.transport.get(&request)? {
            HttpResponse::NotModified => Ok(None),
            HttpResponse::Ok { body, etag } => {
                match etag {
                    Some(e) => self.etags.insert(url.to_string(), e),
                    None => self.etags.remove(url),
                };
                Ok(Some(body))
            }
        }
    }

    /// Re-fetches a URL ignoring the stored ETag. Needed when one
    /// endpoint changed and another did not: the snapshot is rebuilt as
    /// a whole, so a `304` on one half still needs that half's body.
    fn refetch_unconditionally(&mut self, url: &str) -> Result<String, AdapterError> {
        match self.transport.get(&HttpRequest {
            url: url.to_string(),
            etag: None,
        })? {
            HttpResponse::Ok { body, etag } => {
                if let Some(e) = etag {
                    self.etags.insert(url.to_string(), e);
                }
                Ok(body)
            }
            HttpResponse::NotModified => Err(AdapterError::Parse(
                "server returned 304 to an unconditional request".into(),
            )),
        }
    }
}

fn as_labels(value: &serde_json::Value) -> Vec<String> {
    value["labels"]
        .as_array()
        .map(|xs| {
            xs.iter()
                .filter_map(|l| l["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn str_field(value: &serde_json::Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

fn parse_state(value: &serde_json::Value) -> WorkState {
    if value["draft"].as_bool().unwrap_or(false) {
        return WorkState::Draft;
    }
    match value["state"].as_str() {
        Some("closed") if value["merged_at"].is_string() => WorkState::Merged,
        Some("closed") => WorkState::Closed,
        _ => WorkState::Open,
    }
}

fn parse_item(
    value: &serde_json::Value,
    kind: WorkKind,
    state: WorkState,
    checks: ChecksSummary,
) -> Option<WorkItem> {
    Some(WorkItem {
        number: value["number"].as_u64()?,
        title: str_field(value, "title"),
        kind,
        state,
        labels: as_labels(value),
        checks,
        url: str_field(value, "html_url"),
        updated_at: str_field(value, "updated_at"),
    })
}

fn parse_checks(body: &str) -> Result<ChecksSummary, AdapterError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| AdapterError::Parse(e.to_string()))?;
    let mut summary = ChecksSummary::none();
    for run in value["check_runs"].as_array().unwrap_or(&Vec::new()) {
        match (run["status"].as_str(), run["conclusion"].as_str()) {
            (Some("completed"), Some("success")) => summary.passed += 1,
            (Some("completed"), _) => summary.failed += 1,
            _ => summary.pending += 1,
        }
    }
    Ok(summary)
}

impl<T: HttpTransport> WorkAdapter for GithubWorkAdapter<T> {
    fn source_name(&self) -> String {
        "work:github".into()
    }

    fn poll(
        &mut self,
        ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<WorkSnapshot>, AdapterError> {
        let repo = ctx.repo.clone().ok_or_else(|| {
            AdapterError::Unsupported(format!(
                "project `{}` has no work.repo, so there is nothing to poll",
                ctx.name
            ))
        })?;

        let issues_body = self.fetch(&issues_url(&repo))?;
        let pulls_body = self.fetch(&pulls_url(&repo))?;

        if issues_body.is_none() && pulls_body.is_none() {
            if let Some(cached) = self.cached.clone() {
                return Ok(Observed::polled(cached, now, self.interval));
            }
        }

        let mut items = Vec::new();

        let issues_json = match issues_body {
            Some(body) => body,
            None => self.refetch_unconditionally(&issues_url(&repo))?,
        };
        let issues: serde_json::Value =
            serde_json::from_str(&issues_json).map_err(|e| AdapterError::Parse(e.to_string()))?;
        for value in issues.as_array().unwrap_or(&Vec::new()) {
            // GitHub returns pull requests from the issues endpoint too;
            // `pulls_url` covers them with their head SHA, so skip them
            // here rather than reporting each one twice.
            if value.get("pull_request").is_some() {
                continue;
            }
            if let Some(item) = parse_item(
                value,
                WorkKind::Issue,
                parse_state(value),
                ChecksSummary::none(),
            ) {
                items.push(item);
            }
        }

        let pulls_json = match pulls_body {
            Some(body) => body,
            None => self.refetch_unconditionally(&pulls_url(&repo))?,
        };
        let pulls: serde_json::Value =
            serde_json::from_str(&pulls_json).map_err(|e| AdapterError::Parse(e.to_string()))?;
        for value in pulls.as_array().unwrap_or(&Vec::new()) {
            let state = parse_state(value);
            // Check runs cost one request per pull request, and a closed
            // or merged one's checks are dead history. TTUI's feed is 71
            // pulls of which 1 is open: asking about all of them costs 73
            // requests and ~24s per poll, which at the default 30s
            // interval exhausts an authenticated hourly rate limit in
            // about 35 minutes. Only work still in flight is asked about.
            let in_flight = matches!(state, WorkState::Open | WorkState::Draft);
            let checks = match (in_flight, value["head"]["sha"].as_str()) {
                (true, Some(sha)) => match self.fetch(&check_runs_url(&repo, sha))? {
                    Some(body) => {
                        let summary = parse_checks(&body)?;
                        self.checks.insert(sha.to_string(), summary);
                        summary
                    }
                    // `304`: what we already hold is current. Falling back
                    // to `none()` here would zero an open pull request's
                    // checks on every poll after the first.
                    None => self.checks.get(sha).copied().unwrap_or_default(),
                },
                _ => ChecksSummary::none(),
            };
            if let Some(item) = parse_item(value, WorkKind::PullRequest, state, checks) {
                items.push(item);
            }
        }

        let snapshot = WorkSnapshot { items };
        self.cached = Some(snapshot.clone());
        Ok(Observed::polled(snapshot, now, self.interval))
    }
}
