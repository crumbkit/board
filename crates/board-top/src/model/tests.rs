//! Tests for the model.

use super::*;
use crate::parse::{Finding, Project, Question, Realm};

const NOW: i64 = 1_794_564_745;

fn change(path: &str, phase: &str, mount: &str, task: Option<&str>) -> OpenChange {
    OpenChange {
        path: path.to_owned(),
        title: path.rsplit('/').next().unwrap().to_owned(),
        mount: mount.to_owned(),
        phase: phase.to_owned(),
        task: task.map(str::to_owned),
    }
}

fn event(days_ago: i64, from: Option<&str>, to: &str) -> PhaseEvent {
    PhaseEvent {
        at: NOW - days_ago * DAY,
        from: from.map(str::to_owned),
        to: to.to_owned(),
    }
}

fn inputs() -> Inputs {
    Inputs {
        now: NOW,
        limit: 2,
        ..Inputs::default()
    }
}

fn row<'a>(view: &'a View, path: &str) -> &'a Row {
    view.rows().find(|row| row.path == path).unwrap()
}

#[test]
fn a_change_gets_queued_time_then_one_segment_per_phase() {
    let mut input = inputs();
    input.open = vec![change(
        "/changes/a",
        "build",
        "/repos/board",
        Some("/tasks/t.md"),
    )];
    input.history.insert(
        "/changes/a".to_owned(),
        vec![event(8, None, "plan"), event(5, Some("plan"), "build")],
    );
    input
        .task_created
        .insert("/tasks/t.md".to_owned(), NOW - 10 * DAY);
    let view = build(&input);
    let kinds: Vec<(Kind, i64, i64)> = row(&view, "/changes/a")
        .segments
        .iter()
        .map(|s| (s.kind, s.from / DAY, s.to / DAY))
        .collect();
    assert_eq!(
        kinds,
        [
            (Kind::Queued, 4, 6),
            (Kind::Plan, 6, 9),
            (Kind::Build, 9, 14)
        ]
    );
    assert_eq!(row(&view, "/changes/a").state, "build 5d");
}

#[test]
fn a_change_with_no_history_shows_its_phase_across_the_window() {
    let mut input = inputs();
    input.open = vec![change("/changes/a", "plan", "/repos/board", None)];
    let view = build(&input);
    let found = row(&view, "/changes/a");
    assert_eq!(found.segments.len(), 1);
    assert_eq!(found.state, "plan 14d+");
    assert_eq!(found.tone, Tone::Bad);
}

#[test]
fn a_move_inside_the_window_keeps_the_phase_before_it() {
    let mut input = inputs();
    input.open = vec![change("/changes/a", "build", "/repos/board", None)];
    input.history.insert(
        "/changes/a".to_owned(),
        vec![event(4, Some("plan"), "build")],
    );
    let view = build(&input);
    let found = row(&view, "/changes/a");
    assert_eq!(found.segments[0].kind, Kind::Plan);
    assert_eq!(found.segments[0].from, 0);
    assert_eq!(found.state, "build 4d");
}

#[test]
fn landed_and_dropped_changes_collapse_into_counts_but_stay_in_the_panel() {
    let mut input = inputs();
    input.closed = vec![
        change("/changes/l", "done", "/repos/board", None),
        change("/changes/d", "dropped", "/repos/board", None),
    ];
    input.history.insert(
        "/changes/l".to_owned(),
        vec![event(9, None, "plan"), event(3, Some("plan"), "done")],
    );
    input.history.insert(
        "/changes/d".to_owned(),
        vec![event(9, None, "plan"), event(2, Some("plan"), "dropped")],
    );
    let view = build(&input);
    assert_eq!(view.rows().count(), 0);
    assert_eq!(view.groups.len(), 1);
    assert_eq!((view.groups[0].landed, view.groups[0].dropped), (1, 1));
    assert_eq!(view.panel.landed, 1);
    assert_eq!(view.panel.in_play, 0);
    assert_eq!(view.panel.started, 2);
}

