//! Draws a view with ratatui, in the layout of the agreed mockup: rows of
//! changes with a 14-day bar and a state column, and a panel of counts and a
//! stacked chart underneath.

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::model::{
    End, Group, Kind, Panel, Row, Segment, Tone, View, WINDOW_DAYS, WINDOW_SECONDS,
};
use crate::time::DAY;

/// The smallest terminal the view draws in.
pub const MIN_WIDTH: u16 = 80;
/// The smallest terminal height the view draws in.
pub const MIN_HEIGHT: u16 = 20;

const PROJECT_WIDTH: usize = 14;
const TITLE_WIDTH: usize = 30;
const STATE_WIDTH: usize = 14;
const FIXED_LINES: usize = 12;
const CHART_LINES: usize = 6;

const FG: Color = Color::Rgb(0xc9, 0xd4, 0xd6);
const DIM: Color = Color::Rgb(0x6f, 0x81, 0x86);
const BRIGHT: Color = Color::Rgb(0xee, 0xf4, 0xf5);
const SELECTED_BG: Color = Color::Rgb(0x16, 0x26, 0x2d);
const ACCENT: Color = Color::Rgb(0x6c, 0xc0, 0xea);
const WARN: Color = Color::Rgb(0xe2, 0xb2, 0x3a);

fn kind_color(kind: Kind) -> Color {
    match kind {
        Kind::Queued => Color::Rgb(0x6a, 0x58, 0x38),
        Kind::Plan => Color::Rgb(0x3b, 0x6a, 0x84),
        Kind::Build => Color::Rgb(0x2f, 0x7a, 0xa8),
        Kind::Review => Color::Rgb(0x6a, 0x5a, 0x9c),
        Kind::Land => Color::Rgb(0x3f, 0x8f, 0x6a),
        Kind::Waiting => Color::Rgb(0xc9, 0x9a, 0x2e),
        Kind::Paused => Color::Rgb(0x4a, 0x56, 0x5a),
    }
}

fn tone_color(tone: Tone) -> Color {
    match tone {
        Tone::Running => ACCENT,
        Tone::Warn => WARN,
        Tone::Bad => Color::Rgb(0xe0, 0x60, 0x5a),
        Tone::Ok => Color::Rgb(0x5f, 0xd0, 0x8a),
        Tone::Dim => DIM,
    }
}

fn fit(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        format!("{text:<width$}")
    } else {
        let kept: String = text.chars().take(width.saturating_sub(1)).collect();
        format!("{kept}…")
    }
}

fn kind_in(segments: &[Segment], at: i64) -> Option<Kind> {
    segments
        .iter()
        .rev()
        .find(|segment| segment.from <= at && at < segment.to)
        .map(|segment| segment.kind)
}

fn wide(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The cell of a lane `width` cells wide that `seconds` into the window falls in.
fn cell_of(seconds: i64, width: usize) -> usize {
    let cell = seconds.clamp(0, WINDOW_SECONDS) * wide(width) / WINDOW_SECONDS;
    usize::try_from(cell)
        .unwrap_or(0)
        .min(width.saturating_sub(1))
}

/// The cells of a row's bar, as a colour or an empty cell.
fn lane_cells(row: &Row, width: usize) -> Vec<(char, Style)> {
    let cells_wide = wide(width.max(1));
    let mut cells: Vec<(char, Style)> = (0..cells_wide)
        .map(|index| {
            let middle = (2 * index + 1) * WINDOW_SECONDS / (2 * cells_wide);
            let (start, next) = (
                index * WINDOW_SECONDS / cells_wide,
                (index + 1) * WINDOW_SECONDS / cells_wide,
            );
            match kind_in(&row.segments, middle) {
                Some(kind) => {
                    let mut style = Style::new().fg(kind_color(kind));
                    if row.dim {
                        style = style.add_modifier(Modifier::DIM);
                    }
                    ('█', style)
                }
                None if index > 0 && start / (2 * DAY) != next / (2 * DAY) => {
                    ('┊', Style::new().fg(Color::Rgb(0x2a, 0x3a, 0x40)))
                }
                None => (' ', Style::new()),
            }
        })
        .collect();
    let (seconds, glyph, color) = match row.end {
        End::Landed(at) => (at, '✓', tone_color(Tone::Ok)),
        End::Dropped(at) => (at, '✗', tone_color(Tone::Bad)),
        End::Open => return cells,
    };
    if let Some(cell) = cells.get_mut(cell_of(seconds, width)) {
        *cell = (glyph, Style::new().fg(color));
    }
    cells
}

fn runs(cells: Vec<(char, Style)>) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut text = String::new();
    let mut style = None;
    for (glyph, cell_style) in cells {
        if style.is_some_and(|current| current != cell_style) {
            spans.push(Span::styled(
                std::mem::take(&mut text),
                style.unwrap_or_default(),
            ));
        }
        style = Some(cell_style);
        text.push(glyph);
    }
    if let Some(style) = style {
        spans.push(Span::styled(text, style));
    }
    spans
}

