//! The refresh thread.
//!
//! The whole design is one sentence: **the thread owns the adapters and
//! the UI owns the state**. Nothing is shared, nothing is locked, and no
//! lock is ever held across a render. The UI drains with `try_recv` and
//! returns immediately, so a poll that takes twenty seconds costs the
//! event loop nothing.
//!
//! It also splits its adapters by `CheckCost`. Readers go in the
//! cadence; anything that runs a build is held back until someone asks
//! for it by name. That split is the only thing standing between this
//! design and a cockpit that runs `cargo test` every thirty seconds on
//! the machine running the agent sessions.

use parallax_baseline::adapters::http::HttpTransport;
use parallax_baseline::adapters::verification::VerificationStatus;
use parallax_baseline::freshness::Observed;
use parallax_baseline::peers::PeerClient;
use parallax_baseline::state::{aggregate_project, ProjectAdapters, ProjectState};
use parallax_baseline::validate::Validated;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::SystemTime;

/// A peer whose transport is decided at runtime: the live one in a
/// running cockpit, a recorded one in fixture mode. Boxed because those
/// are different concrete types and they share one list.
pub type BoxedPeer = PeerClient<Box<dyn HttpTransport + Send>>;

/// Where the refresh thread gets its `now`.
///
/// Frozen in fixture mode, so a captured frame is identical run to run
/// and a Plumb NO-GO means the layout is wrong rather than that time
/// passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    /// Sample the system clock at each observation.
    System,
    /// Always this instant.
    Frozen(SystemTime),
}

impl Clock {
    /// The instant to stamp an observation with.
    pub fn now(&self) -> SystemTime {
        match self {
            Clock::System => SystemTime::now(),
            Clock::Frozen(t) => *t,
        }
    }
}

/// What the UI asks the refresh thread to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Re-read every source that only reads. Never runs a build.
    RefreshReads,
    /// Run one project's build checks, because the operator asked.
    RunChecks {
        /// Which project.
        project: String,
    },
    /// Run every project's build checks.
    RunAllChecks,
    /// Finish the current request and stop.
    Stop,
}

/// What the refresh thread sends back.
#[derive(Debug)]
pub enum Update {
    /// A project's state, freshly aggregated. Boxed because a
    /// `ProjectState` is much larger than the other variants.
    Project(Box<ProjectState>),
    /// A project's build checks ran, and this is what they said.
    ChecksRan {
        /// Which project.
        project: String,
        /// One standing per check that ran.
        checks: Vec<Observed<VerificationStatus>>,
    },
    /// A project could not be refreshed at all.
    ///
    /// Distinct from a `Degradation`, which lives *inside* a
    /// `ProjectState` and means one source failed. This means the
    /// aggregation itself did not happen.
    Failed {
        /// Which project.
        project: String,
        /// Why, in one sentence.
        problem: String,
    },
    /// A peer answered: everything that machine currently holds.
    ///
    /// The whole list at once rather than a project at a time, because
    /// it is also the answer to "what is *no longer* there" — a project
    /// removed on the Pi has to leave the rail, and a stream of
    /// per-project updates can never say that.
    PeerState {
        /// Which peer, by the name its registry entry gives it.
        peer: String,
        /// Every project it serves.
        projects: Vec<ProjectState>,
    },
    /// A peer did not answer.
    PeerFailed {
        /// Which peer.
        peer: String,
        /// Why, in one sentence.
        reason: String,
    },
}

/// Takes the checks that run a build out of `adapters`, leaving only
/// what is safe to poll on a cadence.
///
/// Re-exported rather than defined here. It said it was public so that
/// "a future daemon" could apply the same rule; the probe is that
/// daemon, and it cannot depend on a cockpit — so the rule moved down
/// into `parallax-baseline` where all three consumers can reach it.
/// One implementation, which is the whole point the comment was making.
pub use parallax_baseline::state::split_by_cost;

/// One project's adapters, split by what calling them costs.
struct Split {
    validated: Validated,
    /// Everything safe to poll on a cadence.
    reading: ProjectAdapters,
    /// The checks that run a build, held back until asked for.
    executing: Vec<Box<dyn parallax_baseline::adapters::verification::VerificationAdapter + Send>>,
}

/// A handle on the refresh thread.
pub struct Refresher {
    requests: Sender<Request>,
    updates: Receiver<Update>,
    handle: Option<JoinHandle<()>>,
    executor_kinds: BTreeMap<String, Vec<String>>,
}

impl Refresher {
    /// Starts a refresh thread over the given projects.
    ///
    /// The adapters move into the thread and never come back, which is
    /// what makes "nothing is shared" true rather than aspirational.
    pub fn spawn(projects: Vec<(Validated, ProjectAdapters)>, clock: Clock) -> Self {
        Self::spawn_with_peers(projects, Vec::new(), clock)
    }