#[test]
fn ended_paths_are_history_entries_that_are_not_open() {
    let open = vec![change("/changes/a", "build", "/repos/board", None)];
    let history = HashMap::from([
        (
            "/changes/a".to_owned(),
            vec![event(1, Some("plan"), "build")],
        ),
        (
            "/changes/l".to_owned(),
            vec![event(1, Some("land"), "done")],
        ),
        (
            "/changes/m".to_owned(),
            vec![event(1, Some("plan"), "build")],
        ),
    ]);
    assert_eq!(ended_paths(&open, &history, NOW - 14 * DAY), ["/changes/l"]);
}

#[test]
fn an_open_question_turns_the_wait_into_a_waiting_segment() {
    let mut input = inputs();
    input.open = vec![change("/changes/a", "build", "/repos/board", None)];
    input.questions = Some(Questions {
        list: vec![Question {
            path: "/q/1".to_owned(),
            asked: "2026-11-11T10:12:25Z".to_owned(),
            blocks: Some("/changes/a".to_owned()),
            ..Question::default()
        }],
        total: 6,
    });
    let view = build(&input);
    let found = row(&view, "/changes/a");
    assert_eq!(found.segments.last().unwrap().kind, Kind::Waiting);
    assert_eq!(found.state, "waiting 2d");
    assert_eq!(view.panel.waiting, Some(6));
}

fn mapped_input() -> Inputs {
    let mut input = inputs();
    input.open = vec![
        change("/changes/a", "build", "/repos/board", None),
        change("/changes/b", "plan", "/repos/board", None),
        change("/changes/c", "plan", "/repos/dash", None),
    ];
    input.map = Some(WorkMap {
        realms: vec![Realm {
            id: "board".to_owned(),
            title: "Board".to_owned(),
        }],
        projects: vec![Project {
            id: "rename".to_owned(),
            realm: "board".to_owned(),
            title: "Rename".to_owned(),
            state: "active".to_owned(),
            changes: vec!["/changes/b".to_owned(), "/changes/a".to_owned()],
        }],
        findings: vec![
            Finding {
                kind: "stale".to_owned(),
                paths: vec!["/changes/a".to_owned()],
            },
            Finding {
                kind: "landed".to_owned(),
                paths: vec!["/changes/b".to_owned()],
            },
        ],
        questions_waiting: [("board".to_owned(), 4)].into(),
    });
    input
}

#[test]
fn rows_group_by_realm_and_project_from_the_map() {
    let view = build(&mapped_input());
    assert!(view.mapped);
    assert_eq!(view.groups.len(), 2);
    assert_eq!(view.groups[0].realm.as_deref(), Some("Board"));
    assert_eq!(view.groups[0].project, "rename");
    assert_eq!(view.groups[0].rows.len(), 2);
    assert_eq!(view.groups[1].realm.as_deref(), Some("unmapped"));
    assert_eq!(view.groups[1].project, "dash");
    assert_eq!(view.panel.waiting, Some(4));
    assert_eq!(view.panel.projects, 1);
}

#[test]
fn a_finding_sets_the_state_and_a_landed_finding_dims_the_row() {
    let view = build(&mapped_input());
    assert_eq!(row(&view, "/changes/a").state, "stale");
    assert!(!row(&view, "/changes/a").dim);
    assert_eq!(row(&view, "/changes/b").state, "landed");
    assert!(row(&view, "/changes/b").dim);
    assert!(!row(&view, "/changes/c").dim);
}

#[test]
fn open_rows_and_groups_come_before_ended_ones() {
    let mut input = mapped_input();
    input.closed = vec![change("/changes/z", "done", "/repos/zed", None)];
    input.history.insert(
        "/changes/z".to_owned(),
        vec![event(3, Some("land"), "done")],
    );
    let view = build(&input);
    let order: Vec<&str> = view.groups.iter().map(|g| g.project.as_str()).collect();
    assert_eq!(order, ["rename", "dash", "zed"]);
}

#[test]
fn without_a_map_rows_group_by_mount() {
    let mut input = mapped_input();
    input.map = None;
    let view = build(&input);
    assert!(!view.mapped);
    let names: Vec<&str> = view.groups.iter().map(|g| g.project.as_str()).collect();
    assert_eq!(names, ["board", "dash"]);
    assert!(view.groups.iter().all(|group| group.realm.is_none()));
    assert_eq!(row(&view, "/changes/a").state, "build 14d+");
    assert_eq!(view.panel.projects, 2);
}

