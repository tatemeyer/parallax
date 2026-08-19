//! The verification pane: where each declared check stands, and the
//! difference between a check that reported nothing and a check nobody
//! has asked to run.

use parallax_baseline::adapters::verification::VerificationOutcome;
use parallax_baseline::state::ProjectState;

/// Where one check stands, from the cockpit's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// It passed.
    Pass,
    /// It failed.
    Fail,
    /// It could not reach a conclusion. Never upgraded to a pass.
    Hold,
    /// The source says it has never run — no Plumb run exists yet.
    NotRun,
    /// It runs a build, and nobody has asked it to this session.
    ///
    /// Distinct from [`Standing::NotRun`] on purpose: a check whose last
    /// result predates the code on disk looks like an answer and is not
    /// one, so the cockpit shows the honest thing instead of a stale
    /// green.
    NotRunThisSession,
}

/// One row of the verification pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationRow {
    /// The manifest's display label for the check, e.g. `lint`.
    pub kind: String,
    /// Where it stands.
    pub standing: Standing,
    /// A one-line explanation, when the adapter had one.
    pub detail: Option<String>,
}

/// Every declared check's standing.
///
/// `pending` names the checks that run a build and have not been asked
/// to this session; anything in it that has no reported standing shows
/// as [`Standing::NotRunThisSession`].
pub fn verification_rows(project: &ProjectState, pending: &[String]) -> Vec<VerificationRow> {
    let mut rows: Vec<VerificationRow> = project
        .verification
        .iter()
        .map(|observed| VerificationRow {
            kind: observed.value.kind.clone(),
            standing: match observed.value.outcome {
                VerificationOutcome::Pass => Standing::Pass,
                VerificationOutcome::Fail => Standing::Fail,
                VerificationOutcome::Hold => Standing::Hold,
                VerificationOutcome::NotRun => Standing::NotRun,
            },
            detail: observed.value.detail.clone(),
        })
        .collect();

    for kind in pending {
        if !rows.iter().any(|r| &r.kind == kind) {
            rows.push(VerificationRow {
                kind: kind.clone(),
                standing: Standing::NotRunThisSession,
                detail: None,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::test_support::*;

    #[test]
    fn a_reported_check_renders_its_outcome() {
        let p = project_with(|p| {
            p.verification
                .push(watched(check("tests", VerificationOutcome::Fail), at(0)));
        });
        let rows = verification_rows(&p, &[]);
        assert_eq!(rows[0].kind, "tests");
        assert_eq!(rows[0].standing, Standing::Fail);
    }

    /// The distinction the spec insists on: Plumb has never run, versus
    /// we have not asked `cargo test` to run.
    #[test]
    fn never_run_and_not_asked_are_different_standings() {
        let p = project_with(|p| {
            p.verification.push(watched(
                check("perceptual", VerificationOutcome::NotRun),
                at(0),
            ));
        });
        let rows = verification_rows(&p, &["lint".to_string()]);
        assert_eq!(rows[0].standing, Standing::NotRun, "plumb has no runs");
        assert_eq!(
            rows[1].standing,
            Standing::NotRunThisSession,
            "lint was never asked"
        );
    }

    /// Once a build check has actually run, its reported standing wins;
    /// it must not be re-listed as unasked.
    #[test]
    fn a_check_that_has_run_is_not_also_listed_as_unasked() {
        let p = project_with(|p| {
            p.verification
                .push(watched(check("lint", VerificationOutcome::Pass), at(0)));
        });
        let rows = verification_rows(&p, &["lint".to_string()]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].standing, Standing::Pass);
    }

    #[test]
    fn a_detail_is_carried_through_rather_than_dropped() {
        let p = project_with(|p| {
            let mut status = check("perceptual", VerificationOutcome::Fail);
            status.detail = Some("20260814T112200Z".into());
            p.verification.push(watched(status, at(0)));
        });
        assert_eq!(
            verification_rows(&p, &[])[0].detail.as_deref(),
            Some("20260814T112200Z")
        );
    }

    #[test]
    fn a_project_declaring_no_checks_has_no_rows() {
        assert!(verification_rows(&bare_project("p"), &[]).is_empty());
    }
}
