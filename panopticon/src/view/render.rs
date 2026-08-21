//! Drawing a frame.
//!
//! The panes compose their rows into full-width lines and render them
//! through TTUI's `List`, which lends a selection highlight and clips to
//! the area. `Table` was the obvious fit and does not work here: it
//! applies one `col_width` to every column, so a row holding `#165` and
//! a ninety-character title has to choose between a four-cell title and
//! four ninety-cell columns. Filed upstream as tatemeyer/ttui#170 with
//! this use case attached; working around it costs one `format!` per row.

use crate::view::model::{Declared, Health, RailRow};
use crate::view::sanitize::sanitize;
use crate::view::status::SourceCell;
use crate::view::{artifacts, metrics, model, sessions, status, verification, work};
use parallax_baseline::state::{PlatformState, ProjectState};
use std::time::SystemTime;
use ttui::buffer::{Buffer, Cell};
use ttui::layout::{Constraint, Direction, Layout, Rect};
use ttui::theme::Theme;
use ttui::widgets::block::Block;
use ttui::widgets::list::List;

/// Which detail pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// What is in flight.
    Work,
    /// Where each declared check stands.
    Verification,
    /// What runs produced.
    Artifacts,
    /// Which agent sessions are live.
    Sessions,
    /// What this session has done. The one tab that is not about the
    /// selected project — every entry names its own.
    Log,
    /// What the experiments measured. The one tab that is not about
    /// software being built.
    Metrics,
}

impl Tab {
    /// The six tabs, in the order `1`–`6` select them.
    ///
    /// `Metrics` is last so that every existing key keeps its meaning:
    /// `1`–`4` and `5`-for-the-log are unchanged, and the new pane
    /// takes the first free number. Position is the mapping — key `n`
    /// is `ALL[n - 1]` — so there is no off-by-one to remember.
    pub const ALL: [Tab; 6] = [
        Tab::Work,
        Tab::Verification,
        Tab::Artifacts,
        Tab::Sessions,
        Tab::Log,
        Tab::Metrics,
    ];

    /// The tab's label in the detail header.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Work => "WORK",
            Tab::Verification => "VERIFY",
            Tab::Artifacts => "ARTIFACTS",
            Tab::Sessions => "SESSIONS",
            Tab::Log => "LOG",
            Tab::Metrics => "METRICS",
        }
    }
}

/// Everything one frame needs. Borrowed, so rendering allocates only the
/// strings it draws.
pub struct Frame<'a> {
    /// Every registered project.
    pub platform: &'a PlatformState,
    /// Which project the rail has selected.
    pub selected: usize,
    /// Which detail pane is showing.
    pub tab: Tab,
    /// What the selected project's manifest declares.
    pub declared: Declared,
    /// Checks that run a build and have not been asked to this session.
    pub pending_checks: &'a [String],
    /// The instant the frame is rendered as of.
    pub now: SystemTime,
    /// Which row of the detail pane is selected.
    pub detail_selected: usize,
    /// What this session has attempted, oldest first, as
    /// `(summary, result, mark)`. Rendered strings rather than actions:
    /// the render path is not allowed to name an action, and does not
    /// need to.
    ///
    /// The mark arrives already chosen rather than as a `bool` the
    /// renderer turns into one, because there are four states and the
    /// two that are neither success nor failure — accepted-and-running,
    /// and answer-lost — are the ones a boolean would have to lie about.
    pub log: &'a [(String, String, &'static str)],
    /// The question on screen, when one is being asked. Presentation
    /// only — the answer is handled in `control`, not here.
    pub question: Option<&'a str>,
    /// Whether the Cloister Bell is currently ringing.
    ///
    /// Presentation only, and deliberately meagre: a line in the footer
    /// and nothing else. It never opens a modal and never swallows a
    /// keystroke, because the operator has to keep working while
    /// something is on fire.
    pub alarm: bool,
}

impl Frame<'_> {
    /// The selected project, when there is one.
    pub fn project(&self) -> Option<&ProjectState> {
        self.platform.projects.get(self.selected)
    }
}

/// Below this the rail is dropped and the detail pane gets the width.
const RAIL_WIDTH: u16 = 18;
/// Below this nothing useful fits at all.
const MIN_WIDTH: u16 = 24;
/// Header, one row of content, footer.
const MIN_HEIGHT: u16 = 6;

