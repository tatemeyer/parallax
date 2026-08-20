//! Running an action this machine was asked to run.
//!
//! Three things live here, and each exists to keep one promise the
//! design makes.
//!
//! [`Ledger`] is what makes asking twice safe: an id it already holds is
//! answered from the record rather than run again.
//!
//! The worker is what makes **a submission not an execution**. The
//! actions worth taking remotely are the slow ones — a capture drives a
//! PTY, a build on a Pi 5 is minutes — and a request that held the
//! socket open for the duration could not be given an honest timeout.
//! So [`Control::submit`] decides, records, and returns; a thread does
//! the work.
//!
//! [`Audit`] is where the machine that did the thing keeps the record.
//! The cockpit's log is in memory on another machine and dies with it.

use parallax_baseline::actions::wire::{
    ActionId, ActionRequest, ActionStatus, ProbeRun, StatusReply, SubmitReply,
};
use parallax_baseline::actions::ActionExecutor;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

/// How many actions a probe remembers.
///
/// Far more than an operator takes in a session, and bounded because an
/// unbounded map keyed by client-chosen ids is a memory leak on a
/// machine with a kiosk on it. A count rather than an age deliberately:
/// a count needs no clock, and the Pi has no RTC.
pub const LEDGER_BOUND: usize = 256;

/// An executor per project, by the name that machine knows it by.
pub type Executors = HashMap<String, Box<dyn ActionExecutor + Send>>;

/// Where a probe writes what it was asked to do.
pub trait Audit {
    /// Records one line.
    fn line(&mut self, entry: &str);
}

/// The audit that ships: stdout, which under the systemd user unit is
/// the journal.
#[derive(Debug, Default)]
pub struct StdoutAudit;

impl Audit for StdoutAudit {
    fn line(&mut self, entry: &str) {
        println!("{entry}");
    }
}

/// What this machine has been asked to run, and what came of it.
///
/// **Memory, and bounded.** Both of those make `not-submitted` a claim
/// about this ledger rather than about the world, which is why every
/// answer it gives is stamped with [`ProbeRun`]: a client comparing that
/// to the run it submitted under can tell "I never got it" from "I have
/// forgotten".
pub struct Ledger {
    run: ProbeRun,
    entries: VecDeque<(ActionId, ActionStatus)>,
    bound: usize,
}

impl Ledger {
    /// A ledger for one run of one probe.
    pub fn new(run: ProbeRun) -> Self {
        Self::with_bound(run, LEDGER_BOUND)
    }

    /// A ledger with a different bound, for tests that want to overflow
    /// it without writing 256 actions.
    pub fn with_bound(run: ProbeRun, bound: usize) -> Self {
        Self {
            run,
            entries: VecDeque::new(),
            bound: bound.max(1),
        }
    }

    /// Which run this is.
    pub fn run(&self) -> &ProbeRun {
        &self.run
    }

    /// What this ledger knows about `id`.
    pub fn status(&self, id: &ActionId) -> ActionStatus {
        self.entries
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, status)| status.clone())
            .unwrap_or(ActionStatus::NotSubmitted)
    }

    /// Whether this ledger has heard of `id` at all.
    pub fn holds(&self, id: &ActionId) -> bool {
        !matches!(self.status(id), ActionStatus::NotSubmitted)
    }

    /// Files `status` under `id`, replacing any earlier one.
    ///
    /// Updating in place rather than pushing keeps an action's history
    /// one entry long — a `Running` that became `Done` is one action,
    /// and letting it take two slots would halve the bound for the
    /// actions that actually finished.
    pub fn record(&mut self, id: ActionId, status: ActionStatus) {
        if let Some(entry) = self.entries.iter_mut().find(|(known, _)| *known == id) {
            entry.1 = status;
            return;
        }
        self.entries.push_back((id, status));
        while self.entries.len() > self.bound {
            self.entries.pop_front();
        }
    }

    /// This ledger's answer to a status query, stamped with its run.
    pub fn reply(&self, id: &ActionId) -> StatusReply {
        StatusReply {
            run: self.run.clone(),
            status: self.status(id),
        }
    }
}