fn axis_line(lane: usize) -> Line<'static> {
    let mut chars = vec![' '; lane];
    for back in (2..=12).step_by(2) {
        let label = format!("-{back}d");
        let at = cell_of((WINDOW_DAYS - back) * DAY, lane);
        for (offset, glyph) in label.chars().enumerate() {
            if let Some(slot) = chars.get_mut(at + offset) {
                *slot = glyph;
            }
        }
    }
    for (offset, glyph) in "now".chars().rev().enumerate() {
        if let Some(slot) = lane
            .checked_sub(offset + 1)
            .and_then(|at| chars.get_mut(at))
        {
            *slot = glyph;
        }
    }
    let lane_text: String = chars.into_iter().collect();
    Line::from(vec![
        Span::styled(
            format!(
                "{} {} ",
                fit("project", PROJECT_WIDTH),
                fit("change", TITLE_WIDTH)
            ),
            Style::new().fg(DIM),
        ),
        Span::styled(lane_text, Style::new().fg(DIM)),
        Span::styled(
            format!(" {}", fit("state", STATE_WIDTH)),
            Style::new().fg(DIM),
        ),
    ])
}

fn row_line(row: &Row, label: &str, selected: bool, lane: usize) -> Line<'static> {
    let base = if row.dim { DIM } else { FG };
    let background = if selected { SELECTED_BG } else { Color::Reset };
    let (project_style, title_style) = if selected {
        (
            Style::new().fg(ACCENT),
            Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::new().fg(if label.is_empty() { DIM } else { base }),
            Style::new().fg(base),
        )
    };
    let mut spans = vec![
        Span::styled(format!("{} ", fit(label, PROJECT_WIDTH)), project_style),
        Span::styled(format!("{} ", fit(&row.title, TITLE_WIDTH)), title_style),
    ];
    spans.extend(runs(lane_cells(row, lane)));
    let state_color = if row.dim { DIM } else { tone_color(row.tone) };
    spans.push(Span::styled(
        format!(" {}", fit(&row.state, STATE_WIDTH)),
        Style::new().fg(state_color),
    ));
    Line::from(spans).style(Style::new().bg(background))
}

fn group_lines(group: &Group, view: &View, first_index: usize, lane: usize) -> Vec<Line<'static>> {
    group
        .rows
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let label = if offset == 0 {
                group.project.as_str()
            } else {
                ""
            };
            row_line(row, label, first_index + offset == view.selected, lane)
        })
        .collect()
}

/// One dim line that stands for a project's ended changes.
fn ended_line(group: &Group) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{} ", fit(&group.project, PROJECT_WIDTH)),
            Style::new().fg(DIM),
        ),
        Span::styled(
            format!(
                "+{} landed, {} dropped in {WINDOW_DAYS}d",
                group.landed, group.dropped
            ),
            Style::new().fg(DIM),
        ),
    ])
}

fn stat(label: &str, value: &str, color: Color, width: usize) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!("{label:<width$}"), Style::new().fg(DIM)),
        Span::styled(format!("{value:>4}"), Style::new().fg(color)),
    ]
}

fn count(value: Option<usize>) -> String {
    value.map_or_else(|| "?".to_owned(), |value| value.to_string())
}

