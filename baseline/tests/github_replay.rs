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
const HEAD_139: &str = "9f1e2d3c4b5a60718293a4b5c6d7e8f90a1b2c3d";
const HEAD_142: &str = "1a7d51c9f0e2b3a4d5c6e7f8091a2b3c4d5e6f70";
const HEAD_143: &str = "0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/github")
        .join(name)
}

/// The recorded feeds deliberately still contain a closed issue and a
/// merged pull request, even though `state=open` would not return them.
/// They cover `parse_state`'s `merged_at` arm and the guard that keeps a
/// poll from asking about finished work's check runs — both of which
/// must stay correct if the query ever widens again.
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
        Some("W/\"checks-1\""),
    )
    .unwrap();
    t.insert(
        check_runs_url(REPO, HEAD_143),
        r#"{"total_count":0,"check_runs":[]}"#,
        None,
    );
    // Served, but a poll must never ask for it: 139 is merged.
    t.insert(
        check_runs_url(REPO, HEAD_139),
        r#"{"total_count":4,"check_runs":[]}"#,
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
    assert_eq!(snapshot.items.len(), 6, "3 issues + 3 pulls");
    let numbers: Vec<u64> = snapshot.items.iter().map(|i| i.number).collect();
    assert_eq!(numbers, vec![134, 140, 141, 139, 142, 143]);
}

#[test]
fn issue_and_pull_request_kinds_are_distinguished() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[0].kind, WorkKind::Issue);
    assert_eq!(items[3].kind, WorkKind::PullRequest);
    assert_eq!(items[5].kind, WorkKind::PullRequest);
}

#[test]
fn state_maps_including_the_draft_case() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[0].state, WorkState::Closed, "issue 134");
    assert_eq!(items[1].state, WorkState::Open, "issue 140");
    assert_eq!(
        items[3].state,
        WorkState::Merged,
        "pull 139 carries merged_at"
    );
    assert_eq!(items[4].state, WorkState::Open, "pull 142");
    assert_eq!(items[5].state, WorkState::Draft, "pull 143 is a draft");
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
    assert!(items[5].labels.is_empty());
}

#[test]
fn check_runs_are_summarised_per_pull_request_only() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    assert_eq!(items[4].checks.passed, 3);
    assert_eq!(items[4].checks.failed, 0);
    assert_eq!(items[4].checks.pending, 1);
    assert!(!items[4].checks.is_green(), "one check still running");
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

/// Check runs cost one request per pull request. A repository's pulls
/// are overwhelmingly closed — TTUI's feed is 71 of which 1 is open —
/// so asking about all of them costs 73 requests and ~24s per poll,
/// which at the default 30s interval exhausts an authenticated hourly
/// rate limit in about 35 minutes.
#[test]
fn a_poll_asks_about_checks_only_for_work_still_in_flight() {
    let mut a = GithubWorkAdapter::new(transport());
    a.poll(&ctx(), at(0)).unwrap();

    let asked: Vec<&String> = a
        .transport()
        .requests()
        .iter()
        .map(|r| &r.url)
        .filter(|u| u.contains("check-runs"))
        .collect();
    assert_eq!(
        asked.len(),
        2,
        "one per in-flight pull, not per pull: {asked:?}"
    );
    assert!(asked.iter().any(|u| u.contains(HEAD_142)), "open");
    assert!(asked.iter().any(|u| u.contains(HEAD_143)), "draft");
    assert!(
        !asked.iter().any(|u| u.contains(HEAD_139)),
        "merged pull's checks are dead history"
    );
}

/// A merged pull request still appears, with no checks reported.
/// `ChecksSummary::total() == 0` already means "nothing reported", and
/// `WorkState::Merged` is how a frontend tells that apart from pending.
#[test]
fn a_merged_pull_request_is_still_listed_just_without_checks() {
    let mut a = GithubWorkAdapter::new(transport());
    let items = a.poll(&ctx(), at(0)).unwrap().value.items;
    let merged = items
        .iter()
        .find(|i| i.number == 139)
        .expect("139 is listed");
    assert_eq!(merged.state, WorkState::Merged);
    assert_eq!(merged.checks.total(), 0);
}

/// A transport whose issues feed carries no ETag, so every poll rebuilds
/// the snapshot instead of being served from cache — which is what lets
/// the check-runs `304` path be reached at all.
fn transport_with_volatile_issues() -> FixtureTransport {
    let mut t = FixtureTransport::new();
    t.insert_from_file(issues_url(REPO), &fixture("issues.json"), None)
        .unwrap();
    t.insert_from_file(pulls_url(REPO), &fixture("pulls.json"), Some("W/\"p1\""))
        .unwrap();
    t.insert_from_file(
        check_runs_url(REPO, HEAD_142),
        &fixture("check-runs.json"),
        Some("W/\"checks-1\""),
    )
    .unwrap();
    t.insert(
        check_runs_url(REPO, HEAD_143),
        r#"{"total_count":0,"check_runs":[]}"#,
        None,
    );
    t
}

/// GitHub sends an ETag with check runs too, so the second poll's
/// conditional request comes back `304`. That proves the counts are
/// current — reading it as "no checks" would blank an open pull
/// request's status on every poll after the first.
#[test]
fn a_not_modified_check_runs_response_keeps_the_counts_it_confirmed() {
    let mut a = GithubWorkAdapter::new(transport_with_volatile_issues());

    let before = checks_for(&mut a, 142, at(0));
    assert_eq!(before.passed, 3, "first poll reads the counts");

    let after = checks_for(&mut a, 142, at(60));
    assert_eq!(
        after, before,
        "a 304 confirms the counts, it does not clear them"
    );

    // The second poll really did ask conditionally and get a 304 —
    // otherwise this test would pass without exercising anything.
    let conditional = a
        .transport()
        .requests()
        .iter()
        .filter(|r| r.url.contains(HEAD_142) && r.etag.is_some())
        .count();
    assert_eq!(
        conditional, 1,
        "the second check-runs request carried the ETag"
    );
}

fn checks_for(
    adapter: &mut GithubWorkAdapter<FixtureTransport>,
    number: u64,
    now: SystemTime,
) -> parallax_baseline::adapters::work::ChecksSummary {
    adapter
        .poll(&ctx(), now)
        .unwrap()
        .value
        .items
        .iter()
        .find(|i| i.number == number)
        .unwrap_or_else(|| panic!("#{number} is listed"))
        .checks
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