/// Draws a whole frame.
///
/// Degrades rather than panicking on a small terminal: it drops the rail
/// first, then falls back to one honest line. A zero-width pane renders
/// nothing at all, which reads as a bug rather than as a small window.
pub fn render(frame: &Frame<'_>, area: Rect, buf: &mut Buffer) {
    let theme = Theme::default();

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        // Short on purpose: the message has to survive the width that
        // triggered it, and a clipped "terminal too sm" is worse than
        // no message at all.
        put_str(buf, area.x, area.y, "too small", &area);
        return;
    }

    let rows = Layout::new(
        Direction::Vertical,
        vec![Constraint::Fill(1), Constraint::Fixed(3)],
    )
    .split(area);
    let (body, footer_area) = (rows[0], rows[1]);

    let detail_area = if area.width >= RAIL_WIDTH * 2 {
        let columns = Layout::new(
            Direction::Horizontal,
            vec![Constraint::Fixed(RAIL_WIDTH), Constraint::Fill(1)],
        )
        .split(body);
        render_rail(frame, columns[0], &theme, buf);
        columns[1]
    } else {
        body
    };

    render_detail(frame, detail_area, &theme, buf);
    render_footer(frame, footer_area, &theme, buf);
}

fn render_rail(frame: &Frame<'_>, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let inner = Block::new()
        .title("PROJECTS")
        .theme(theme)
        .render(area, buf);
    let rows = model::rail_rows(frame.platform, frame.now);
    // Elided like the detail pane, not left to clip. A qualified name
    // is the longest thing this pane ever holds — `ttui@tates-laptop`
    // is 17 columns and the rail is 18 — and a row cut off without
    // saying so reads as a name that simply is that short.
    let lines: Vec<String> = rows
        .iter()
        .map(|row| elide(&rail_line(row), inner.width))
        .collect();
    List::new(&lines, frame.selected).render(inner, buf);
}

fn rail_line(row: &RailRow) -> String {
    let glyph = match row.health {
        Health::Ok => "ok",
        Health::Pending => "..",
        Health::Broken => "!!",
    };
    format!("{glyph} {}", row.name)
}

fn render_detail(frame: &Frame<'_>, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let title = detail_title(frame);
    let inner = Block::new()
        .title(&sanitize(&title))
        .theme(theme)
        .render(area, buf);

    let Some(project) = frame.project() else {
        put_str(
            buf,
            inner.x,
            inner.y,
            "no projects registered — pass --projects-root or --fixtures",
            &inner,
        );
        return;
    };

    let lines: Vec<String> = match frame.tab {
        Tab::Work => work_lines(frame, project),
        Tab::Verification => verification_lines(frame, project),
        Tab::Artifacts => artifact_lines(project),
        Tab::Metrics => metrics_lines(frame, project),
        Tab::Sessions => session_lines(frame, project),
        Tab::Log => log_lines(frame),
    }
    .iter()
    .map(|line| elide(line, inner.width))
    .collect();
    let (visible, selected) = window(&lines, frame.detail_selected, inner.height as usize);
    List::new(visible, selected).render(inner, buf);
}

/// The run of `lines` that fits `height`, positioned so the selected
/// one is inside it, and where the selection sits within that run.
///
/// `List` draws from the top and takes as many rows as fit, so a pane
/// holding more lines than that simply lost the rest — silently, with
/// the highlight nowhere on screen and `j` appearing to do nothing.
/// Every pane was short enough for that not to show until a real
/// metrics feed arrived with a hundred and thirteen lines in it.
///
/// The window moves only as far as it has to: while the selection fits
/// on the first screenful this is the identity, which is why no frame
/// that already rendered moves.
fn window(lines: &[String], selected: usize, height: usize) -> (&[String], usize) {
    if height == 0 || lines.len() <= height {
        return (lines, selected);
    }
    // Scrolled to keep the selection on the last row, then clamped so
    // the final screenful is full rather than trailing off the bottom.
    let start = selected
        .saturating_sub(height - 1)
        .min(lines.len() - height);
    (&lines[start..start + height], selected - start)
}