fn chart_cells(panel: &Panel, lane: usize) -> Vec<Vec<(char, Style)>> {
    let kinds = [
        Kind::Plan,
        Kind::Build,
        Kind::Review,
        Kind::Land,
        Kind::Waiting,
    ];
    let totals: Vec<usize> = panel.days.iter().map(|day| day.iter().sum()).collect();
    let maximum = totals
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .max(panel.limit)
        .max(1);
    let eighths = CHART_LINES * 8;
    let scale = |value: usize| (value * eighths).div_ceil(maximum);
    let limit_row = (scale(panel.limit).saturating_sub(1) / 8).min(CHART_LINES - 1);
    let columns = panel.days.len().max(1);
    let mut lines = vec![vec![(' ', Style::new()); lane]; CHART_LINES];
    for (day, counts) in panel.days.iter().enumerate() {
        let start = day * lane / columns;
        let end = ((day + 1) * lane / columns).max(start + 1).min(lane);
        let mut bounds = [0usize; 5];
        let mut sum = 0;
        for (slot, count) in counts.iter().enumerate() {
            sum += count;
            bounds[slot] = scale(sum);
        }
        for (row_from_bottom, line) in lines.iter_mut().rev().enumerate() {
            let low = row_from_bottom * 8;
            let top = bounds[4].min(low + 8);
            if top <= low {
                continue;
            }
            let covering = kinds
                .iter()
                .enumerate()
                .map(|(slot, kind)| {
                    let slot_low = if slot == 0 { 0 } else { bounds[slot - 1] };
                    let overlap = bounds[slot].min(low + 8).saturating_sub(slot_low.max(low));
                    (overlap, *kind)
                })
                .max_by_key(|(overlap, _)| *overlap)
                .map_or(Kind::Plan, |(_, kind)| kind);
            let glyph = match top - low {
                8 => '█',
                7 => '▇',
                6 => '▆',
                5 => '▅',
                4 => '▄',
                3 => '▃',
                2 => '▂',
                _ => '▁',
            };
            for cell in line
                .iter_mut()
                .take(end.saturating_sub(1).max(start + 1))
                .skip(start)
            {
                *cell = (glyph, Style::new().fg(kind_color(covering)));
            }
        }
    }
    let marker = lines.len() - 1 - limit_row;
    for cell in &mut lines[marker] {
        if cell.0 == ' ' {
            *cell = ('┄', Style::new().fg(WARN));
        }
    }
    let label = format!("limit {}", panel.limit);
    for (offset, glyph) in label.chars().rev().enumerate() {
        if let Some(slot) = lane.checked_sub(offset + 1) {
            lines[marker][slot] = (glyph, Style::new().fg(WARN));
        }
    }
    lines
}

fn panel_lines(panel: &Panel, lane: usize) -> Vec<Line<'static>> {
    let left = [
        stat("in play", &panel.in_play.to_string(), BRIGHT, 9),
        stat("limit", &panel.limit.to_string(), WARN, 9),
        stat("projects", &panel.projects.to_string(), BRIGHT, 9),
        stat("queued", &count(panel.queued), BRIGHT, 9),
    ];
    let middle = [
        stat(
            "landed 14d",
            &panel.landed.to_string(),
            tone_color(Tone::Ok),
            22,
        ),
        stat("started 14d", &panel.started.to_string(), BRIGHT, 22),
        stat("idle > 3d", &panel.idle.to_string(), WARN, 22),
        stat("waiting on you", &count(panel.waiting), WARN, 22),
    ];
    let chart = chart_cells(panel, lane);
    let mut lines = Vec::new();
    for (index, chart_row) in chart.into_iter().enumerate() {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (slot, width) in [(&left, PROJECT_WIDTH + 1), (&middle, TITLE_WIDTH + 1)] {
            let mut used = 0;
            if let Some(parts) = slot.get(index) {
                for part in parts {
                    used += part.content.chars().count();
                    spans.push(part.clone());
                }
            }
            spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
        }
        spans.extend(runs(chart_row));
        lines.push(Line::from(spans));
    }
    let mut legend = vec![Span::raw(" ".repeat(PROJECT_WIDTH + TITLE_WIDTH + 2))];
    for (kind, name) in [
        (Kind::Queued, "queued"),
        (Kind::Plan, "plan"),
        (Kind::Build, "build"),
        (Kind::Review, "review"),
        (Kind::Land, "land"),
        (Kind::Waiting, "waiting on you"),
    ] {
        legend.push(Span::styled("█ ", Style::new().fg(kind_color(kind))));
        legend.push(Span::styled(format!("{name}  "), Style::new().fg(DIM)));
    }
    lines.push(Line::from(legend));
    lines
}

