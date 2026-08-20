//! The thread that carries actions to other machines.
//!
//! **Why this is not the refresh thread.** Two reasons, and the second
//! is the one that decided it.
//!
//! The refresh thread is observation, and `tests/read_only.rs` encodes
//! that: it may not name an action. Threading control through it would
//! have meant deleting the guarantee rather than keeping it.
//!
//! And it would have been slower in the way that matters. A read sweep
//! walks every project and then every peer, and a peer that is asleep
//! costs the connect timeout — so `m` pressed at the wrong moment would
//! wait behind two dead machines before it was even sent. A keystroke
//! that acts must not queue behind a poll that only looks.
//!
//! **This thread decides nothing.** `Control` raises the prompt and
//! `authorize` on the far machine settles it; all that happens here is
//! carrying, waiting, and asking again.

use parallax_baseline::actions::wire::ActionId;
use parallax_baseline::actions::{Action, Confirmation, Standing, Submitted, Submitter};
use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

/// A peer this cockpit may act on. Boxed for the reason `BoxedPeer` is:
/// a live transport and a recorded one are different concrete types and
/// they share one list.
pub type BoxedSubmitter = Box<dyn Submitter + Send>;

/// What the UI asks the courier to do.
pub enum Errand {
    /// Offer an action to a machine.
    Submit {
        /// Which machine.
        peer: String,
        /// What to ask it to do.
        action: Action,
        /// The operator's confirmation, when one was given.
        confirmation: Option<Confirmation>,
    },
    /// Ask every machine what became of the actions it accepted.
    Poll,
    /// Finish and stop.
    Stop,
}

/// What the courier sends back.
#[derive(Debug)]
pub enum Answer {
    /// A machine answered a submission — or did not.
    Submitted {
        /// The action's own words, which is how the log line it belongs
        /// to is found before any id exists to match on.
        summary: String,
        /// What the machine said, including the case where it said
        /// nothing at all.
        outcome: Submitted,
    },
    /// An accepted action reached an end worth telling the operator.
    Resolved {
        /// Which action.
        id: ActionId,
        /// Where it ended up.
        standing: Standing,
    },
}

/// A handle on the courier thread.
pub struct Courier {
    errands: Sender<Errand>,
    answers: Receiver<Answer>,
    handle: Option<JoinHandle<()>>,
    peers: BTreeSet<String>,
}

impl Courier {
    /// Starts a courier over the machines this cockpit may act on.
    ///
    /// The submitters move into the thread and never come back, which is
    /// what keeps "the UI never makes a network call" true rather than
    /// aspirational.
    pub fn spawn(submitters: Vec<BoxedSubmitter>) -> Self {
        let peers = submitters.iter().map(|s| s.peer().to_string()).collect();
        let (errand_tx, errand_rx) = channel::<Errand>();
        let (answer_tx, answer_rx) = channel::<Answer>();
        let handle = std::thread::spawn(move || run(submitters, errand_rx, answer_tx));
        Self {
            errands: errand_tx,
            answers: answer_rx,
            handle: Some(handle),
            peers,
        }
    }

    /// A courier that carries nothing, for a cockpit with no peers it
    /// may act on.
    ///
    /// Not the fixture-mode default any more: a fixture peer that
    /// recorded a control surface is carried to, over a transport that
    /// cannot reach it. See `fixtures::CONTROL_SUFFIX`.
    pub fn idle() -> Self {
        Self::spawn(Vec::new())
    }

    /// Whether this cockpit can act on `peer` at all.
    ///
    /// A peer it can *see* but not act on is the ordinary case: control
    /// is off by default on every probe.
    pub fn carries_to(&self, peer: &str) -> bool {
        self.peers.contains(peer)
    }

    /// Asks the thread to do something. Returns immediately.
    pub fn send(&self, errand: Errand) {
        // A closed channel means the thread is already gone, which is
        // not a reason to take the UI down with it.
        let _ = self.errands.send(errand);
    }

