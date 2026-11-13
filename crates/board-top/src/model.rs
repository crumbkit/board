//! The view's model: one row per change, grouped, with a bar of segments over
//! the window and a panel of counts. It is built from parsed data only, so it
//! is tested without a terminal, a board or git.

use std::collections::{HashMap, HashSet};

use crate::parse::{OpenChange, PhaseEvent, Questions, WorkMap};
use crate::time::{DAY, age};

/// Days the bars and the panel cover.
pub const WINDOW_DAYS: i64 = 14;
/// Seconds the window covers.
pub const WINDOW_SECONDS: i64 = WINDOW_DAYS * DAY;
/// A change that has not moved for more than this many days is idle.
pub const IDLE_DAYS: i64 = 3;

/// What a stretch of a bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The task waited before a change claimed it.
    Queued,
    /// The change was in plan.
    Plan,
    /// The change was in build.
    Build,
    /// The change was in review.
    Review,
    /// The change was in land.
    Land,
    /// An open question waited on the owner.
    Waiting,
    /// The change was paused.
    Paused,
}

impl Kind {
    fn of_phase(phase: &str) -> Self {
        match phase {
            "plan" => Self::Plan,
            "build" => Self::Build,
            "review" => Self::Review,
            "land" => Self::Land,
            _ => Self::Paused,
        }
    }
}

/// A stretch of a bar, in seconds from the start of the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// What the stretch shows.
    pub kind: Kind,
    /// Where it starts.
    pub from: i64,
    /// Where it ends.
    pub to: i64,
}

/// How a row's change ended, if it did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum End {
    /// Still open.
    Open,
    /// Landed this many seconds from the window start.
    Landed(i64),
    /// Dropped this many seconds from the window start.
    Dropped(i64),
}

/// The colour family of a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Moving normally.
    Running,
    /// Needs attention.
    Warn,
    /// Stuck or dropped.
    Bad,
    /// Landed.
    Ok,
    /// Quiet.
    Dim,
}

/// One change.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The record the row shows; later navigation and clicks open it.
    pub path: String,
    /// The change's title.
    pub title: String,
    /// The bar.
    pub segments: Vec<Segment>,
    /// How it ended, if it did.
    pub end: End,
    /// The right-hand column.
    pub state: String,
    /// The state's colour family.
    pub tone: Tone,
    /// Whether the row is drawn dim, as when the map says it already landed.
    pub dim: bool,
    /// The change's phase when the frame was drawn.
    pub phase: String,
    /// Whether it has been open and unmoved for more than `IDLE_DAYS`.
    pub idle: bool,
    /// Whether the change was created inside the window.
    pub started: bool,
    last_move: i64,
}

impl Row {
    /// Whether the change is in play: open and in plan, build, review or land.
    #[must_use]
    pub fn in_play(&self) -> bool {
        matches!(self.end, End::Open) && self.phase != "paused"
    }
}

/// A project's rows under its realm.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The realm's name, when a work map gave one.
    pub realm: Option<String>,
    /// The project id, or the mount without a map.
    pub project: String,
    /// The rows, longest-unmoved first.
    pub rows: Vec<Row>,
    /// Changes of this project that landed in the window and have no row.
    pub landed: usize,
    /// Changes of this project that were dropped in the window and have no row.
    pub dropped: usize,
}

impl Group {
    /// Whether the group has ended changes to show as one line.
    #[must_use]
    pub fn has_ended(&self) -> bool {
        self.landed + self.dropped > 0
    }
}

/// The panel under the rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panel {
    /// Changes in play.
    pub in_play: usize,
    /// The allowed number in play.
    pub limit: usize,
    /// Open projects.
    pub projects: usize,
    /// Open tasks no open change claims; `None` when tasks could not be read.
    pub queued: Option<usize>,
    /// Changes that landed in the window.
    pub landed: usize,
    /// Changes created in the window.
    pub started: usize,
    /// Changes idle for more than `IDLE_DAYS`.
    pub idle: usize,
    /// Questions waiting on the owner; `None` when none could be counted.
    pub waiting: Option<usize>,
    /// Per day, the changes in play by plan, build, review, land and waiting.
    pub days: Vec<[usize; 5]>,
    /// Why the board is over the limit, or that it is not.
    pub why: String,
}