/// The tab strip, with the showing tab bracketed.
fn detail_title(frame: &Frame<'_>) -> String {
    // Qualified, like the rail: the header is how an operator confirms
    // which machine's `sesh` they are about to press a key at.
    let name = frame
        .project()
        .map(|p| p.qualified_name())
        .unwrap_or_else(|| "—".to_string());
    let tabs: Vec<String> = Tab::ALL
        .iter()
        .map(|t| {
            if *t == frame.tab {
                format!("[{}]", t.label())
            } else {
                format!(" {} ", t.label())
            }
        })
        .collect();
    format!("{name}  {}", tabs.join(""))
}

fn work_lines(frame: &Frame<'_>, project: &ProjectState) -> Vec<String> {
    if !frame.declared.work {
        return vec!["not declared".to_string()];
    }
    let rows = work::work_rows(project);
    if rows.is_empty() {
        return vec!["nothing in flight".to_string()];
    }
    let mut lines: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "{}{:<5} {:<6} {:<10} {:<14} {:<12} {:<7} {}",
                r.kind, r.number, r.state, r.implement, r.merge, r.readiness, r.checks, r.title
            )
        })
        .collect();
    if !project.unmapped_labels.is_empty() {
        lines.push(format!("unmapped: {}", project.unmapped_labels.join(", ")));
    }
    lines
}

fn verification_lines(frame: &Frame<'_>, project: &ProjectState) -> Vec<String> {
    if !frame.declared.verification {
        return vec!["not declared".to_string()];
    }
    verification::verification_rows(project, frame.pending_checks)
        .iter()
        .map(|r| {
            let standing = match r.standing {
                verification::Standing::Pass => "pass",
                verification::Standing::Fail => "FAIL",
                verification::Standing::Hold => "HOLD",
                verification::Standing::NotRun => "never run",
                verification::Standing::NotRunThisSession => "not run this session",
            };
            match &r.detail {
                Some(detail) => format!("{:<12} {:<20} {}", r.kind, standing, detail),
                None => format!("{:<12} {}", r.kind, standing),
            }
        })
        .collect()
}

fn artifact_lines(project: &ProjectState) -> Vec<String> {
    let rows = artifacts::artifact_rows(project);
    if rows.is_empty() {
        return vec!["nothing produced yet".to_string()];
    }
    rows.iter()
        .map(|r| format!("{:<9} {:<26} {}", r.kind, r.name, r.summary))
        .collect()
}

/// What the experiments measured.
///
/// Two ages in the feed's header, because a curve read two seconds ago
/// from a run that died an hour back is fresh and stalled at once. Then
/// one block per metric, its rows drawn against that metric's own
/// scale.
fn metrics_lines(frame: &Frame<'_>, project: &ProjectState) -> Vec<String> {
    let feeds = metrics::metric_feeds(project, frame.now);
    if feeds.is_empty() {
        return vec!["no metrics feeds".to_string()];
    }
    metrics::metric_lines(&feeds)
        .iter()
        .map(metric_line)
        .collect()
}

/// The column a series' label is given, and cut off at.
///
/// **Cut off, not merely padded.** A real producer's dimensions ran to
/// 197 characters, and a row that overran this column pushed its own
/// point count, its band and its numbers off the right-hand edge —
/// leaving a pane of identical prefixes and no measurements on it. The
/// label is ordered most-distinguishing-first upstream so that what
/// survives the cut is the part that tells two rows apart.
///
/// The width is what is left over at the cockpit's declared 120
/// columns once the point count, the band and the three numbers have
/// theirs — fixed columns and an elision, the same arrangement the work
/// pane uses for a title.
const LABEL: u16 = 42;

/// One line of the metrics pane.
fn metric_line(line: &metrics::MetricLine<'_>) -> String {
    match line {
        metrics::MetricLine::Feed(feed) => {
            let produced = match feed.produced {
                Some(age) => format!("produced {}s ago", age.as_secs()),
                // Not "0s", and not a date from 1970. Nobody could say.
                None => "produced unknown".to_string(),
            };
            // The series count is here so that a pane showing the first
            // twenty rows of a hundred and six says which of those it is
            // showing.
            format!(
                "{}   read {}s ago · {produced} · {} metrics, {} series",
                feed.name,
                feed.read.as_secs(),
                feed.groups.len(),
                feed.series()
            )
        }
        metrics::MetricLine::Group(group) => format!("  {}", group.name),
        metrics::MetricLine::Row { row, axis } => {
            let label = if row.label.is_empty() {
                "—".to_string()
            } else {
                row.label.clone()
            };
            format!(
                "    {:<width$} {:>4}  {}",
                elide(&label, LABEL),
                row.points,
                shape_line(&row.shape, *axis),
                width = LABEL as usize,
            )
        }
    }
}

