//! The two guarantees that make the refresh design worth its
//! complexity: a slow adapter never delays a tick, and a refresh cycle
//! never runs a build.

use panopticon::refresh::{Clock, Refresher, Request, Update};
use parallax_baseline::adapters::verification::{
    CheckCost, CommandOutput, CommandRunner, CommandVerificationAdapter, VerificationAdapter,
};
use parallax_baseline::adapters::work::{WorkAdapter, WorkSnapshot};
use parallax_baseline::adapters::{AdapterError, ProjectContext};
use parallax_baseline::freshness::{Observed, DEFAULT_POLL_INTERVAL};
use parallax_baseline::manifest::parse_manifest;
use parallax_baseline::state::ProjectAdapters;
use parallax_baseline::validate::{validate, Validated};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
}

fn validated(name: &str) -> Validated {
    validate(parse_manifest(&format!("project:\n  name: {name}\n  root: /tmp/{name}\n")).unwrap())
        .unwrap()
}

/// A work adapter that takes its time, the way a hung socket does.
struct SlowWork {
    delay: Duration,
}

impl WorkAdapter for SlowWork {
    fn source_name(&self) -> String {
        "work:slow".into()
    }
    fn poll(
        &mut self,
        _ctx: &ProjectContext,
        now: SystemTime,
    ) -> Result<Observed<WorkSnapshot>, AdapterError> {
        std::thread::sleep(self.delay);
        Ok(Observed::polled(
            WorkSnapshot::default(),
            now,
            DEFAULT_POLL_INTERVAL,
        ))
    }
}

/// A runner that counts what it was asked to run.
///
/// The plan called for one that panics. A panic on the refresh thread
/// would unwind that thread and be invisible to the test, so the
/// guarantee is observed rather than asserted by explosion — same
/// property, actually visible.
#[derive(Clone)]
struct CountingRunner {
    calls: Arc<AtomicUsize>,
}

impl CommandRunner for CountingRunner {
    fn run(&mut self, _command: &str, _cwd: &Path) -> std::io::Result<CommandOutput> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(CommandOutput {
            status: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

/// The UI thread must never wait on an adapter. Driving the refresher
/// while a five-second poll is in flight, the caller keeps going.
#[test]
fn a_slow_adapter_never_delays_the_caller() {
    let project = validated("slow");
    let mut adapters = ProjectAdapters::new();
    adapters.work = Some(Box::new(SlowWork {
        delay: Duration::from_secs(5),
    }));

    let refresher = Refresher::spawn(vec![(project, adapters)], Clock::Frozen(at(0)));
    refresher.request(Request::RefreshReads);

    // Stand in for the event loop: drain, do nothing, come back.
    let started = std::time::Instant::now();
    let mut drains = 0;
    while started.elapsed() < Duration::from_millis(300) {
        assert!(refresher.drain().is_empty(), "nothing has arrived yet");
        drains += 1;
    }

    assert!(
        drains > 100,
        "the caller kept working while the adapter slept: {drains} drains"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "and did not wait for the five-second poll"
    );
}

/// The spec's rule, made unbreakable: a refresh cycle is provably
/// incapable of running `cargo test` on the machine running the agent
/// sessions.
#[test]
fn a_refresh_cycle_runs_no_command_and_an_explicit_request_does() {
    let calls = Arc::new(AtomicUsize::new(0));
    let runner = CountingRunner {
        calls: Arc::clone(&calls),
    };

    let mut adapters = ProjectAdapters::new();
    adapters
        .verification
        .push(Box::new(CommandVerificationAdapter::new(
            "lint",
            "cargo clippy",
            runner.clone(),
        )));
    adapters
        .verification
        .push(Box::new(CommandVerificationAdapter::new(
            "tests",
            "cargo test",
            runner,
        )));

    let refresher = Refresher::spawn(vec![(validated("ttui"), adapters)], Clock::Frozen(at(0)));

    // Five refresh cycles, each allowed to finish before the next is
    // asked for.
    //
    // They used to be fired back-to-back and waited on together. That
    // now proves less than it looks: read-refreshes that queue up while
    // one is running collapse into a single sweep, so five requests
    // would mean one cycle. The claim here is about five *cycles* each
    // running no build, so they are separated rather than counted.
    for _ in 0..5 {
        refresher.request(Request::RefreshReads);
        wait_for(&refresher, |updates| {
            updates.iter().any(|u| matches!(u, Update::Project(_)))
        });
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "five refresh cycles ran no build"
    );

    // And the refusal is a gate, not a wall.
    refresher.request(Request::RunChecks {
        project: "ttui".into(),
    });
    wait_for(&refresher, |updates| {
        updates
            .iter()
            .any(|u| matches!(u, Update::ChecksRan { .. }))
    });
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "asking runs both declared checks, once each"
    );
}

/// Both declared checks run a build, so the cadence has nothing to poll
/// and the pane will read "not run this session" until asked.
#[test]
fn build_checks_are_named_so_the_pane_can_say_they_have_not_run() {
    let mut adapters = ProjectAdapters::new();
    adapters
        .verification
        .push(Box::new(CommandVerificationAdapter::new(
            "lint",
            "cargo clippy",
            CountingRunner {
                calls: Arc::new(AtomicUsize::new(0)),
            },
        )));
    let refresher = Refresher::spawn(vec![(validated("ttui"), adapters)], Clock::Frozen(at(0)));
    assert_eq!(refresher.executor_kinds("ttui"), ["lint".to_string()]);
    assert!(
        refresher.executor_kinds("unregistered").is_empty(),
        "a project nobody registered claims nothing"
    );
}

#[test]
fn the_two_built_in_check_kinds_land_on_the_side_they_should() {
    let counting = CountingRunner {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let command = CommandVerificationAdapter::new("tests", "cargo test", counting);
    assert_eq!(command.cost(), CheckCost::Execute);

    let plumb = parallax_baseline::adapters::verification::PlumbVerificationAdapter::new(
        "perceptual",
        "/tmp/runs",
    );
    assert_eq!(plumb.cost(), CheckCost::Read);
}

/// Drains until `done` says so, or fails rather than hanging forever.
fn wait_for(refresher: &Refresher, done: impl Fn(&[Update]) -> bool) {
    let started = std::time::Instant::now();
    let mut seen = Vec::new();
    while started.elapsed() < Duration::from_secs(10) {
        seen.extend(refresher.drain());
        if done(&seen) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the refresh thread never produced what was expected");
}