#[test]
fn the_panel_says_why_the_board_is_over_the_limit() {
    let mut input = mapped_input();
    input.map = None;
    let view = build(&input);
    assert!(view.panel.over());
    assert_eq!(
        view.panel.why,
        "over because: 3 changes are in play, over the limit of 2. 2 of them have not left plan."
    );
    input.limit = 3;
    let view = build(&input);
    assert!(!view.panel.over());
    assert_eq!(
        view.panel.why,
        "within the limit: 3 changes are in play, limit 3."
    );
}

#[test]
fn the_panel_counts_idle_changes_queued_tasks_and_daily_phases() {
    let mut input = inputs();
    input.open = vec![
        change(
            "/changes/a",
            "build",
            "/repos/board",
            Some("/tasks/claimed.md"),
        ),
        change("/changes/b", "plan", "/repos/board", None),
    ];
    input.history.insert(
        "/changes/a".to_owned(),
        vec![event(6, None, "plan"), event(1, Some("plan"), "build")],
    );
    input.open_tasks = Some(vec![
        "/tasks/claimed.md".to_owned(),
        "/tasks/free.md".to_owned(),
    ]);
    let view = build(&input);
    assert_eq!(view.panel.idle, 1);
    assert_eq!(view.panel.queued, Some(1));
    assert_eq!(view.panel.days.len(), 15);
    assert_eq!(view.panel.days[0], [1, 0, 0, 0, 0]);
    assert_eq!(view.panel.days[14], [1, 1, 0, 0, 0]);
    assert_eq!(view.panel.waiting, None);
}

#[test]
fn every_row_carries_its_record_path_and_one_is_selected() {
    let view = build(&mapped_input());
    let paths: Vec<&str> = view.rows().map(|row| row.path.as_str()).collect();
    assert_eq!(paths.len(), 3);
    assert_eq!(view.selected_path(), Some(paths[0]));
}

#[test]
fn a_change_that_ended_before_the_window_is_not_fetched() {
    let history = HashMap::from([(
        "/changes/old".to_owned(),
        vec![event(20, Some("land"), "done")],
    )]);
    assert_eq!(
        ended_paths(&[], &history, NOW - 14 * DAY),
        Vec::<String>::new()
    );
}

#[test]
fn collapsed_lines_come_after_every_row() {
    let mut input = mapped_input();
    input.closed = vec![change("/changes/z", "done", "/repos/zed", None)];
    input.history.insert(
        "/changes/z".to_owned(),
        vec![event(3, Some("land"), "done")],
    );
    let view = build(&input);
    let last = view.groups.last().unwrap();
    assert!(last.rows.is_empty() && last.landed == 1);
    assert!(
        view.groups[..view.groups.len() - 1]
            .iter()
            .all(|g| !g.rows.is_empty())
    );
}

#[test]
fn the_highlight_follows_its_record_when_rows_move() {
    let mut input = inputs();
    input.open = vec![
        change("/changes/a", "build", "/repos/board", None),
        change("/changes/b", "build", "/repos/board", None),
    ];
    let mut old = build(&input);
    let at = old.rows().position(|row| row.path == "/changes/b").unwrap();
    old.selected = at;

    input
        .open
        .insert(0, change("/changes/new", "plan", "/repos/board", None));
    let mut fresh = build(&input);
    fresh.keep_selection(&old);
    assert_eq!(fresh.selected_path(), Some("/changes/b"));

    input.open.retain(|c| c.path != "/changes/b");
    let mut gone = build(&input);
    gone.keep_selection(&old);
    assert!(gone.selected < gone.rows().count());
}

fn long_view(count: usize) -> View {
    let mut input = inputs();
    input.open = (0..count)
        .map(|n| change(&format!("/changes/c{n:02}"), "build", "/repos/board", None))
        .collect();
    build(&input)
}