impl Panel {
    /// Whether more changes are in play than the limit allows.
    #[must_use]
    pub fn over(&self) -> bool {
        self.in_play > self.limit
    }
}

/// Everything a frame shows.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The groups, in display order.
    pub groups: Vec<Group>,
    /// The panel.
    pub panel: Panel,
    /// Problems to show on the status line.
    pub status: Vec<String>,
    /// How old the data is and whether a load runs, for the status line.
    pub freshness: String,
    /// Whether groups come from a work map.
    pub mapped: bool,
    /// The index of the highlighted row among all rows.
    pub selected: usize,
    /// The first body line drawn when the rows do not all fit.
    pub scroll: usize,
    /// The first body line the last `reveal` showed.
    pub shown: usize,
    /// The search query; empty when no filter is on.
    pub filter: String,
    /// Whether the search prompt is open and keys are text.
    pub typing: bool,
}

impl View {
    /// Keeps the highlight on the record it was on in `old`, or at the same
    /// place when that record has gone.
    pub fn keep_selection(&mut self, old: &View) {
        self.filter.clone_from(&old.filter);
        self.typing = old.typing;
        let last = self.rows().count().saturating_sub(1);
        self.selected = old
            .selected_path()
            .and_then(|path| self.rows().position(|row| row.path == path))
            .unwrap_or_else(|| old.selected.min(last));
        self.scroll = if old.scroll == usize::MAX && self.selected != last {
            old.shown
        } else {
            old.scroll
        };
        self.shown = old.shown;
    }

    /// Every row that passes the filter, in display order.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.groups
            .iter()
            .flat_map(move |group| group.rows.iter().filter(move |row| self.passes(group, row)))
    }

    /// The record path of the highlighted row.
    #[must_use]
    pub fn selected_path(&self) -> Option<&str> {
        self.rows().nth(self.selected).map(|row| row.path.as_str())
    }
}

/// What the data layer gathered.
#[derive(Debug, Default)]
pub struct Inputs {
    /// Seconds since the epoch of the frame.
    pub now: i64,
    /// The allowed number in play.
    pub limit: usize,
    /// The open changes.
    pub open: Vec<OpenChange>,
    /// Changes that ended inside the window, as their records read.
    pub closed: Vec<OpenChange>,
    /// Phase moves by full change path.
    pub history: HashMap<String, Vec<PhaseEvent>>,
    /// When each task was created, by task path.
    pub task_created: HashMap<String, i64>,
    /// The work map, when there is one.
    pub map: Option<WorkMap>,
    /// The open questions, when they could be read.
    pub questions: Option<Questions>,
    /// The open tasks, when they could be read.
    pub open_tasks: Option<Vec<String>>,
    /// Problems the data layer met.
    pub status: Vec<String>,
}

/// The paths in `history` whose last move ended the change and that are not
/// open: the changes that landed or were dropped inside the window.
#[must_use]
pub fn ended_paths<'a>(
    open: &[OpenChange],
    history: impl IntoIterator<Item = (&'a String, &'a Vec<PhaseEvent>)>,
    window_start: i64,
) -> Vec<String> {
    let open: HashSet<&str> = open.iter().map(|change| change.path.as_str()).collect();
    let mut ended: Vec<String> = history
        .into_iter()
        .filter(|(path, events)| {
            !open.contains(path.as_str())
                && events
                    .last()
                    .is_some_and(|event| is_terminal(&event.to) && event.at >= window_start)
        })
        .map(|(path, _)| path.clone())
        .collect();
    ended.sort();
    ended
}

fn is_terminal(phase: &str) -> bool {
    matches!(phase, "done" | "dropped")
}

struct Span {
    from: i64,
    to: i64,
    phase: String,
}