fn lines_for(view: &View, area: Rect) -> Vec<Line<'static>> {
    let width = usize::from(area.width);
    let lane = width
        .saturating_sub(PROJECT_WIDTH + TITLE_WIDTH + STATE_WIDTH + 3)
        .max(usize::try_from(WINDOW_DAYS).unwrap_or(14));
    let mut lines: Vec<Line<'static>> = Vec::new();

    let badge = if view.panel.over() {
        " OVER · WIP "
    } else {
        " WITHIN LIMIT "
    };
    let badge_style = if view.panel.over() {
        Style::new()
            .bg(WARN)
            .fg(Color::Rgb(0x1a, 0x14, 0x05))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(tone_color(Tone::Ok))
    };
    let grouped = if view.mapped {
        "grouped by the work map"
    } else {
        "grouped by repository"
    };
    let left = format!(" last {WINDOW_DAYS} days  {grouped} ");
    let pad = width.saturating_sub(left.chars().count() + badge.chars().count() + 7);
    lines.push(Line::from(vec![
        Span::styled(
            " board ",
            Style::new().bg(kind_color(Kind::Build)).fg(BRIGHT),
        ),
        Span::styled(left, Style::new().fg(DIM)),
        Span::raw(" ".repeat(pad)),
        Span::styled(badge, badge_style),
    ]));
    lines.push(axis_line(lane));

    let total_rows: usize = view.groups.iter().map(|group| group.rows.len()).sum();
    let capacity = usize::from(area.height).saturating_sub(FIXED_LINES);
    let mut body: Vec<Line<'static>> = Vec::new();
    let mut first_index = 0;
    let mut last_realm: Option<&str> = None;
    for group in view.groups.iter().filter(|group| !group.rows.is_empty()) {
        if view.mapped && last_realm != group.realm.as_deref() {
            if let Some(realm) = &group.realm {
                body.push(Line::from(Span::styled(
                    format!("── {realm} "),
                    Style::new().fg(DIM).add_modifier(Modifier::BOLD),
                )));
            }
            last_realm = group.realm.as_deref();
        }
        body.extend(group_lines(group, view, first_index, lane));
        first_index += group.rows.len();
    }
    body.extend(
        view.groups
            .iter()
            .filter(|group| group.has_ended())
            .map(ended_line),
    );
    let shown_rows = body.len();
    if shown_rows > capacity {
        let hidden = shown_rows - capacity.saturating_sub(1);
        body.truncate(capacity.saturating_sub(1));
        body.push(Line::from(Span::styled(
            format!(
                "… {hidden} more lines of {total_rows} changes do not fit; make the terminal taller"
            ),
            Style::new().fg(DIM),
        )));
    }
    let blank = capacity.saturating_sub(body.len());
    lines.extend(body);
    lines.extend(std::iter::repeat_with(Line::default).take(blank));

    lines.push(Line::from(Span::styled(
        "─".repeat(width),
        Style::new().fg(Color::Rgb(0x1f, 0x2c, 0x31)),
    )));
    lines.extend(panel_lines(&view.panel, lane));
    let why_color = if view.panel.over() { WARN } else { DIM };
    lines.push(Line::from(Span::styled(
        format!(" {}", view.panel.why),
        Style::new().fg(why_color),
    )));
    let mut keys = vec![
        Span::styled(" q", Style::new().fg(ACCENT)),
        Span::styled(" quit", Style::new().fg(DIM)),
    ];
    if !view.freshness.is_empty() {
        keys.push(Span::styled(
            format!("   {}", view.freshness),
            Style::new().fg(DIM),
        ));
    }
    if let Some(problem) = view.status.first() {
        keys.push(Span::styled(format!("   {problem}"), Style::new().fg(WARN)));
    }
    lines.push(Line::from(keys));
    lines
}

