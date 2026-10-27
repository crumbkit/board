//! Where a removal lands (docs/architecture/writes/removing.md).

use super::*;
use crate::Removes;

/// The file a removal of `text` names and what it removes, or the
/// code that refuses it.
fn removal(scenario: &Scenario, text: &str) -> Result<(String, Removes), Code> {
    let repositories = scenario.repositories();
    let mut snapshot = repositories.snapshot().unwrap();
    snapshot
        .removal_target(&path(text))
        .map(|(target, removes)| (target.path, removes))
        .map_err(|error| error.code())
}

#[test]
fn a_removal_names_a_record_or_a_file_and_a_path_that_names_nothing_is_refused() {
    let scenario = Scenario::new();
    scenario
        .write("tasks/a.json", Item::File("{}"))
        .write("notes/n.md", Item::File("# n\n"));
    assert_eq!(
        removal(&scenario, "/tasks/a"),
        Ok(("tasks/a.json".into(), Removes::File))
    );
    assert_eq!(
        removal(&scenario, "/notes/n.md"),
        Ok(("notes/n.md".into(), Removes::File))
    );
    assert_eq!(removal(&scenario, "/tasks/b"), Err(Code::PathNotFound));
}

#[test]
fn a_removal_of_a_directory_is_refused_and_of_a_file_in_it_is_admitted() {
    let scenario = Scenario::new();
    scenario.write("tasks/a.json", Item::File("{}"));
    let repositories = scenario.repositories();
    let refused = repositories
        .snapshot()
        .unwrap()
        .removal_target(&path("/tasks"))
        .unwrap_err();
    assert_eq!(refused.code(), Code::RmDirectory);
    assert_eq!(
        refused.to_string(),
        "/tasks cannot be removed: it is a directory"
    );
    assert_eq!(
        removal(&scenario, "/tasks/a"),
        Ok(("tasks/a.json".into(), Removes::File))
    );
}

#[test]
fn a_removal_of_a_link_names_the_link_and_through_it_names_the_file_it_leads_to() {
    let scenario = Scenario::new();
    scenario
        .write("tasks/a.json", Item::File("{}"))
        .write(
            "l.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/tasks"}"#),
        )
        .write(
            "gone.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/nothing"}"#),
        );
    assert_eq!(
        removal(&scenario, "/l"),
        Ok(("l.json".into(), Removes::Pointer))
    );
    assert_eq!(
        removal(&scenario, "/l/a"),
        Ok(("tasks/a.json".into(), Removes::File))
    );
    assert_eq!(
        removal(&scenario, "/gone"),
        Ok(("gone.json".into(), Removes::Pointer))
    );
}

#[test]
fn a_removal_through_a_mount_is_refused_and_of_the_mount_itself_is_admitted() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "{}")]);
    let outer = Scenario::new();
    let mount = format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{pinned}"}}"#,
        inner.path()
    );
    outer.write("m.json", Item::File(&mount));
    let repositories = outer.repositories();
    let refused = repositories
        .snapshot()
        .unwrap()
        .removal_target(&path("/m/x"))
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutThroughMount);
    assert_eq!(
        refused.to_string(),
        "/m/x cannot be removed: it is in the board mounted at /m"
    );
    assert_eq!(refused.fields(), board_model::Fields::at(&path("/m")));
    assert_eq!(
        removal(&outer, "/m"),
        Ok(("m.json".into(), Removes::Pointer))
    );
}
