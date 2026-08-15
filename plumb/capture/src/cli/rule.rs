//! `plumb rule`: records an operator's overrule of one finding as a
//! `Ruling`, looked up by `--fingerprint` out of the run's
//! already-written `verdict.md` (Task 12's renderer) so the operator
//! only has to name the fingerprint and their reasoning, not retype
//! the finding's scenario/region/claim/lens/severity. The ruling is
//! appended to `rulings.jsonl` and never touches this run (or any
//! future run's) prompt construction — `suppress` (wired into `plumb
//! merge`) is the only consumer, and it runs after every lens has
//! already reported.

use super::IoFailure;
use parallax_plumb::finding::{Lens, Severity};
use parallax_plumb::merge::fingerprint as compute_fingerprint;
use parallax_plumb::rulings::{self, Ruling, Scope};
use std::path::Path;

/// Failure recording a ruling.
#[derive(Debug)]
pub(super) enum RuleCliError {
    /// `--scope` was not `scenario` or `project-wide`.
    Usage(String),
    /// No finding in the run's `verdict.md` matched `--fingerprint`.
    NotFound(String),
    /// Reading `verdict.md`/`--taste`, or writing the ruling, failed.
    Io(IoFailure),
}

impl std::fmt::Display for RuleCliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleCliError::Usage(m) => write!(f, "{m}"),
            RuleCliError::NotFound(m) => write!(f, "{m}"),
            RuleCliError::Io(e) => write!(f, "{e}"),
        }
    }
}

fn parse_scope(s: &str) -> Result<Scope, RuleCliError> {
    match s {
        "scenario" => Ok(Scope::Scenario),
        "project-wide" => Ok(Scope::ProjectWide),
        _ => Err(RuleCliError::Usage(format!(
            "--scope {s:?} is not `scenario` or `project-wide`"
        ))),
    }
}

fn parse_lens(s: &str) -> Option<Lens> {
    match s {
        "breakage" => Some(Lens::Breakage),
        "intent" => Some(Lens::Intent),
        "design" => Some(Lens::Design),
        "motion" => Some(Lens::Motion),
        _ => None,
    }
}

fn parse_severity(s: &str) -> Option<Severity> {
    match s.to_lowercase().as_str() {
        "blocker" => Some(Severity::Blocker),
        "major" => Some(Severity::Major),
        "minor" => Some(Severity::Minor),
        "nit" => Some(Severity::Nit),
        _ => None,
    }
}

/// The fields of one finding, as recovered from a `verdict.md`
/// findings entry that fingerprints to the one the operator named.
struct FoundFinding {
    lens: Lens,
    severity: Severity,
    scenario: String,
    region: String,
    claim: String,
}

/// Scans `verdict.md`'s text (the exact shape `verdict::render_verdict`
/// writes: `- [SEVERITY] lens / region — claim` followed by a `
/// scenario: ` line) for the one finding whose recomputed fingerprint
/// matches `target`. `verdict.md` never prints the fingerprint itself,
/// so this recomputes it the same way `merge::fingerprint` always has,
/// from the same scenario/region/claim triple.
fn find_finding(verdict_text: &str, target: &str) -> Option<FoundFinding> {
    let lines: Vec<&str> = verdict_text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix("- [") else {
            continue;
        };
        let Some((sev_str, rest2)) = rest.split_once("] ") else {
            continue;
        };
        let Some((lens_str, rest3)) = rest2.split_once(" / ") else {
            continue;
        };
        let Some((region, claim)) = rest3.split_once(" — ") else {
            continue;
        };
        let Some(scenario) = lines
            .get(i + 1)
            .and_then(|l| l.trim().strip_prefix("scenario: "))
        else {
            continue;
        };
        if compute_fingerprint(scenario, region, claim) != target {
            continue;
        }
        let (Some(severity), Some(lens)) = (parse_severity(sev_str), parse_lens(lens_str)) else {
            continue;
        };
        return Some(FoundFinding {
            lens,
            severity,
            scenario: scenario.to_string(),
            region: region.to_string(),
            claim: claim.to_string(),
        });
    }
    None
}

