use board_model::{Code, LogicalPath, Moved};
use board_scenario::{Item, Scenario};
use board_tree::Status;

use crate::Board;
use crate::tests::{board, open};

fn moving(scenario: &Scenario) -> Board {
    let mut board = open(scenario);
    let path = |text: &str| text.parse::<LogicalPath>().unwrap();
    board.moving(move || vec![Moved::new(path("/controls"), path("/rules")).unwrap()]);
    board
}

/// A board holding `/rules/p`, and the library it mounts at `/lib`, which
/// holds `/rules/q`.
fn mounting() -> (Scenario, Scenario) {
    let library = Scenario::new();
    let commit = library.commit(&[("rules/q.json", r#"{"$type": "lr/project/v1"}"#)]);
    let scenario = board();
    let mount = format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
        library.path()
    );
    scenario
        .write("lib.json", Item::File(&mount))
        .write("rules/p.json", Item::File(r#"{"$type": "lr/project/v1"}"#));
    (scenario, library)
}

#[test]
fn a_glob_under_a_moved_prefix_swaps_at_the_root_and_at_a_mounted_root_and_nowhere_else() {
    let (scenario, _library) = mounting();
    let board = moving(&scenario);
    let swapped = |pattern: &str| board.moved_glob(pattern);
    assert_eq!(
        swapped("/controls/board/*"),
        Some("/rules/board/*".to_owned())
    );
    assert_eq!(swapped("/rules/*"), Some("/controls/*".to_owned()));
    assert_eq!(
        swapped("/lib/controls/*.json"),
        Some("/lib/rules/*.json".to_owned())
    );
    assert_eq!(swapped("/projects/controls/*"), None);
    assert_eq!(swapped("/control*/x"), None);
    assert_eq!(swapped("controls/*"), None);
    assert_eq!(open(&scenario).moved_glob("/controls/*"), None);
}

#[test]
fn a_reference_under_a_moved_prefix_checks_as_resolved_and_without_the_prefix_dangles() {
    let (scenario, _library) = mounting();
    for project in ["/controls/p", "/lib/controls/q"] {
        let task = format!(r#"{{"$type": "lr/task/v1", "title": "t", "project": "{project}"}}"#);
        scenario.write("tasks/t.json", Item::File(&task));
        let status = |board: &Board| {
            let report = board.check(None, None).unwrap();
            let entry = report
                .entries
                .into_iter()
                .find(|entry| entry.path == "tasks/t.json");
            entry.unwrap().status
        };
        assert_eq!(status(&moving(&scenario)), Status::Ok, "{project}");
        assert_eq!(
            status(&open(&scenario)),
            Status::Degraded(Code::RefDangling),
            "{project}"
        );
    }
}