/// One row's values, in the only form its shape licenses.
fn shape_line(shape: &metrics::RowShape, axis: Option<(f64, f64)>) -> String {
    match shape {
        // A curve is the only shape that earns a sparkline, and the
        // point count printed beside it is what keeps the summary
        // honest when fifty cells stand in for four thousand points.
        metrics::RowShape::Curve { by, points } => {
            format!("{} (by {by})", spark(points, axis))
        }
        // Positioned on the group's shared scale, not just printed.
        // Three variants whose numbers overlap are three bands that
        // visibly overlap, which is the difference between a reader
        // seeing a null result and a reader doing arithmetic.
        metrics::RowShape::Spread { min, median, max } => {
            format!(
                "{} {min:.4} {median:.4} {max:.4}",
                track(*min, *median, *max, axis)
            )
        }
        // No line, no bar: one measurement rendered as a value.
        metrics::RowShape::Single(value) => format!("{value:.4}"),
        // Not zero. Zero is a measurement.
        metrics::RowShape::Empty => "parsed, no points".to_string(),
    }
}

/// How many cells a spread's band is drawn across.
const TRACK: usize = 24;

/// One row's measurements as a band on its group's shared scale.
///
/// `├` and `┤` are the extremes, `●` the median, and position is what
/// carries the meaning: two variants whose bands sit on top of each
/// other did not differ, and one that sits clear of the rest did. That
/// comparison is the entire content of a null result, and it is not
/// available from three numbers printed in a column.
///
/// `┼` is a row whose whole spread fell inside one cell. It is neither
/// extreme because at this resolution there is only one place to be —
/// and a `├` with no `┤` after it reads as an interval running off the
/// end of the row, which is the opposite of what happened. The three
/// numbers beside it still carry the width the cell cannot.
fn track(min: f64, median: f64, max: f64, axis: Option<(f64, f64)>) -> String {
    let (lo, hi) = axis.unwrap_or((min, max));
    let span = hi - lo;
    let cell = |value: f64| -> usize {
        if span <= 0.0 {
            // Every measurement in the group identical. One mark at the
            // left rather than a band spanning a range nothing varied
            // across.
            return 0;
        }
        (((value - lo) / span) * (TRACK - 1) as f64).round() as usize
    };

    let (first, middle, last) = (cell(min), cell(median), cell(max));
    (0..TRACK)
        .map(|i| {
            // Narrower than a cell — including a row where nothing
            // varied at all, which lands every mark on cell zero.
            if first == last {
                return if i == first { '┼' } else { ' ' };
            }
            // Edges before the median. Where a median rounds onto an
            // extreme they are within a cell of each other anyway, and
            // a band that appears to stop at its median understates how
            // far the measurements actually reached — which is the one
            // thing this row exists to show.
            if i == first {
                '├'
            } else if i == last {
                '┤'
            } else if i == middle {
                '●'
            } else if i > first && i < last {
                '─'
            } else {
                ' '
            }
        })
        .collect()
}

/// A sparkline over a group's shared scale, so two rows of one metric
/// are comparable rather than each filling its own height.
fn spark(points: &[f64], axis: Option<(f64, f64)>) -> String {
    const RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let (min, max) = axis.unwrap_or((0.0, 0.0));
    let span = max - min;
    points
        .iter()
        .map(|point| {
            if span <= 0.0 {
                // Every point identical: the middle of the ramp says
                // "flat", where the bottom would read as "zero".
                return RAMP[RAMP.len() / 2];
            }
            let scaled = ((point - min) / span * (RAMP.len() - 1) as f64).round() as usize;
            RAMP[scaled.min(RAMP.len() - 1)]
        })
        .collect()
}