struct Timeline {
    spans: Vec<Span>,
    created: Option<i64>,
    ended: Option<(i64, String)>,
    clipped: bool,
}

fn timeline(events: &[PhaseEvent], current: &str, window_start: i64, now: i64) -> Timeline {
    let mut spans = Vec::new();
    let (mut created, mut ended) = (None, None);
    let mut open: Option<(i64, String)> = None;
    match events.first() {
        None => {
            if !is_terminal(current) {
                open = Some((window_start, current.to_owned()));
            }
        }
        Some(first) => {
            match &first.from {
                Some(phase) => open = Some((window_start, phase.clone())),
                None => created = Some(first.at),
            }
            for event in events {
                if let Some((from, phase)) = open.take() {
                    spans.push(Span {
                        from,
                        to: event.at,
                        phase,
                    });
                }
                if is_terminal(&event.to) {
                    ended = Some((event.at, event.to.clone()));
                } else {
                    open = Some((event.at, event.to.clone()));
                }
            }
        }
    }
    if let Some((from, phase)) = open {
        spans.push(Span {
            from,
            to: now,
            phase,
        });
    }
    let clipped = ended.is_none() && spans.last().is_some_and(|span| span.from == window_start);
    Timeline {
        spans,
        created,
        ended,
        clipped,
    }
}

/// Seconds from the start of the window.
fn days(seconds: i64, window_start: i64) -> i64 {
    seconds - window_start
}

/// Paints `kind` over `[from, to)` of a bar, splitting what lies under it.
fn overlay(segments: &mut Vec<Segment>, from: i64, to: i64, kind: Kind) {
    let mut result = Vec::new();
    for segment in segments.drain(..) {
        if segment.to <= from || segment.from >= to {
            result.push(segment);
            continue;
        }
        if segment.from < from {
            result.push(Segment {
                to: from,
                ..segment
            });
        }
        if segment.to > to {
            result.push(Segment {
                from: to,
                ..segment
            });
        }
    }
    result.push(Segment { kind, from, to });
    result.sort_by_key(|segment| segment.from);
    *segments = result;
}

fn row_for(inputs: &Inputs, change: &OpenChange, ended: bool, asked: Option<i64>) -> Row {
    let now = inputs.now;
    let window_start = now - WINDOW_DAYS * DAY;
    let events = inputs
        .history
        .get(&change.path)
        .map_or(&[][..], Vec::as_slice);
    let line = timeline(events, &change.phase, window_start, now);

    let mut segments = Vec::new();
    if let Some(created) = line.created
        && let Some(task) = &change.task
        && let Some(&queued_at) = inputs.task_created.get(task)
        && queued_at < created
    {
        segments.push(Segment {
            kind: Kind::Queued,
            from: days(queued_at.max(window_start), window_start),
            to: days(created, window_start),
        });
    }
    for span in &line.spans {
        segments.push(Segment {
            kind: Kind::of_phase(&span.phase),
            from: days(span.from, window_start),
            to: days(span.to, window_start),
        });
    }
    let last_move = line
        .ended
        .as_ref()
        .map(|(at, _)| *at)
        .or_else(|| line.spans.last().map(|span| span.from))
        .unwrap_or(now);
    let end = match (&line.ended, ended) {
        (Some((at, phase)), _) if phase == "dropped" => End::Dropped(days(*at, window_start)),
        (Some((at, _)), _) => End::Landed(days(*at, window_start)),
        (None, true) if change.phase == "dropped" => End::Dropped(days(now, window_start)),
        (None, true) => End::Landed(days(now, window_start)),
        (None, false) => End::Open,
    };

    let finding = inputs.map.as_ref().and_then(|map| {
        map.findings
            .iter()
            .find(|finding| finding.paths.contains(&change.path))
    });
    let age_text = if line.clipped {
        format!("{WINDOW_DAYS}d+")
    } else {
        age(now - last_move)
    };
    let idle_for = now - last_move;
    let in_play = matches!(end, End::Open) && change.phase != "paused";
    let idle = in_play && idle_for > IDLE_DAYS * DAY;
    let (state, tone) = match (&end, finding, asked) {
        (End::Landed(_), _, _) => (format!("landed {age_text}"), Tone::Ok),
        (End::Dropped(_), _, _) => (format!("dropped {age_text}"), Tone::Bad),
        (End::Open, Some(finding), _) => (finding.kind.clone(), Tone::Warn),
        (End::Open, None, Some(asked)) => {
            let from = days(asked.max(window_start), window_start);
            overlay(&mut segments, from, days(now, window_start), Kind::Waiting);
            (format!("waiting {}", age(now - asked)), Tone::Warn)
        }
        (End::Open, None, None) if change.phase == "paused" => {
            (format!("paused {age_text}"), Tone::Dim)
        }
        (End::Open, None, None) => {
            let tone = if line.clipped {
                Tone::Bad
            } else if idle {
                Tone::Warn
            } else {
                Tone::Running
            };
            (format!("{} {age_text}", change.phase), tone)
        }
    };
    Row {
        path: change.path.clone(),
        title: change.title.clone(),
        segments,
        end,
        state,
        tone,
        dim: finding.is_some_and(|finding| finding.kind == "landed"),
        phase: change.phase.clone(),
        idle,
        started: line.created.is_some(),
        last_move: if line.clipped {
            window_start
        } else {
            last_move
        },
    }
}