    /// Starts a refresh thread over local projects **and** peers.
    ///
    /// A peer goes on the same cadence as everything else and is fetched
    /// on the same thread, so the rule that nothing blocks the UI holds
    /// with a network behind it exactly as it held with a disk.
    pub fn spawn_with_peers(
        projects: Vec<(Validated, ProjectAdapters)>,
        peers: Vec<BoxedPeer>,
        clock: Clock,
    ) -> Self {
        let (request_tx, request_rx) = channel::<Request>();
        let (update_tx, update_rx) = channel::<Update>();

        let mut splits = Vec::new();
        let mut executor_kinds: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for (validated, mut adapters) in projects {
            let name = validated.manifest().project.name.clone();
            let executing = split_by_cost(&mut adapters);
            for adapter in &executing {
                executor_kinds
                    .entry(name.clone())
                    .or_default()
                    .push(kind_of(&adapter.source_name()));
            }
            splits.push(Split {
                validated,
                reading: adapters,
                executing,
            });
        }

        let handle = std::thread::spawn(move || run(splits, peers, clock, request_rx, update_tx));

        Self {
            requests: request_tx,
            updates: update_rx,
            handle: Some(handle),
            executor_kinds,
        }
    }

    /// Asks the thread to do something. Returns immediately.
    pub fn request(&self, request: Request) {
        // A closed channel means the thread is already gone, which is
        // not a reason to take the UI down with it.
        let _ = self.requests.send(request);
    }

    /// Takes everything that has arrived. Never blocks.
    pub fn drain(&self) -> Vec<Update> {
        let mut out = Vec::new();
        loop {
            match self.updates.try_recv() {
                Ok(update) => out.push(update),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return out,
            }
        }
    }