/// What this session has done. A cockpit that can act has to show what
/// it did, and a failure has to be as legible as a success — an action
/// that quietly did not happen is worse than one that visibly failed.
fn log_lines(frame: &Frame<'_>) -> Vec<String> {
    if frame.log.is_empty() {
        return vec!["nothing attempted this session".to_string()];
    }
    frame
        .log
        .iter()
        .map(|(summary, result, mark)| format!("{mark}  {summary}  —  {result}"))
        .collect()
}

fn session_lines(frame: &Frame<'_>, project: &ProjectState) -> Vec<String> {
    if !frame.declared.sessions {
        return vec!["not declared".to_string()];
    }
    let rows = sessions::session_rows(project, frame.now);
    if rows.is_empty() {
        return vec!["no sessions".to_string()];
    }
    rows.iter()
        .map(|r| {
            let state = if r.active { "active" } else { "idle" };
            format!("{:<26} {:<7} {}s", r.name, state, r.idle_for.as_secs())
        })
        .collect()
}

/// The footer renders every source's age, and a degraded source's
/// reason. It is the pane that makes the rest of the screen
/// trustworthy, so it never scrolls and never drops a row silently — it
/// says how many it could not fit.
fn render_footer(frame: &Frame<'_>, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let title = footer_title(frame);
    let inner = Block::new()
        .title(&sanitize(&title))
        .theme(theme)
        .render(area, buf);

    // A question outranks the source ages. The footer is the one strip
    // guaranteed to be on screen, and an operator being asked something
    // should not have to find where they were asked.
    if let Some(question) = frame.question {
        put_str(buf, inner.x, inner.y, &elide(question, inner.width), &inner);
        return;
    }

    let Some(project) = frame.project() else {
        return;
    };
    let cells = status::footer(project, frame.now);
    if cells.is_empty() {
        put_str(buf, inner.x, inner.y, "no sources declared", &inner);
        return;
    }
    let line = footer_line(&cells, inner.width);
    put_str(buf, inner.x, inner.y, &line, &inner);
}

/// Cuts a line to the pane and says that it did.
///
/// The same rule the footer already follows, applied to the one place
/// that was not following it: a title clipped flush against the border
/// is indistinguishable from a title that simply ended there, so the
/// operator cannot tell a whole sentence from half of one. `...` rather
/// than `…` deliberately — the ellipsis codepoint has no glyph in the
/// rasterizer the perceptual tier captures through, and a cockpit that
/// cannot be captured cannot be judged.
fn elide(line: &str, width: u16) -> String {
    const MARK: &str = "...";
    // Sanitised *before* measuring, not only on the way to the buffer.
    // An escape is several `char`s and zero display columns, so counting
    // the raw string truncates against a length that has nothing to do
    // with what is visible — the cell reads as full while showing almost
    // nothing, and the `...` claims there is more when there is not.
    let line = sanitize(line);
    let line = line.as_str();
    let width = width as usize;
    if line.chars().count() <= width {
        return line.to_string();
    }
    // Too narrow to say anything and mark it: the mark wins, because
    // "there is more here" outranks three characters of the more.
    if width <= MARK.len() {
        return MARK.chars().take(width).collect();
    }
    let keep = width - MARK.len();
    line.chars().take(keep).chain(MARK.chars()).collect()
}

/// The footer's title, naming whose blocker it is.
///
/// The box below it holds the *selected* project's sources, but the
/// bell rings for the platform — so an unmodified banner puts
/// `** BLOCKER **` over a healthy project's sources with nothing on
/// screen to account for it. Naming the projects turns a warning the
/// operator has to go looking for into one they can act on.
fn footer_title(frame: &Frame<'_>) -> String {
    if frame.question.is_some() {
        return "CONFIRM".to_string();
    }
    if !frame.alarm {
        return "SOURCES".to_string();
    }
    let broken: Vec<String> = model::rail_rows(frame.platform, frame.now)
        .into_iter()
        .filter(|r| r.health == Health::Broken)
        .map(|r| r.name)
        .collect();
    if broken.is_empty() {
        // The bell outlasts the fire by design, so it can still be
        // ringing once every project has recovered. Claiming a project
        // is broken when none is would be its own dishonesty.
        return "SOURCES  ** BLOCKER **".to_string();
    }
    format!("SOURCES  ** BLOCKER: {} **", broken.join(", "))
}

