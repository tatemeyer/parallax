//! The Cloister Bell: the blocker-overlay banner. It rings only for
//! degraded sources -- "impending catastrophe," per the platform design
//! -- and nothing else. Rendering here is a pure function of
//! `&PlatformState`; deciding *whether* the banner is currently visible
//! (dismissed vs. not, reappeared vs. not) is `App`'s job, because that
//! decision needs memory of the previous frame's degradation set, which
//! a pure renderer must not carry.

use crate::fmt::ALERT;
use parallax_baseline::state::PlatformState;
use std::collections::BTreeSet;
use ttui::buffer::Buffer;
use ttui::layout::Rect;
use ttui::widgets::text::Text;

/// One (project, source) pair with an active degradation -- the
/// identity a Bell dismissal is scoped to. Comparing two of these sets
/// is how `App` tells "still the same problem" from "something changed,"
/// which is what decides whether a dismissed Bell reappears.
pub type DegradationSet = BTreeSet<(String, String)>;

/// The platform's current degradation set: every `(project, source)`
/// pair with an active degradation, across every project.
pub fn degradation_set(platform: &PlatformState) -> DegradationSet {
    platform
        .degraded()
        .into_iter()
        .map(|(project, d)| (project.to_string(), d.source.clone()))
        .collect()
}

/// The project with the most degraded sources, and how many -- ties
/// broken in favour of whichever was registered first. `None` when
/// nothing is degraded.
pub fn worst_offender(platform: &PlatformState) -> Option<(&str, usize)> {
    let mut best: Option<(&str, usize)> = None;
    for p in &platform.projects {
        let n = p.degradations.len();
        if n == 0 {
            continue;
        }
        if best.is_none_or(|(_, best_n)| n > best_n) {
            best = Some((p.name.as_str(), n));
        }
    }
    best
}

/// The banner's text: total degraded-source count and the worst
/// offender. `None` when nothing is degraded -- a caller must not
/// render an empty banner over a clean screen.
pub fn banner_text(platform: &PlatformState) -> Option<String> {
    let total = degradation_set(platform).len();
    if total == 0 {
        return None;
    }
    let (name, n) = worst_offender(platform)?;
    let plural = if total == 1 { "" } else { "s" };
    Some(format!(
        "{ALERT} {total} source{plural} degraded -- worst: {name} ({n} down) -- Esc to dismiss"
    ))
}

/// Renders the banner into a fresh `width`x1 buffer. Blank (no
/// degradations) yields a blank buffer rather than an error, so a
/// caller that always renders can do so unconditionally -- but see
/// `App::bell_visible`: whether to blit this at all is still a
/// stateful decision this function does not make.
pub fn render_bell(platform: &PlatformState, width: u16) -> Buffer {
    let mut buf = Buffer::new(width, 1);
    if let Some(text) = banner_text(platform) {
        Text::new(&text).render(
            Rect {
                x: 0,
                y: 0,
                width,
                height: 1,
            },
            &mut buf,
        );
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallax_baseline::state::{Degradation, ProjectState};

    fn project(name: &str) -> ProjectState {
        ProjectState {
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn degraded(mut p: ProjectState, sources: &[&str]) -> ProjectState {
        for s in sources {
            p.degradations.push(Degradation {
                source: s.to_string(),
                reason: "unreachable".into(),
            });
        }
        p
    }

    #[test]
    fn a_clean_platform_has_an_empty_degradation_set() {
        let platform = PlatformState {
            projects: vec![project("a"), project("b")],
        };
        assert!(degradation_set(&platform).is_empty());
        assert!(banner_text(&platform).is_none());
        assert!(worst_offender(&platform).is_none());
    }

    #[test]
    fn degradation_set_names_every_project_source_pair() {
        let platform = PlatformState {
            projects: vec![
                degraded(project("a"), &["work:github"]),
                degraded(project("b"), &["verification:command:lint"]),
            ],
        };
        let set = degradation_set(&platform);
        assert_eq!(set.len(), 2);
        assert!(set.contains(&("a".to_string(), "work:github".to_string())));
        assert!(set.contains(&("b".to_string(), "verification:command:lint".to_string())));
    }

    #[test]
    fn worst_offender_is_the_project_with_the_most_degraded_sources() {
        let platform = PlatformState {
            projects: vec![
                degraded(project("a"), &["work:github"]),
                degraded(
                    project("b"),
                    &["verification:command:lint", "session:filesystem"],
                ),
            ],
        };
        assert_eq!(worst_offender(&platform), Some(("b", 2)));
    }

    #[test]
    fn worst_offender_ties_favour_earlier_registration() {
        let platform = PlatformState {
            projects: vec![
                degraded(project("first"), &["work:github"]),
                degraded(project("second"), &["work:github"]),
            ],
        };
        assert_eq!(worst_offender(&platform), Some(("first", 1)));
    }

    #[test]
    fn banner_text_reports_the_total_count_and_the_worst_offender() {
        let platform = PlatformState {
            projects: vec![
                degraded(project("a"), &["work:github"]),
                degraded(
                    project("b"),
                    &["verification:command:lint", "session:filesystem"],
                ),
            ],
        };
        let text = banner_text(&platform).unwrap();
        assert!(text.contains('3'), "total across both projects: {text}");
        assert!(text.contains('b'), "worst offender: {text}");
        assert!(text.contains("2 down"), "{text}");
    }

    #[test]
    fn banner_text_uses_the_singular_for_exactly_one_degradation() {
        let platform = PlatformState {
            projects: vec![degraded(project("a"), &["work:github"])],
        };
        let text = banner_text(&platform).unwrap();
        assert!(text.contains("1 source degraded"), "{text}");
        assert!(!text.contains("sources degraded"), "{text}");
    }

    #[test]
    fn render_bell_writes_the_banner_text_into_the_buffer() {
        let platform = PlatformState {
            projects: vec![degraded(project("a"), &["work:github"])],
        };
        let buf = render_bell(&platform, 80);
        assert_eq!(buf.get(0, 0).symbol, '!');
    }

    #[test]
    fn render_bell_on_a_clean_platform_is_blank() {
        let platform = PlatformState {
            projects: vec![project("a")],
        };
        let buf = render_bell(&platform, 20);
        for x in 0..20 {
            assert_eq!(buf.get(x, 0).symbol, ' ');
        }
    }

    #[test]
    fn render_bell_does_not_panic_at_zero_width() {
        let platform = PlatformState {
            projects: vec![degraded(project("a"), &["work:github"])],
        };
        let buf = render_bell(&platform, 0);
        assert_eq!(buf.width, 0);
    }
}