    /// Which check kinds run a build, per project — the ones the cadence
    /// will never touch, and which therefore read "not run this session"
    /// until someone asks for them.
    pub fn executor_kinds(&self, project: &str) -> &[String] {
        self.executor_kinds
            .get(project)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Stops the thread and waits for it.
    pub fn stop(mut self) {
        self.request(Request::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Refresher {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// `verification:command:lint` -> `lint`.
///
/// The adapters carry their kind only inside `source_name`, and the
/// alternative — re-reading the manifest — would duplicate the
/// interpretation the factory exists to own.
fn kind_of(source_name: &str) -> String {
    source_name
        .rsplit(':')
        .next()
        .unwrap_or(source_name)
        .to_string()
}

/// Drops read-refreshes that queued while one was already running.
///
/// The cadence fires every poll interval whether or not the last cycle
/// finished. That was harmless when a cycle meant reading a disk and
/// polling GitHub; it stopped being harmless once a cycle could include
/// a machine that is asleep, because a peer that has to time out costs
/// most of an interval by itself. Two dead peers and the sweep no longer
/// fits between two ticks — and every sweep that runs late makes the
/// next one later, permanently.
///
/// Two consecutive read-refreshes ask the same question, so the second
/// is dropped rather than re-asked; the answer it would have produced
/// arrives from the first, moments later. **Nothing else is touched.**
/// `Stop` has to arrive, and a `RunChecks` the operator actually pressed
/// has to run — collapsing those would lose work somebody asked for,
/// which is a different and much worse bug than being slow.
fn coalesce(requests: Vec<Request>) -> Vec<Request> {
    let mut out: Vec<Request> = Vec::with_capacity(requests.len());
    for request in requests {
        let already_queued =
            request == Request::RefreshReads && out.contains(&Request::RefreshReads);
        if !already_queued {
            out.push(request);
        }
    }
    out
}

/// The thread body. One request at a time, one send per project.
fn run(
    mut splits: Vec<Split>,
    mut peers: Vec<BoxedPeer>,
    clock: Clock,
    requests: Receiver<Request>,
    updates: Sender<Update>,
) {
    while let Ok(first) = requests.recv() {
        // Take everything that piled up while the last cycle ran, then
        // collapse the duplicates out of it.
        let mut pending = vec![first];
        while let Ok(next) = requests.try_recv() {
            pending.push(next);
        }
        for request in coalesce(pending) {
            if run_one(request, &mut splits, &mut peers, clock, &updates).is_break() {
                return;
            }
        }
    }
}

/// Handles one request. Breaks when the thread should stop.
fn run_one(
    request: Request,
    splits: &mut [Split],
    peers: &mut [BoxedPeer],
    clock: Clock,
    updates: &Sender<Update>,
) -> std::ops::ControlFlow<()> {
    use std::ops::ControlFlow::{Break, Continue};
    match request {
        Request::Stop => Break(()),
        Request::RefreshReads => {
            for split in splits.iter_mut() {
                // Per project, not one batch: one slow project must not
                // withhold the others, and a rail that updates row by
                // row is more honest than a screen that changes all at
                // once.
                let state = aggregate_project(&split.validated, &mut split.reading, clock.now());
                if updates.send(Update::Project(Box::new(state))).is_err() {
                    return Break(()); // the UI is gone
                }
            }
            // Peers last: local state costs a disk read and a peer costs
            // a round trip, so the rows this machine can answer for
            // appear first.
            for peer in peers.iter_mut() {
                let update = match peer.fetch(clock.now()) {
                    Ok(projects) => Update::PeerState {
                        peer: peer.name().to_string(),
                        projects,
                    },
                    // One unreachable machine degrades itself and
                    // nothing else — the rule the registry and
                    // `aggregate` already share, now across a network.
                    Err(failure) => Update::PeerFailed {
                        peer: peer.name().to_string(),
                        reason: failure.reason,
                    },
                };
                if updates.send(update).is_err() {
                    return Break(());
                }
            }
            Continue(())
        }
        Request::RunChecks { project } => {
            if let Some(split) = splits
                .iter_mut()
                .find(|s| s.validated.manifest().project.name == project)
            {
                if run_checks(split, clock, updates).is_err() {
                    return Break(());
                }
            }
            Continue(())
        }
        Request::RunAllChecks => {
            for split in splits.iter_mut() {
                if run_checks(split, clock, updates).is_err() {
                    return Break(());
                }
            }
            Continue(())
        }
    }
}

/// Runs one project's build checks and reports what they said.
fn run_checks(
    split: &mut Split,
    clock: Clock,
    updates: &Sender<Update>,
) -> Result<(), std::sync::mpsc::SendError<Update>> {
    let name = split.validated.manifest().project.name.clone();
    let root = split
        .validated
        .manifest()
        .project
        .root
        .clone()
        .unwrap_or_default();
    let mut ctx = parallax_baseline::adapters::ProjectContext::new(name.clone(), root);
    if let Some(work) = &split.validated.manifest().work {
        ctx = ctx.with_repo(work.repo.clone());
    }

    let mut checks = Vec::new();
    for adapter in split.executing.iter_mut() {
        match adapter.check(&ctx, clock.now()) {
            Ok(observed) => checks.push(observed),
            Err(problem) => {
                updates.send(Update::Failed {
                    project: name.clone(),
                    problem: problem.to_string(),
                })?;
            }
        }
    }
    updates.send(Update::ChecksRan {
        project: name,
        checks,
    })
}

#[cfg(test)]
mod coalesce_tests {
    use super::*;

    fn checks(project: &str) -> Request {
        Request::RunChecks {
            project: project.to_string(),
        }
    }

    /// The idle case, which is almost always the case: nothing piled up,
    /// so nothing changes.
    #[test]
    fn one_request_passes_through_untouched() {
        assert_eq!(
            coalesce(vec![Request::RefreshReads]),
            vec![Request::RefreshReads]
        );
    }

    /// The whole point. Three ticks fired while a sweep was still waiting
    /// on a sleeping laptop, and all three ask the same question.
    #[test]
    fn read_refreshes_that_piled_up_collapse_to_one() {
        let piled = vec![
            Request::RefreshReads,
            Request::RefreshReads,
            Request::RefreshReads,
        ];
        assert_eq!(coalesce(piled), vec![Request::RefreshReads]);
    }

    /// Losing work somebody asked for is a worse bug than being slow, so
    /// nothing but a duplicate read is ever dropped.
    #[test]
    fn work_the_operator_asked_for_is_never_collapsed() {
        let piled = vec![
            Request::RefreshReads,
            checks("ttui"),
            Request::RefreshReads,
            checks("sesh"),
            Request::RunAllChecks,
        ];
        assert_eq!(
            coalesce(piled),
            vec![
                Request::RefreshReads,
                checks("ttui"),
                checks("sesh"),
                Request::RunAllChecks,
            ]
        );
    }

    /// Two presses of `c` on one project are two runs, not a duplicate:
    /// the operator asked twice, presumably because something changed in
    /// between.
    #[test]
    fn the_same_project_asked_to_run_checks_twice_runs_twice() {
        let piled = vec![checks("ttui"), checks("ttui")];
        assert_eq!(coalesce(piled), vec![checks("ttui"), checks("ttui")]);
    }

    #[test]
    fn stop_survives_coalescing() {
        let piled = vec![Request::RefreshReads, Request::Stop, Request::RefreshReads];
        assert!(coalesce(piled).contains(&Request::Stop));
    }
}