    /// Takes everything that has arrived. Never blocks.
    pub fn drain(&self) -> Vec<Answer> {
        let mut out = Vec::new();
        loop {
            match self.answers.try_recv() {
                Ok(answer) => out.push(answer),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return out,
            }
        }
    }
}

impl Default for Courier {
    fn default() -> Self {
        Self::idle()
    }
}

impl Drop for Courier {
    fn drop(&mut self) {
        let _ = self.errands.send(Errand::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// The thread body.
fn run(mut submitters: Vec<BoxedSubmitter>, errands: Receiver<Errand>, answers: Sender<Answer>) {
    // Which machine holds each action still waiting on an answer.
    let mut outstanding: Vec<(usize, ActionId)> = Vec::new();

    while let Ok(errand) = errands.recv() {
        match errand {
            Errand::Stop => return,
            Errand::Submit {
                peer,
                action,
                confirmation,
            } => {
                let summary = action.summary();
                let outcome = match submitters.iter_mut().position(|s| s.peer() == peer) {
                    None => Submitted::Refused {
                        reason: format!(
                            "no probe on {peer} this cockpit may act through — the machine \
                             serves state but control is not enabled on it"
                        ),
                    },
                    Some(at) => {
                        let outcome = submitters[at].submit(&action, confirmation.as_ref());
                        if let Submitted::Accepted { id, .. } = &outcome {
                            outstanding.push((at, id.clone()));
                        }
                        // An `Unknown` is deliberately *not* tracked
                        // here. There is no acceptance to read a later
                        // `not-submitted` against, so the client cannot
                        // reach a conclusion by asking again — which is
                        // what `RemoteExecutor` already says, and
                        // re-deciding it here would put that rule in two
                        // places that could disagree.
                        outcome
                    }
                };
                if answers
                    .send(Answer::Submitted { summary, outcome })
                    .is_err()
                {
                    return; // the UI is gone
                }
            }
            Errand::Poll => {
                let mut still_waiting = Vec::with_capacity(outstanding.len());
                for (at, id) in outstanding.drain(..) {
                    let standing = submitters[at].standing(&id);
                    if matches!(standing, Standing::Running) {
                        still_waiting.push((at, id));
                        continue;
                    }
                    // An `Unknown` stops being asked about. Asking again
                    // cannot resolve it: the probe has already given the
                    // only answer it has, and a line that flickered
                    // between unknown and unknown would read as activity.
                    if answers.send(Answer::Resolved { id, standing }).is_err() {
                        return;
                    }
                }
                outstanding = still_waiting;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_baseline::actions::wire::ProbeRun;
    use std::sync::{Arc, Mutex};

    /// A submitter that answers from a script and records what it was
    /// asked, so a test can assert on both halves.
    struct Scripted {
        peer: String,
        offered: Arc<Mutex<Vec<Action>>>,
        reply: Submitted,
        standings: Arc<Mutex<Vec<Standing>>>,
    }

    impl Submitter for Scripted {
        fn peer(&self) -> &str {
            &self.peer
        }

        fn submit(&mut self, action: &Action, _c: Option<&Confirmation>) -> Submitted {
            self.offered.lock().unwrap().push(action.clone());
            self.reply.clone()
        }

        fn standing(&mut self, _id: &ActionId) -> Standing {
            let mut queued = self.standings.lock().unwrap();
            if queued.is_empty() {
                return Standing::Running;
            }
            queued.remove(0)
        }
    }

    fn label() -> Action {
        Action::SetAutonomyLabel {
            project: "sesh".into(),
            item: 7,
            label: "gated".into(),
        }
    }

    fn accepted() -> Submitted {
        Submitted::Accepted {
            id: ActionId::new("desktop-1-1"),
            run: ProbeRun::new("r1"),
        }
    }

    /// Blocks until an answer arrives, or gives up rather than hanging
    /// the suite.
    fn next(courier: &Courier) -> Answer {
        for _ in 0..2000 {
            if let Some(answer) = courier.drain().into_iter().next() {
                return answer;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the courier never answered");
    }

    fn courier_with(
        reply: Submitted,
        standings: Vec<Standing>,
    ) -> (Courier, Arc<Mutex<Vec<Action>>>) {
        let offered = Arc::new(Mutex::new(Vec::new()));
        let submitter = Scripted {
            peer: "pi5".into(),
            offered: Arc::clone(&offered),
            reply,
            standings: Arc::new(Mutex::new(standings)),
        };
        (Courier::spawn(vec![Box::new(submitter)]), offered)
    }

    #[test]
    fn an_action_reaches_the_machine_it_was_addressed_to() {
        let (courier, offered) = courier_with(accepted(), vec![]);
        courier.send(Errand::Submit {
            peer: "pi5".into(),
            action: label(),
            confirmation: None,
        });
        assert!(matches!(next(&courier), Answer::Submitted { .. }));
        assert_eq!(offered.lock().unwrap()[0], label());
    }

    /// A machine the cockpit can see but not act on is the ordinary
    /// case, and it must say so rather than silently doing nothing.
    #[test]
    fn a_machine_with_no_control_is_refused_by_name() {
        let (courier, offered) = courier_with(accepted(), vec![]);
        courier.send(Errand::Submit {
            peer: "tates-laptop".into(),
            action: label(),
            confirmation: None,
        });
        match next(&courier) {
            Answer::Submitted {
                outcome: Submitted::Refused { reason },
                ..
            } => assert!(reason.contains("tates-laptop"), "got {reason}"),
            other => panic!("got {other:?}"),
        }
        assert!(
            offered.lock().unwrap().is_empty(),
            "it was offered to the wrong machine"
        );
    }

    #[test]
    fn an_accepted_action_is_asked_about_until_it_settles() {
        let (courier, _) = courier_with(
            accepted(),
            vec![
                Standing::Running,
                Standing::Done {
                    summary: "labelled".into(),
                },
            ],
        );
        courier.send(Errand::Submit {
            peer: "pi5".into(),
            action: label(),
            confirmation: None,
        });
        assert!(matches!(next(&courier), Answer::Submitted { .. }));

        courier.send(Errand::Poll);
        courier.send(Errand::Poll);
        match next(&courier) {
            Answer::Resolved { standing, .. } => {
                assert!(matches!(standing, Standing::Done { .. }))
            }
            other => panic!("got {other:?}"),
        }
    }

    /// Once settled, it is not asked about again — a resolved line that
    /// kept being rewritten would read as an action still happening.
    #[test]
    fn a_settled_action_is_not_asked_about_again() {
        let (courier, _) = courier_with(
            accepted(),
            vec![Standing::Done {
                summary: "labelled".into(),
            }],
        );
        courier.send(Errand::Submit {
            peer: "pi5".into(),
            action: label(),
            confirmation: None,
        });
        assert!(matches!(next(&courier), Answer::Submitted { .. }));
        courier.send(Errand::Poll);
        assert!(matches!(next(&courier), Answer::Resolved { .. }));

        courier.send(Errand::Poll);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            courier.drain().is_empty(),
            "a settled action was asked about again"
        );
    }

    /// An `Unknown` submission has no acceptance to read a later answer
    /// against, so asking again cannot resolve it.
    #[test]
    fn an_unknown_submission_is_not_tracked_because_asking_cannot_settle_it() {
        let (courier, _) = courier_with(
            Submitted::Unknown {
                id: ActionId::new("desktop-1-1"),
                reason: "pi5: read timed out".into(),
            },
            vec![Standing::Done {
                summary: "labelled".into(),
            }],
        );
        courier.send(Errand::Submit {
            peer: "pi5".into(),
            action: label(),
            confirmation: None,
        });
        assert!(matches!(
            next(&courier),
            Answer::Submitted {
                outcome: Submitted::Unknown { .. },
                ..
            }
        ));

        courier.send(Errand::Poll);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(courier.drain().is_empty());
    }

    #[test]
    fn an_idle_courier_carries_to_nobody() {
        let courier = Courier::idle();
        assert!(!courier.carries_to("pi5"));
    }
}
