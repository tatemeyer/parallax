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
use crate::view::status::SourceCell;
use crate::view::{artifacts, model, sessions, status, verification, work};
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
}

impl Tab {
    /// The four tabs, in the order `1`–`4` select them.
    pub const ALL: [Tab; 4] = [Tab::Work, Tab::Verification, Tab::Artifacts, Tab::Sessions];

    /// The tab's label in the detail header.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Work => "WORK",
            Tab::Verification => "VERIFY",
            Tab::Artifacts => "ARTIFACTS",
            Tab::Sessions => "SESSIONS",
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
    let lines: Vec<String> = rows.iter().map(rail_line).collect();
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
    let inner = Block::new().title(&title).theme(theme).render(area, buf);

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
        Tab::Sessions => session_lines(frame, project),
    }
    .iter()
    .map(|line| elide(line, inner.width))
    .collect();
    List::new(&lines, frame.detail_selected).render(inner, buf);
}

/// The tab strip, with the showing tab bracketed.
fn detail_title(frame: &Frame<'_>) -> String {
    let name = frame.project().map(|p| p.name.as_str()).unwrap_or("—");
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
    let inner = Block::new().title(&title).theme(theme).render(area, buf);
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
fn put_str(buf: &mut Buffer, x: u16, y: u16, text: &str, area: &Rect) {
    if y >= area.y + area.height {
        return;
    }
    for (i, ch) in text.chars().enumerate() {
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
