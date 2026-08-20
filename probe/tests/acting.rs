//! Acting on this machine because another one asked.
//!
//! The claims that are only true end to end are tested end to end: over
//! a real loopback socket, with the `RemoteExecutor` a cockpit uses, so
//! the status codes and the JSON are exercised by the code that ships.
//! The claims that are about a decision rather than a wire — asking
//! twice, refusing a project this machine has not got — go straight at
//! `Control`, where adding a socket would only make the failure harder
//! to read.

use parallax_baseline::actions::wire::{ActionId, ActionRequest, ActionStatus, ProbeRun};
use parallax_baseline::actions::{
    Action, ActionExecutor, ActionOutcome, Authorized, Confirmation, RemoteExecutor, Standing,
    Submitted,
};
use parallax_baseline::adapters::factory::AdapterConfig;
use parallax_baseline::adapters::http::UreqTransport;
use parallax_baseline::registry::Registry;
use parallax_probe::control::{Audit, Control, Executors};
use parallax_probe::server::{Probe, Serving};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// An executor that records what it ran and, optionally, waits to be let
/// go — which is how "the submission returned before the action
/// finished" is asserted without a sleep standing in for the claim.
struct Scripted {
    ran: Arc<Mutex<Vec<Action>>>,
    gate: Option<Receiver<()>>,
    fail: bool,
}

impl Scripted {
    fn new(ran: Arc<Mutex<Vec<Action>>>) -> Self {
        Self {
            ran,
            gate: None,
            fail: false,
        }
    }

    fn failing(ran: Arc<Mutex<Vec<Action>>>) -> Self {
        Self {
            ran,
            gate: None,
            fail: true,
        }
    }

    fn gated(ran: Arc<Mutex<Vec<Action>>>) -> (Self, Sender<()>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                ran,
                gate: Some(rx),
                fail: false,
            },
            tx,
        )
    }
}

impl ActionExecutor for Scripted {
    fn execute(
        &mut self,
        authorized: Authorized<'_>,
    ) -> Result<ActionOutcome, parallax_baseline::actions::ActionError> {
        if let Some(gate) = &self.gate {
            // Held until the test says so. A `RecvError` means the test
            // dropped the sender, which is a released gate too.
            let _ = gate.recv();
        }
        let action = authorized.action().clone();
        self.ran.lock().unwrap().push(action.clone());
        if self.fail {
            return Err(parallax_baseline::actions::ActionError::NotSupported(
                "this executor was told to fail".into(),
            ));
        }
        Ok(ActionOutcome {
            summary: action.summary(),
            effects: Vec::new(),
        })
    }
}

/// An audit that keeps its lines so a test can read them.
#[derive(Clone, Default)]
struct Recorded(Arc<Mutex<Vec<String>>>);

impl Audit for Recorded {
    fn line(&mut self, entry: &str) {
        self.0.lock().unwrap().push(entry.to_string());
    }
}

fn merge() -> Action {
    Action::MergePullRequest {
        project: "sesh".into(),
        number: 12,
    }
}

fn label() -> Action {
    Action::SetAutonomyLabel {
        project: "sesh".into(),
        item: 7,
        label: "gated".into(),
    }
}

fn executors(name: &str, executor: Scripted) -> Executors {
    let mut map = Executors::new();
    map.insert(name.to_string(), Box::new(executor));
    map
}

fn control_over(executors: Executors) -> Control {
    Control::start(
        executors,
        Box::new(Recorded::default()),
        ProbeRun::new("r1"),
    )
}

fn request(id: &str, action: Action, confirmed: bool) -> ActionRequest {
    let confirmation = confirmed.then(|| Confirmation::of(&action));
    ActionRequest::new(ActionId::new(id), "desktop", action, confirmation.as_ref())
}

/// Serves an empty registry so `/state` still answers, with control
/// either on or off. Returns the base URL.
fn spawn(control: Option<Control>) -> String {
    let probe = Probe::bind(0).expect("binds an ephemeral loopback port");
    let url = probe.url();
    std::thread::spawn(move || {
        let registry = Registry::default();
        probe.serve(&Serving {
            registry: &registry,
            config: &AdapterConfig::default(),
            peer: "pi5",
            control: control.as_ref(),
        });
    });
    url
}

fn remote(url: String) -> RemoteExecutor<UreqTransport> {
    RemoteExecutor::new(UreqTransport::new(), url, "pi5", "desktop", 1)
}