#[test]
fn the_highlight_stops_at_both_ends() {
    let mut view = long_view(5);
    view.move_by(-1);
    assert_eq!(view.selected, 0);
    view.move_by(100);
    assert_eq!(view.selected, 4);
    view.home();
    assert_eq!(view.selected, 0);
    view.end();
    assert_eq!(view.selected, 4);
}

#[test]
fn a_page_moves_by_the_height_less_one_and_the_window_follows() {
    let mut view = long_view(40);
    view.page(10, true);
    assert_eq!(view.selected, 9);
    let window = view.window(10);
    assert!(window.start <= 9 && 9 < window.end, "{window:?}");
    assert_eq!(window.rows.map(|(_, last)| last >= 10), Some(true));
    view.page(10, false);
    assert_eq!(view.selected, 0);
    assert_eq!(view.window(10).start, 0);
}

#[test]
fn a_list_that_fits_has_no_position_line_and_one_that_does_not_reserves_one() {
    let view = long_view(5);
    assert!(view.window(10).whole);
    let mut view = long_view(40);
    let window = view.window(10);
    assert!(!window.whole);
    assert_eq!(window.end - window.start, 9);
    assert_eq!(window.rows, Some((1, 9)));
    assert_eq!(window.total, 40);
    view.end();
    assert_eq!(view.window(10).rows, Some((32, 40)));
}

#[test]
fn the_selection_stays_visible_after_a_refresh_that_shrinks_the_list() {
    let mut view = long_view(40);
    view.end();
    view.reveal(10);
    assert!(view.scroll > 0);
    let mut fresh = long_view(6);
    fresh.keep_selection(&view);
    assert!(fresh.selected < 6);
    let window = fresh.window(10);
    assert!(window.whole && window.start == 0, "{window:?}");
}

#[test]
fn scrolling_keeps_a_realm_heading_with_its_first_row() {
    let mut input = mapped_input();
    input.map.as_mut().unwrap();
    let mut view = build(&input);
    view.home();
    let lines = view.body();
    assert!(matches!(lines[0], BodyLine::Realm(_)));
    assert_eq!(view.window(3).start, 0);
}

fn view_with_trailing_ended_lines() -> View {
    let mut input = mapped_input();
    input.closed = vec![
        change("/changes/z", "done", "/repos/zed", None),
        change("/changes/y", "done", "/repos/yak", None),
    ];
    for path in ["/changes/z", "/changes/y"] {
        input
            .history
            .insert(path.to_owned(), vec![event(3, Some("land"), "done")]);
    }
    build(&input)
}

#[test]
fn end_scrolls_to_the_last_body_line_even_when_it_is_an_ended_line() {
    let mut view = view_with_trailing_ended_lines();
    let body = view.body();
    assert!(matches!(body.last(), Some(BodyLine::Ended(_))));
    view.end();
    view.reveal(3);
    assert_eq!(view.window(3).end, body.len());
    view.move_by(-1);
    view.reveal(3);
    assert!(view.window(3).start < body.len() - 2);

    let mut again = view_with_trailing_ended_lines();
    again.end();
    again.reveal(3);
    again.home();
    again.move_by(1);
    for _ in 0..20 {
        again.move_by(1);
    }
    assert_eq!(again.scroll, usize::MAX);
    assert_eq!(again.window(3).end, body.len());
}

#[test]
fn no_realm_heading_appears_twice() {
    let view = view_with_trailing_ended_lines();
    let mut headings: Vec<String> = view
        .body()
        .into_iter()
        .filter_map(|line| match line {
            BodyLine::Realm(name) => Some(name),
            _ => None,
        })
        .collect();
    let count = headings.len();
    headings.sort();
    headings.dedup();
    assert_eq!(headings.len(), count, "{headings:?}");
}

#[test]
fn a_refresh_that_adds_rows_after_the_selection_drops_the_bottom_pin() {
    let mut old = long_view(5);
    old.end();
    old.reveal(4);
    assert_eq!(old.scroll, usize::MAX);
    let mut fresh = long_view(5);
    fresh.keep_selection(&old);
    assert_eq!(fresh.scroll, usize::MAX);

    let mut more = long_view(9);
    more.keep_selection(&old);
    assert_eq!(more.scroll, old.shown);
    assert!(more.selected < 8);
}