fn mount_name(mount: &str) -> String {
    mount.rsplit('/').next().unwrap_or(mount).to_owned()
}

/// The kind a bar shows at `day` days into the window.
fn kind_at(row: &Row, at: i64) -> Option<Kind> {
    row.segments
        .iter()
        .rev()
        .find(|segment| {
            segment.from <= at
                && (at < segment.to || (at >= WINDOW_SECONDS && segment.to >= WINDOW_SECONDS))
        })
        .map(|segment| segment.kind)
}

/// Builds the view from the gathered data.
#[must_use]
pub fn build(inputs: &Inputs) -> View {
    let waiting_since = |path: &str| -> Option<i64> {
        let questions = inputs.questions.as_ref()?;
        questions
            .list
            .iter()
            .filter(|question| question.names(path))
            .map(|question| question.asked_at().unwrap_or(inputs.now))
            .min()
    };
    let mut placed: Vec<(Row, String)> = Vec::new();
    for change in &inputs.open {
        let row = row_for(inputs, change, false, waiting_since(&change.path));
        placed.push((row, change.mount.clone()));
    }
    for change in &inputs.closed {
        placed.push((row_for(inputs, change, true, None), change.mount.clone()));
    }
    let groups = group(inputs.map.as_ref(), placed);
    let panel = panel_for(inputs, &groups);
    View {
        mapped: inputs.map.is_some(),
        groups: collapse(groups),
        panel,
        status: inputs.status.clone(),
        freshness: String::new(),
        selected: 0,
        scroll: 0,
        shown: 0,
        filter: String::new(),
        typing: false,
    }
}

/// Takes each group's ended changes out of its rows and counts them. Groups
/// keep their order, so each realm's heading appears once and a group with
/// only ended changes passes by as the list scrolls.
fn collapse(groups: Vec<Group>) -> Vec<Group> {
    groups
        .into_iter()
        .map(|mut group| {
            for row in &group.rows {
                match row.end {
                    End::Landed(_) => group.landed += 1,
                    End::Dropped(_) => group.dropped += 1,
                    End::Open => {}
                }
            }
            group.rows.retain(|row| matches!(row.end, End::Open));
            group
        })
        .collect()
}

