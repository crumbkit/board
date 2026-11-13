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
fn landed_and_dropped_changes_keep_their_row_with_an_end_mark() {
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
    assert_eq!(row(&view, "/changes/l").end, End::Landed(11 * DAY));
    assert_eq!(row(&view, "/changes/l").state, "landed 3d");
    assert!(matches!(row(&view, "/changes/d").end, End::Dropped(_)));
    assert_eq!(row(&view, "/changes/d").state, "dropped 2d");
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
    assert_eq!(ended_paths(&open, &history), ["/changes/l"]);
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
        "over because: 3 changes are in play across 2 projects, over the limit of 2. 2 of them have not left plan."
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
