//! The GitHub work adapter, replayed against recorded API responses.
//! Live GitHub access is real-external-service exempt; this is what
//! covers the adapter instead.

use parallax_baseline::adapters::http::FixtureTransport;
use parallax_baseline::adapters::work::{
    check_runs_url, issues_url, pulls_url, GithubWorkAdapter, WorkAdapter, WorkKind, WorkState,
};
use parallax_baseline::adapters::ProjectContext;
use parallax_baseline::freshness::{Freshness, DEFAULT_POLL_INTERVAL};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const REPO: &str = "tatemeyer/ttui";
const HEAD_142: &str = "1a7d51c9f0e2b3a4d5c6e7f8091a2b3c4d5e6f70";
const HEAD_143: &str = "0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/github")
        .join(name)
}

fn transport() -> FixtureTransport {
    let mut t = FixtureTransport::new();
    t.insert_from_file(
        issues_url(REPO),
        &fixture("issues.json"),
        Some("W/\"issues-1\""),
    )
    .unwrap();
    t.insert_from_file(
        pulls_url(REPO),
        &fixture("pulls.json"),
        Some("W/\"pulls-1\""),
    )
    .unwrap();
    t.insert_from_file(
        check_runs_url(REPO, HEAD_142),
        &fixture("check-runs.json"),
        None,
    )
    .unwrap();
    t.insert(
        check_runs_url(REPO, HEAD_143),
        r#"{"total_count":0,"check_runs":[]}"#,
        None,
    );
    t
}

fn ctx() -> ProjectContext {
    ProjectContext::new("ttui", "<projects-root>/TTUI").with_repo(REPO)
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

#[test]
fn a_poll_returns_both_issues_and_pull_requests() {
    let mut a = GithubWorkAdapter::new(transport());
    let snapshot = a.poll(&ctx(), at(0)).unwrap().value;
    assert_eq!(snapshot.items.len(), 5, "3 issues + 2 pulls");
    let numbers: Vec<u64> = snapshot.items.iter().map(|i| i.number).collect();
    assert_eq!(numbers, vec![134, 140, 141, 142, 143]);
}

#[test]
fn issue_and_pull_request_kinds_are_distinguished() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[0].kind, WorkKind::Issue);
    assert_eq!(items[3].kind, WorkKind::PullRequest);
}

#[test]
fn state_maps_including_the_draft_case() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[0].state, WorkState::Closed, "issue 134");
    assert_eq!(items[1].state, WorkState::Open, "issue 140");
    assert_eq!(items[3].state, WorkState::Open, "pull 142");
    assert_eq!(items[4].state, WorkState::Draft, "pull 143 is a draft");
}

/// Labels are carried verbatim. Projection is `autonomy`'s job, and
/// mixing the two here would put policy in an adapter.
#[test]
fn labels_are_carried_verbatim_and_unfiltered() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(
        items[0].labels,
        vec!["semver:minor".to_string(), "gated".to_string()]
    );
    assert_eq!(items[2].labels, vec!["needs-intent".to_string()]);
    assert!(items[4].labels.is_empty());
}

#[test]
fn check_runs_are_summarised_per_pull_request_only() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[3].checks.passed, 3);
    assert_eq!(items[3].checks.failed, 0);
    assert_eq!(items[3].checks.pending, 1);
    assert!(!items[3].checks.is_green(), "one check still running");
    assert_eq!(items[0].checks.total(), 0, "an issue has no checks");
}

#[test]
fn a_poll_is_stamped_polled_at_the_configured_interval() {
    let mut a = GithubWorkAdapter::new(transport());
    let observed = a.poll(&ctx(), at(0)).unwrap();
    assert_eq!(observed.observed_at, at(0));
    assert_eq!(
        observed.freshness(at(10)),
        Freshness::Fresh {
            age: Duration::from_secs(10)
        }
    );
    assert!(
        observed.freshness(at(31)).is_stale(),
        "default interval is {DEFAULT_POLL_INTERVAL:?}"
    );
}

/// The spec specifies ETag-conditional polling; this is what proves it
/// actually happens rather than being described in a doc comment.
#[test]
fn a_second_poll_sends_the_etag_from_the_first() {
    let mut a = GithubWorkAdapter::new(transport());
    a.poll(&ctx(), at(0)).unwrap();
    a.poll(&ctx(), at(60)).unwrap();
    let sent: Vec<Option<String>> = a
        .transport()
        .requests()
        .iter()
        .filter(|r| r.url == issues_url(REPO))
        .map(|r| r.etag.clone())
        .collect();
    assert_eq!(sent, vec![None, Some("W/\"issues-1\"".to_string())]);
}

/// A 304 means the cached snapshot is current now — the value is
/// unchanged and its observation time advances.
#[test]
fn a_not_modified_response_refreshes_the_observation_without_refetching() {
    let mut a = GithubWorkAdapter::new(transport());
    let first = a.poll(&ctx(), at(0)).unwrap();
    let second = a.poll(&ctx(), at(60)).unwrap();
    assert_eq!(first.value, second.value);
    assert_eq!(second.observed_at, at(60));
    assert_eq!(
        second.freshness(at(60)),
        Freshness::Fresh {
            age: Duration::ZERO
        }
    );
}

#[test]
fn a_rate_limit_response_surfaces_as_an_http_error_rather_than_an_empty_snapshot() {
    let mut transport = transport();
    transport.fail_next(parallax_baseline::adapters::AdapterError::Http {
        status: 403,
        message: "API rate limit exceeded".into(),
    });
    let mut a = GithubWorkAdapter::new(transport);
    let err = a.poll(&ctx(), at(0)).unwrap_err().to_string();
    assert!(
        err.contains("403") && err.contains("rate limit"),
        "got {err}"
    );
}

#[test]
fn polling_without_a_repo_in_the_context_is_a_clear_error() {
    let mut a = GithubWorkAdapter::new(transport());
    let err = a
        .poll(&ProjectContext::new("ttui", "/tmp"), at(0))
        .unwrap_err()
        .to_string();
    assert!(err.contains("repo"), "got {err}");
}