fn panel_for(inputs: &Inputs, groups: &[Group]) -> Panel {
    let rows: Vec<&Row> = groups
        .iter()
        .flat_map(move |group| group.rows.iter())
        .collect();
    let in_play = rows.iter().filter(|row| row.in_play()).count();
    let plan_count = rows
        .iter()
        .filter(|row| row.in_play() && row.phase == "plan")
        .count();
    let play_groups = groups
        .iter()
        .filter(|group| group.rows.iter().any(Row::in_play))
        .count();
    let projects = inputs.map.as_ref().map_or(play_groups, |map| {
        map.projects
            .iter()
            .filter(|project| project.state != "done")
            .count()
    });
    let claimed: HashSet<&str> = inputs
        .open
        .iter()
        .filter_map(|change| change.task.as_deref())
        .collect();
    let queued = inputs.open_tasks.as_ref().map(|tasks| {
        tasks
            .iter()
            .filter(|task| !claimed.contains(task.as_str()))
            .count()
    });
    let waiting = match (&inputs.questions, &inputs.map) {
        (Some(questions), _) => Some(questions.total),
        (None, Some(map)) if !map.questions_waiting.is_empty() => {
            Some(map.questions_waiting.values().sum())
        }
        _ => None,
    };
    let days = (0..=WINDOW_DAYS)
        .map(|day| {
            let mut counts = [0; 5];
            for row in &rows {
                let slot = match kind_at(row, day * DAY) {
                    Some(Kind::Plan) => 0,
                    Some(Kind::Build) => 1,
                    Some(Kind::Review) => 2,
                    Some(Kind::Land) => 3,
                    Some(Kind::Waiting) => 4,
                    _ => continue,
                };
                counts[slot] += 1;
            }
            counts
        })
        .collect();
    let why = if in_play > inputs.limit {
        format!(
            "over because: {in_play} changes are in play, over the limit of {}. {plan_count} of them have not left plan.",
            inputs.limit
        )
    } else {
        format!(
            "within the limit: {in_play} changes are in play, limit {}.",
            inputs.limit
        )
    };
    Panel {
        in_play,
        limit: inputs.limit,
        projects,
        queued,
        landed: rows
            .iter()
            .filter(|row| matches!(row.end, End::Landed(_)))
            .count(),
        started: rows.iter().filter(|row| row.started).count(),
        idle: rows.iter().filter(|row| row.idle).count(),
        waiting,
        days,
        why,
    }
}

fn group(map: Option<&WorkMap>, placed: Vec<(Row, String)>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut rest: Vec<(Row, String)>;
    if let Some(map) = map {
        let mut taken: HashSet<String> = HashSet::new();
        let mut by_path: HashMap<String, (Row, String)> = placed
            .into_iter()
            .map(|entry| (entry.0.path.clone(), entry))
            .collect();
        for realm in &map.realms {
            for project in map
                .projects
                .iter()
                .filter(|project| project.realm == realm.id)
            {
                let mut rows: Vec<Row> = Vec::new();
                for path in &project.changes {
                    if taken.insert(path.clone())
                        && let Some((row, _)) = by_path.remove(path)
                    {
                        rows.push(row);
                    }
                }
                if !rows.is_empty() {
                    rows.sort_by_key(|row| (!row.in_play(), row.last_move));
                    groups.push(Group {
                        realm: Some(realm.title.clone()),
                        project: project.id.clone(),
                        rows,
                        landed: 0,
                        dropped: 0,
                    });
                }
            }
        }
        rest = by_path.into_values().collect();
    } else {
        rest = placed;
    }
    rest.sort_by(|a, b| {
        (&a.1, !a.0.in_play(), a.0.last_move, &a.0.path).cmp(&(
            &b.1,
            !b.0.in_play(),
            b.0.last_move,
            &b.0.path,
        ))
    });
    for (row, mount) in rest {
        let name = mount_name(&mount);
        let realm = map.map(|_| "unmapped".to_owned());
        match groups
            .iter_mut()
            .find(|group| group.project == name && group.realm == realm)
        {
            Some(existing) => existing.rows.push(row),
            None => groups.push(Group {
                realm,
                project: name,
                rows: vec![row],
                landed: 0,
                dropped: 0,
            }),
        }
    }
    groups
}

mod scroll;
mod search;
pub use scroll::{BodyLine, Window};

#[cfg(test)]
mod tests;