/// Draws the view into the frame.
pub fn draw(frame: &mut Frame, view: &View) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let text = format!("terminal too small: board-top needs {MIN_WIDTH}x{MIN_HEIGHT}");
        frame.render_widget(Paragraph::new(text), area);
        return;
    }
    frame.render_widget(Paragraph::new(lines_for(view, area)), area);
}

fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            let line: String = (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect();
            line.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The view drawn once on a terminal of the given size, as plain text.
#[must_use]
pub fn render_text(view: &View, width: u16, height: u16) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).unwrap_or_else(|never| match never {});
    let _ = terminal.draw(|frame| draw(frame, view));
    buffer_text(terminal.backend().buffer())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Inputs, build};
    use crate::parse::{OpenChange, PhaseEvent};

    const NOW: i64 = 1_794_564_745;

    fn change(path: &str, phase: &str, mount: &str) -> OpenChange {
        OpenChange {
            path: path.to_owned(),
            title: format!("title of {path}"),
            mount: mount.to_owned(),
            phase: phase.to_owned(),
            task: None,
        }
    }

    fn sample() -> View {
        let mut input = Inputs {
            now: NOW,
            limit: 1,
            ..Inputs::default()
        };
        input.open = vec![
            change("/changes/a", "build", "/repos/board"),
            change("/changes/b", "plan", "/repos/dash"),
        ];
        input.closed = vec![change("/changes/l", "done", "/repos/board")];
        input.history.insert(
            "/changes/a".to_owned(),
            vec![PhaseEvent {
                at: NOW - 3 * 86_400,
                from: Some("plan".to_owned()),
                to: "build".to_owned(),
            }],
        );
        input.history.insert(
            "/changes/l".to_owned(),
            vec![
                PhaseEvent {
                    at: NOW - 9 * 86_400,
                    from: None,
                    to: "plan".to_owned(),
                },
                PhaseEvent {
                    at: NOW - 5 * 86_400,
                    from: Some("plan".to_owned()),
                    to: "done".to_owned(),
                },
            ],
        );
        build(&input)
    }

    #[test]
    fn draws_the_header_rows_panel_and_over_limit_line() {
        let text = render_text(&sample(), 110, 26);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].contains("last 14 days"), "{text}");
        assert!(lines[0].contains("OVER · WIP"), "{text}");
        assert!(
            lines[1].contains("project") && lines[1].contains("now") && lines[1].contains("state")
        );
        assert!(text.contains("board") && text.contains("title of /changes/a"));
        assert!(text.contains("build 3d"), "{text}");
        assert!(text.contains("+1 landed, 0 dropped in 14d"), "{text}");
        assert!(
            text.contains("in play") && text.contains("waiting on you"),
            "{text}"
        );
        assert!(text.contains("limit 1"), "{text}");
        assert!(
            text.contains("over because: 2 changes are in play,"),
            "{text}"
        );
        assert!(
            lines.last().unwrap().trim_start().starts_with("q quit"),
            "{text}"
        );
    }

    #[test]
    fn the_status_line_shows_how_old_the_data_is() {
        let mut view = sample();
        view.freshness = "updated 40s ago · loading".to_owned();
        let text = render_text(&view, 110, 26);
        assert!(text.contains("updated 40s ago · loading"), "{text}");
    }

    #[test]
    fn a_small_terminal_gets_one_line() {
        let text = render_text(&sample(), 60, 15);
        assert!(text.starts_with("terminal too small"), "{text}");
    }

    #[test]
    fn rows_that_do_not_fit_are_counted() {
        let mut input = Inputs {
            now: NOW,
            limit: 12,
            ..Inputs::default()
        };
        input.open = (0..12)
            .map(|n| change(&format!("/changes/c{n}"), "plan", "/repos/board"))
            .collect();
        let text = render_text(&build(&input), 110, 20);
        assert!(text.contains("do not fit"), "{text}");
        assert!(text.contains("of 12 changes"), "{text}");
    }
}