/// Polls until `f` holds, or gives up. Bounded so a broken worker fails
/// the test rather than hanging the suite.
fn until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// The claim that makes the whole shape work: the probe answers before
/// the action is done. The executor here cannot finish until this test
/// lets it, so a submission that waited for it would never return.
#[test]
fn a_submission_returns_before_the_action_finishes() {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let (executor, release) = Scripted::gated(Arc::clone(&ran));
    let control = control_over(executors("sesh", executor));

    let reply = control.submit(request("desktop-1-1", label(), false));
    assert!(
        matches!(
            reply,
            parallax_baseline::actions::wire::SubmitReply::Accepted { .. }
        ),
        "got {reply:?}"
    );
    assert!(
        ran.lock().unwrap().is_empty(),
        "the action ran before the submission was answered"
    );
    assert_eq!(
        control.status(&ActionId::new("desktop-1-1")).status,
        ActionStatus::Running
    );

    release.send(()).unwrap();
    assert!(
        until(|| matches!(
            control.status(&ActionId::new("desktop-1-1")).status,
            ActionStatus::Done { .. }
        )),
        "the released action never finished"
    );
    assert_eq!(ran.lock().unwrap().len(), 1);
}

/// Retrying is what an operator does after an answer goes missing, and
/// it must not merge twice.
#[test]
fn the_same_id_twice_acts_once() {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let control = control_over(executors("sesh", Scripted::new(Arc::clone(&ran))));

    control.submit(request("desktop-1-1", label(), false));
    assert!(until(|| matches!(
        control.status(&ActionId::new("desktop-1-1")).status,
        ActionStatus::Done { .. }
    )));

    control.submit(request("desktop-1-1", label(), false));
    // Nothing new can have been enqueued, so a second call would have to
    // appear here; give it a chance to before concluding it did not.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        ran.lock().unwrap().len(),
        1,
        "the same id ran the action twice"
    );
}

/// Even a *different* action under a used id gets the recorded answer
/// rather than running: the id is the promise, and honouring the body
/// instead would make a retry a way to smuggle something new past it.
#[test]
fn a_used_id_is_answered_from_the_record_whatever_the_body_says() {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let control = control_over(executors("sesh", Scripted::new(Arc::clone(&ran))));

    control.submit(request("desktop-1-1", label(), false));
    assert!(until(|| matches!(
        control.status(&ActionId::new("desktop-1-1")).status,
        ActionStatus::Done { .. }
    )));

    control.submit(request("desktop-1-1", merge(), true));
    std::thread::sleep(Duration::from_millis(50));
    let ran = ran.lock().unwrap();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0], label(), "a reused id ran a different action");
}

/// The spec's authorization claim, at the probe: the classification is
/// this machine's, whatever the caller believed.
#[test]
fn an_unconfirmed_irreversible_action_is_refused_here() {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let control = control_over(executors("sesh", Scripted::new(Arc::clone(&ran))));

    match control.submit(request("desktop-1-1", merge(), false)) {
        parallax_baseline::actions::wire::SubmitReply::Refused { reason } => {
            assert!(reason.contains("confirmation"), "got {reason}");
        }
        other => panic!("an unconfirmed merge was {other:?}"),
    }
    assert!(ran.lock().unwrap().is_empty());
}

#[test]
fn a_confirmed_irreversible_action_is_accepted() {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let control = control_over(executors("sesh", Scripted::new(Arc::clone(&ran))));

    control.submit(request("desktop-1-1", merge(), true));
    assert!(until(|| matches!(
        control.status(&ActionId::new("desktop-1-1")).status,
        ActionStatus::Done { .. }
    )));
    assert_eq!(ran.lock().unwrap()[0], merge());
}

/// The machine that owns the project is the one that looks it up, which
/// is exactly what the old cross-machine bug got wrong.
#[test]
fn a_project_this_machine_does_not_have_is_refused_by_name() {
    let control = control_over(executors("sesh", Scripted::new(Default::default())));

    let ttui = Action::RequestReReview {
        project: "ttui".into(),
        item: 3,
    };
    match control.submit(request("desktop-1-1", ttui, false)) {
        parallax_baseline::actions::wire::SubmitReply::Refused { reason } => {
            assert!(
                reason.contains("ttui"),
                "the refusal must name it: {reason}"
            );
            assert!(
                reason.contains("sesh"),
                "and say what it does have: {reason}"
            );
        }
        other => panic!("got {other:?}"),
    }
}

/// `sesh@pi5` is a name for telling two rows apart on one screen. It
/// means nothing on the machine that just has `sesh`.
#[test]
fn a_qualified_name_is_refused_rather_than_guessed_at() {
    let control = control_over(executors("sesh", Scripted::new(Default::default())));

    let qualified = Action::RequestReReview {
        project: "pi5/sesh".into(),
        item: 3,
    };
    match control.submit(request("desktop-1-1", qualified, false)) {
        parallax_baseline::actions::wire::SubmitReply::Refused { reason } => {
            assert!(reason.contains("pi5/sesh"), "got {reason}");
        }
        other => panic!("got {other:?}"),
    }
}