/// Joins source cells into one line, and says plainly how many did not
/// fit rather than truncating silently.
fn footer_line(cells: &[SourceCell], width: u16) -> String {
    /// Room kept back for `  (+12)`, so the count of what was hidden is
    /// not itself clipped off the end — which would hide the fact that
    /// anything was hidden.
    const RESERVE: usize = 7;

    let mut line = String::new();
    let mut shown = 0usize;
    for (i, cell) in cells.iter().enumerate() {
        let piece = format!("{} {}", cell.label, cell.age);
        let sep = if line.is_empty() { "" } else { "  ·  " };
        let reserve = if i + 1 < cells.len() { RESERVE } else { 0 };
        let needed = line.chars().count() + sep.chars().count() + piece.chars().count() + reserve;
        if needed > width as usize {
            break;
        }
        line.push_str(sep);
        line.push_str(&piece);
        shown += 1;
    }
    if shown < cells.len() {
        line.push_str(&format!("  (+{})", cells.len() - shown));
    }
    line
}

/// Writes a string at `(x, y)`, clipped to `area`.
///
/// Sanitised here, because almost everything the cockpit shows was
/// captured from somewhere else — command output, a pull request title,
/// a directory name, an adapter's error — and any of it can carry ANSI
/// escapes that would reach the terminal and steer it.
///
/// **This is not the only way a character becomes a cell**, which is
/// worth saying plainly rather than discovering later: `List` and
/// `Block` write to the buffer themselves. So the same call is made in
/// [`elide`], through which every list line passes, and on the two block
/// titles. Three call sites is a habit rather than a property, so
/// `tests/rendering.rs` asserts the end of it instead — that a frame
/// built from state where *every* observed field is packed with escapes
/// leaves no control character anywhere in the buffer, whichever path
/// drew it.
fn put_str(buf: &mut Buffer, x: u16, y: u16, text: &str, area: &Rect) {
    if y >= area.y + area.height {
        return;
    }
    for (i, ch) in sanitize(text).chars().enumerate() {
        let cx = x + i as u16;
        if cx >= area.x + area.width {
            break;
        }
        buf.set(
            cx,
            y,
            Cell {
                symbol: ch,
                alpha: 1.0,
                ..Default::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::window;

    fn lines(n: usize) -> Vec<String> {
        (0..n).map(|i| i.to_string()).collect()
    }

    /// The case every frame recorded before this existed is in: the
    /// pane fits, so the window is the identity and nothing moves.
    #[test]
    fn a_pane_that_fits_is_shown_whole() {
        let all = lines(5);
        let (visible, selected) = window(&all, 3, 10);
        assert_eq!(visible.len(), 5);
        assert_eq!(selected, 3);
    }

    /// While the selection is on the first screenful the window still
    /// does not move — a list that scrolled from the first keypress
    /// would make `j` feel like it moved the wrong thing.
    #[test]
    fn a_selection_on_the_first_screenful_does_not_scroll() {
        let all = lines(100);
        let (visible, selected) = window(&all, 4, 10);
        assert_eq!(visible[0], "0");
        assert_eq!(selected, 4);
    }

    /// The defect this exists for: a hundred and thirteen lines into a
    /// pane twenty-six tall used to show the first twenty-six, with the
    /// highlight nowhere and the other eighty-seven unreachable.
    #[test]
    fn a_selection_past_the_bottom_brings_its_line_onto_the_screen() {
        let all = lines(113);
        let (visible, selected) = window(&all, 90, 26);
        assert_eq!(visible.len(), 26);
        assert_eq!(visible[selected], "90");
    }

    /// The last screenful is a full one rather than a few rows trailing
    /// off the bottom.
    #[test]
    fn the_end_of_a_long_pane_still_fills_the_screen() {
        let all = lines(113);
        let (visible, selected) = window(&all, 112, 26);
        assert_eq!(visible.len(), 26);
        assert_eq!(visible[0], "87");
        assert_eq!(selected, 25);
    }

    /// A pane with no room is not a panic.
    #[test]
    fn no_height_is_no_rows() {
        let all = lines(3);
        let (visible, selected) = window(&all, 2, 0);
        assert_eq!(visible.len(), 3, "nothing to window against");
        assert_eq!(selected, 2);
    }
}
