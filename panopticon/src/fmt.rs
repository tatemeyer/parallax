//! Small display-formatting helpers shared by every screen. Kept
//! crate-private and separate from any one screen so `overview`,
//! `detail`, and `bell` render the three-state distinction (not
//! declared / fetch failed / fetched) identically rather than each
//! re-deriving its own glyphs.

use parallax_baseline::state::ProjectState;
use std::time::Duration;

/// Not declared: the field is absent and nothing in `degradations`
/// explains why.
pub(crate) const DASH: &str = "\u{2014}"; // —
/// Fetch failed: declared, but this cycle could not read it.
pub(crate) const ALERT: &str = "!";

/// Whether a `Degradation` belonging to `family_prefix` (Baseline names
/// sources `"<family>:<detail>"`, e.g. `work:github`) exists for this
/// project -- what distinguishes "never declared" from "declared but
/// this cycle's fetch failed" for a family whose field is absent.
pub(crate) fn family_degraded(state: &ProjectState, family_prefix: &str) -> bool {
    state
        .degradations
        .iter()
        .any(|d| d.source.starts_with(family_prefix))
}

/// A compact age string: seconds/minutes/hours/days, whichever is the
/// coarsest unit that keeps the number small.
pub(crate) fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_picks_the_coarsest_unit_that_keeps_the_number_small() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m");
        assert_eq!(format_duration(Duration::from_secs(7200)), "2h");
        assert_eq!(format_duration(Duration::from_secs(172_800)), "2d");
    }

    #[test]
    fn family_degraded_matches_on_the_source_prefix() {
        use parallax_baseline::state::Degradation;
        let mut p = ProjectState {
            name: "x".into(),
            ..Default::default()
        };
        p.degradations.push(Degradation {
            source: "work:github".into(),
            reason: "boom".into(),
        });
        assert!(family_degraded(&p, "work"));
        assert!(!family_degraded(&p, "verification"));
    }
}