/// The probe's control surface: a ledger, a worker, and the set of
/// projects this machine actually has.
pub struct Control {
    ledger: Arc<Mutex<Ledger>>,
    audit: Arc<Mutex<Box<dyn Audit + Send>>>,
    jobs: Sender<ActionRequest>,
    /// Names the worker can act on. Held here as well as in the worker
    /// because the refusal for an unknown project belongs on the
    /// answering thread, where it can be immediate.
    projects: BTreeSet<String>,
}

impl Control {
    /// Starts a worker over `executors` and returns the surface in front
    /// of it.
    pub fn start(executors: Executors, audit: Box<dyn Audit + Send>, run: ProbeRun) -> Self {
        let ledger = Arc::new(Mutex::new(Ledger::new(run)));
        let audit = Arc::new(Mutex::new(audit));
        let projects = executors.keys().cloned().collect();
        let (jobs, inbox) = mpsc::channel::<ActionRequest>();

        let worker_ledger = Arc::clone(&ledger);
        let worker_audit = Arc::clone(&audit);
        // Detached on purpose. The probe serves until its listener dies,
        // and a worker that outlived the process it belongs to is not a
        // thing this binary can produce.
        std::thread::spawn(move || {
            let mut executors = executors;
            for request in inbox {
                let status = run_one(&mut executors, &request);
                if let Ok(mut audit) = worker_audit.lock() {
                    audit.line(&format!(
                        "action {} finished: {}",
                        request.id,
                        describe(&status)
                    ));
                }
                if let Ok(mut ledger) = worker_ledger.lock() {
                    ledger.record(request.id, status);
                }
            }
        });

        Self {
            ledger,
            audit,
            jobs,
            projects,
        }
    }

    /// Which run of the probe this is.
    pub fn run(&self) -> ProbeRun {
        self.locked_ledger(|l| l.run().clone())
    }

    /// Decides on a request, records it, and returns — **without
    /// running it**.
    pub fn submit(&self, request: ActionRequest) -> SubmitReply {
        let id = request.id.clone();
        let run = self.run();

        // Asking twice is safe: the id is already ours, and the client
        // is entitled to the answer we already have. Deliberately before
        // every other check, so a retry of something we refused gets the
        // same refusal rather than a fresh evaluation that might differ.
        if self.locked_ledger(|l| l.holds(&id)) {
            return SubmitReply::Accepted { id, run };
        }

        if let Some(reason) = self.reason_to_refuse(&request) {
            self.record(
                id,
                ActionStatus::Refused {
                    reason: reason.clone(),
                },
            );
            self.audit(&format!(
                "action {} from {} refused: {reason}",
                request.id, request.requested_by
            ));
            return SubmitReply::Refused { reason };
        }

        self.audit(&format!(
            "action {} accepted: {} (requested by `{}`, unverified)",
            request.id,
            request.action.summary(),
            request.requested_by
        ));
        self.record(id.clone(), ActionStatus::Running);
        if self.jobs.send(request).is_err() {
            // The worker is gone, which is this binary failing rather
            // than the action being refused — but nothing ran, and
            // saying so plainly beats leaving a `Running` that never
            // moves.
            let reason = "the probe's action worker has stopped".to_string();
            self.record(
                id,
                ActionStatus::Refused {
                    reason: reason.clone(),
                },
            );
            return SubmitReply::Refused { reason };
        }
        SubmitReply::Accepted { id, run }
    }