/// Looks `target_fingerprint` up in `<run_dir>/verdict.md`, builds a
/// `Ruling` recording `reason`, `scope`, and a hash of `taste`'s
/// content (or the stable no-profile sentinel if `taste` is `None`),
/// and appends it to `rulings_path`.
pub(super) fn run_rule(
    run_dir: &Path,
    target_fingerprint: &str,
    reason: &str,
    scope: &str,
    taste: Option<&Path>,
    rulings_path: &Path,
) -> Result<Ruling, RuleCliError> {
    let scope = parse_scope(scope)?;

    let verdict_path = run_dir.join("verdict.md");
    let verdict_text = std::fs::read_to_string(&verdict_path).map_err(|source| {
        RuleCliError::Io(IoFailure {
            path: verdict_path.clone(),
            source,
        })
    })?;

    let found = find_finding(&verdict_text, target_fingerprint).ok_or_else(|| {
        RuleCliError::NotFound(format!(
            "no finding with fingerprint {target_fingerprint:?} in {}",
            verdict_path.display()
        ))
    })?;

    let taste_text = taste
        .map(|p| {
            std::fs::read_to_string(p).map_err(|source| {
                RuleCliError::Io(IoFailure {
                    path: p.to_path_buf(),
                    source,
                })
            })
        })
        .transpose()?;

    let ruling = Ruling {
        fingerprint: target_fingerprint.to_string(),
        lens: found.lens,
        severity: found.severity,
        scenario: found.scenario,
        region: found.region,
        claim: found.claim,
        reason: reason.to_string(),
        date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        taste_hash: rulings::taste_hash(taste_text.as_deref()),
        scope,
    };

    rulings::append_ruling(rulings_path, &ruling).map_err(|source| {
        RuleCliError::Io(IoFailure {
            path: rulings_path.to_path_buf(),
            source,
        })
    })?;

    Ok(ruling)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_plumb::finding::{Confidence, Finding};
    use parallax_plumb::merge::merge;
    use parallax_plumb::verdict::{write_verdict, VerdictInput};

    fn write_sample_verdict(run_dir: &Path) -> String {
        let findings = merge(vec![Finding {
            lens: Lens::Breakage,
            scenario: "dial".into(),
            severity: Severity::Blocker,
            region: "upper-right".into(),
            claim: "the border does not close".into(),
            evidence: "e".into(),
            confidence: Confidence::High,
        }]);
        let fp = findings[0].fingerprint.clone();
        let input = VerdictInput {
            run_id: "20260814T101500Z".into(),
            reports: Vec::new(),
            findings,
            suppressed: Vec::new(),
            stale_rulings: Vec::new(),
            dropped_no_region: 0,
            deferred: Vec::new(),
            capture_failures: Vec::new(),
        };
        write_verdict(&input, run_dir).unwrap();
        fp
    }

    #[test]
    fn rule_records_a_ruling_for_a_finding_present_in_verdict_md() {
        let tmp = tempfile::tempdir().unwrap();
        let run_dir = tmp.path().join("run1");
        std::fs::create_dir_all(&run_dir).unwrap();
        let fp = write_sample_verdict(&run_dir);
        let rulings_path = tmp.path().join("rulings.jsonl");

        let ruling = run_rule(
            &run_dir,
            &fp,
            "the border gap is intentional here",
            "scenario",
            None,
            &rulings_path,
        )
        .unwrap();

        assert_eq!(ruling.fingerprint, fp);
        assert_eq!(ruling.lens, Lens::Breakage);
        assert_eq!(ruling.severity, Severity::Blocker);
        assert_eq!(ruling.scenario, "dial");
        assert_eq!(ruling.scope, Scope::Scenario);

        let back = rulings::load_rulings(&rulings_path).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].fingerprint, fp);
    }

    #[test]
    fn rule_errors_when_the_fingerprint_is_not_in_verdict_md() {
        let tmp = tempfile::tempdir().unwrap();
        let run_dir = tmp.path().join("run1");
        std::fs::create_dir_all(&run_dir).unwrap();
        write_sample_verdict(&run_dir);
        let rulings_path = tmp.path().join("rulings.jsonl");

        let err = run_rule(
            &run_dir,
            "0000000000000000",
            "reason",
            "scenario",
            None,
            &rulings_path,
        )
        .unwrap_err();

        assert!(matches!(err, RuleCliError::NotFound(_)));
    }

    #[test]
    fn rule_rejects_an_unknown_scope_value() {
        let tmp = tempfile::tempdir().unwrap();
        let run_dir = tmp.path().join("run1");
        std::fs::create_dir_all(&run_dir).unwrap();
        let fp = write_sample_verdict(&run_dir);
        let rulings_path = tmp.path().join("rulings.jsonl");

        let err = run_rule(&run_dir, &fp, "reason", "everywhere", None, &rulings_path).unwrap_err();

        assert!(matches!(err, RuleCliError::Usage(_)));
    }

    #[test]
    fn rule_accepts_project_wide_scope() {
        let tmp = tempfile::tempdir().unwrap();
        let run_dir = tmp.path().join("run1");
        std::fs::create_dir_all(&run_dir).unwrap();
        let fp = write_sample_verdict(&run_dir);
        let rulings_path = tmp.path().join("rulings.jsonl");

        let ruling =
            run_rule(&run_dir, &fp, "reason", "project-wide", None, &rulings_path).unwrap();

        assert_eq!(ruling.scope, Scope::ProjectWide);
    }

    #[test]
    fn rule_hashes_a_given_taste_file_into_the_ruling() {
        let tmp = tempfile::tempdir().unwrap();
        let run_dir = tmp.path().join("run1");
        std::fs::create_dir_all(&run_dir).unwrap();
        let fp = write_sample_verdict(&run_dir);
        let rulings_path = tmp.path().join("rulings.jsonl");
        let taste_path = tmp.path().join("taste.md");
        std::fs::write(&taste_path, "Prefer sharp corners.").unwrap();

        let ruling = run_rule(
            &run_dir,
            &fp,
            "reason",
            "scenario",
            Some(&taste_path),
            &rulings_path,
        )
        .unwrap();

        assert_eq!(
            ruling.taste_hash,
            rulings::taste_hash(Some("Prefer sharp corners."))
        );
        assert_ne!(ruling.taste_hash, rulings::taste_hash(None));
    }

    #[test]
    fn rule_errors_when_verdict_md_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let run_dir = tmp.path().join("no-such-run");
        let rulings_path = tmp.path().join("rulings.jsonl");

        let err = run_rule(&run_dir, "abc", "reason", "scenario", None, &rulings_path).unwrap_err();

        assert!(matches!(err, RuleCliError::Io(_)));
    }
}