/// The machine that did the thing keeps the record — and records the
/// requester as the unverified claim it is.
#[test]
fn every_accepted_action_is_audited_with_the_requester_as_a_claim() {
    let lines = Recorded::default();
    let control = Control::start(
        executors("sesh", Scripted::new(Default::default())),
        Box::new(lines.clone()),
        ProbeRun::new("r1"),
    );

    control.submit(request("desktop-1-1", label(), false));
    assert!(until(|| lines.0.lock().unwrap().len() >= 2));

    let lines = lines.0.lock().unwrap();
    let accepted = lines
        .iter()
        .find(|l| l.contains("accepted"))
        .expect("audited");
    assert!(accepted.contains("desktop"), "no requester: {accepted}");
    assert!(
        accepted.contains("unverified"),
        "the claim must be labelled a claim: {accepted}"
    );
    assert!(lines.iter().any(|l| l.contains("finished")));
}

/// Over a real socket: a probe without the flag refuses to act, and says
/// what to do about it. `403` rather than `404` so an operator can tell
/// "control is off" from "this probe is too old to have it".
#[test]
fn a_probe_without_the_flag_refuses_to_act_and_names_the_flag() {
    let mut cockpit = remote(spawn(None));
    match cockpit.submit(&label(), None) {
        Submitted::Refused { reason } => {
            assert!(reason.contains("--allow-control"), "got {reason}");
        }
        other => panic!("a control-disabled probe answered {other:?}"),
    }
}

/// Turning control off must not cost observation. The read path is the
/// reason the probe exists.
#[test]
fn a_probe_without_control_still_serves_state() {
    use parallax_baseline::peers::PeerClient;
    use std::time::SystemTime;

    let url = spawn(None);
    let mut peer = PeerClient::new(UreqTransport::new(), url);
    assert!(
        peer.fetch(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
            .is_ok(),
        "a control-disabled probe stopped answering /state"
    );
}

/// And the read path does not become a write path because a write path
/// exists beside it.
#[test]
fn state_still_runs_no_build_with_control_enabled() {
    use parallax_baseline::peers::PeerClient;
    use std::time::SystemTime;

    let control = control_over(executors("sesh", Scripted::new(Default::default())));
    let url = spawn(Some(control));
    let mut peer = PeerClient::new(UreqTransport::new(), url);
    let started = Instant::now();
    let projects = peer
        .fetch(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .expect("answers");
    assert!(projects.is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "took long enough to have built something"
    );
}

/// The whole round trip, over a socket, with the types a cockpit uses:
/// submit, get an id back, and read the outcome off the machine that
/// ran it.
#[test]
fn a_cockpit_submits_over_a_socket_and_reads_the_outcome_back() {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let control = control_over(executors("sesh", Scripted::new(Arc::clone(&ran))));
    let mut cockpit = remote(spawn(Some(control)));

    let id = match cockpit.submit(&merge(), Some(&Confirmation::of(&merge()))) {
        Submitted::Accepted { id, .. } => id,
        other => panic!("expected acceptance, got {other:?}"),
    };
    assert!(
        until(|| matches!(cockpit.standing(&id), Standing::Done { .. })),
        "the action never reported done; last standing {:?}",
        cockpit.standing(&id)
    );
    assert_eq!(ran.lock().unwrap()[0], merge());
}

/// An action that ran and failed is a **real answer**, and must reach
/// the operator as one. Folding it into the unknown would be the
/// opposite of this arc's mistake and just as misleading: there is
/// nothing uncertain about a merge conflict.
#[test]
fn an_action_that_ran_and_failed_is_reported_as_failed_not_unknown() {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let control = control_over(executors("sesh", Scripted::failing(Arc::clone(&ran))));
    let mut cockpit = remote(spawn(Some(control)));

    let id = match cockpit.submit(&label(), None) {
        Submitted::Accepted { id, .. } => id,
        other => panic!("expected acceptance, got {other:?}"),
    };
    assert!(
        until(|| matches!(cockpit.standing(&id), Standing::Failed { .. })),
        "a failed action reported {:?}",
        cockpit.standing(&id)
    );
    assert_eq!(
        ran.lock().unwrap().len(),
        1,
        "it should have been attempted"
    );
}

/// An id this run never saw, from a client that holds no acceptance for
/// it, is unknown rather than "never arrived" — the client cannot tell
/// the difference, and guessing is how a merge happens twice.
#[test]
fn an_id_this_cockpit_never_submitted_is_unknown_rather_than_never_arrived() {
    let control = control_over(executors("sesh", Scripted::new(Default::default())));
    let mut cockpit = remote(spawn(Some(control)));

    match cockpit.standing(&ActionId::new("someone-else-1-1")) {
        Standing::Unknown { .. } => {}
        other => panic!("got {other:?}"),
    }
}