    /// Why this request cannot be run here, if it cannot.
    ///
    /// Authorization is checked here **and** again in the worker. Here
    /// so an operator learns immediately that a merge needs confirming
    /// rather than after a round trip; again there because
    /// [`parallax_baseline::actions::Authorized`] borrows the action it
    /// authorizes and so cannot be carried across a channel — which is
    /// the same property that stops it crossing a network.
    fn reason_to_refuse(&self, request: &ActionRequest) -> Option<String> {
        if !request.version_matches() {
            return Some(format!(
                "this probe speaks `{}`, and the request is `{}`",
                parallax_baseline::actions::wire::ACTION_API_VERSION,
                request.api_version
            ));
        }
        let project = request.action.project();
        if project.contains('/') {
            return Some(format!(
                "`{project}` is a name for two machines' rows on one screen. \
                 Ask for the project by the name this machine knows it by."
            ));
        }
        if !self.projects.contains(project) {
            return Some(match self.projects.is_empty() {
                true => format!("no project `{project}` here: this machine has none registered"),
                false => format!(
                    "no project `{project}` on this machine. It has: {}",
                    self.projects.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
            });
        }
        request.authorize_here().err().map(|e| e.to_string())
    }

    /// What became of an action, stamped with this run.
    pub fn status(&self, id: &ActionId) -> StatusReply {
        self.locked_ledger(|l| l.reply(id))
    }

    fn record(&self, id: ActionId, status: ActionStatus) {
        if let Ok(mut ledger) = self.ledger.lock() {
            ledger.record(id, status);
        }
    }

    fn audit(&self, line: &str) {
        if let Ok(mut audit) = self.audit.lock() {
            audit.line(line);
        }
    }

    /// Reads the ledger, tolerating a poisoned lock.
    ///
    /// A panicking worker must not take the read path down with it: a
    /// probe that stopped answering `/state` because an action failed
    /// would turn one bad merge into a machine that has vanished.
    fn locked_ledger<T>(&self, f: impl FnOnce(&Ledger) -> T) -> T {
        match self.ledger.lock() {
            Ok(ledger) => f(&ledger),
            Err(poisoned) => f(&poisoned.into_inner()),
        }
    }
}

/// Executes one request, returning what to record.
fn run_one(executors: &mut Executors, request: &ActionRequest) -> ActionStatus {
    let Some(executor) = executors.get_mut(request.action.project()) else {
        return ActionStatus::Refused {
            reason: format!("no project `{}` on this machine", request.action.project()),
        };
    };
    match request.authorize_here() {
        Err(e) => ActionStatus::Refused {
            reason: e.to_string(),
        },
        Ok(authorized) => match executor.execute(authorized) {
            Ok(outcome) => ActionStatus::Done {
                summary: outcome.summary,
            },
            Err(e) => ActionStatus::Failed {
                reason: e.to_string(),
            },
        },
    }
}

/// One line describing a status, for the audit.
fn describe(status: &ActionStatus) -> String {
    match status {
        ActionStatus::Running => "still running".to_string(),
        ActionStatus::Done { summary } => format!("done — {summary}"),
        ActionStatus::Failed { reason } => format!("failed — {reason}"),
        ActionStatus::Refused { reason } => format!("refused — {reason}"),
        ActionStatus::NotSubmitted => "not submitted".to_string(),
    }
}

#[cfg(test)]
mod ledger_tests {
    use super::*;

    fn id(n: usize) -> ActionId {
        ActionId::new(format!("desktop-1-{n}"))
    }

    fn ledger() -> Ledger {
        Ledger::new(ProbeRun::new("r1"))
    }

    #[test]
    fn an_id_never_seen_is_not_submitted() {
        assert_eq!(ledger().status(&id(1)), ActionStatus::NotSubmitted);
    }

    #[test]
    fn a_recorded_status_comes_back() {
        let mut l = ledger();
        l.record(id(1), ActionStatus::Running);
        assert_eq!(l.status(&id(1)), ActionStatus::Running);
    }

    #[test]
    fn recording_again_updates_rather_than_appending() {
        let mut l = ledger();
        l.record(id(1), ActionStatus::Running);
        l.record(
            id(1),
            ActionStatus::Done {
                summary: "merged".into(),
            },
        );
        assert_eq!(
            l.status(&id(1)),
            ActionStatus::Done {
                summary: "merged".into()
            }
        );
        assert_eq!(l.entries.len(), 1, "one action must not take two slots");
    }

    /// The bound is a memory guard, and forgetting is not the same as
    /// never having heard — which is why the client compares runs. Here
    /// the ledger's own half: the oldest entry goes.
    #[test]
    fn the_ledger_forgets_its_oldest_entry_at_the_bound() {
        let mut l = Ledger::with_bound(ProbeRun::new("r1"), 2);
        for n in 1..=3 {
            l.record(id(n), ActionStatus::Running);
        }
        assert_eq!(l.status(&id(1)), ActionStatus::NotSubmitted);
        assert_eq!(l.status(&id(3)), ActionStatus::Running);
    }

    #[test]
    fn every_reply_is_stamped_with_the_run_that_gave_it() {
        assert_eq!(ledger().reply(&id(1)).run, ProbeRun::new("r1"));
    }
}
