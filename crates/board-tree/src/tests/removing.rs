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

/// Each file a recursive removal of `text` names, with its logical path,
/// its file and what it removes, or the code that refuses them.
fn removals(scenario: &Scenario, text: &str) -> Result<Vec<(String, String, Removes)>, Code> {
    let repositories = scenario.repositories();
    let mut snapshot = repositories.snapshot().unwrap();
    let found = snapshot.removal_targets(&path(text));
    found
        .map(|found| {
            found
                .into_iter()
                .map(|(logical, target, removes)| (logical.to_string(), target.path, removes))
                .collect()
        })
        .map_err(|error| error.code())
}

fn named(logical: &str, file: &str, removes: Removes) -> (String, String, Removes) {
    (logical.into(), file.into(), removes)
}

#[test]
fn a_recursive_removal_names_every_file_beneath_and_a_pointer_itself_and_a_file_alone() {
    let scenario = Scenario::new();
    scenario
        .write("tasks/a.json", Item::File("{}"))
        .write("tasks/sub/b.md", Item::File("# b\n"))
        .write(
            "tasks/l.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/notes"}"#),
        )
        .write("notes/n.md", Item::File("# n\n"));
    assert_eq!(
        removals(&scenario, "/tasks"),
        Ok(vec![
            named("/tasks/a", "tasks/a.json", Removes::File),
            named("/tasks/l", "tasks/l.json", Removes::Pointer),
            named("/tasks/sub/b.md", "tasks/sub/b.md", Removes::File),
        ])
    );
    assert_eq!(
        removals(&scenario, "/tasks/a"),
        Ok(vec![named("/tasks/a", "tasks/a.json", Removes::File)])
    );
    assert_eq!(removals(&scenario, "/gone"), Err(Code::PathNotFound));
}

#[test]
fn a_recursive_removal_over_a_symlink_is_refused_and_beside_it_is_admitted() {
    let scenario = Scenario::new();
    scenario
        .write("tasks/a.json", Item::File("{}"))
        .write("tasks/s.json", Item::Symlink("a.json"))
        .write("notes/n.md", Item::File("# n\n"));
    assert_eq!(removals(&scenario, "/tasks"), Err(Code::PutSymlink));
    assert_eq!(
        removals(&scenario, "/notes"),
        Ok(vec![named("/notes/n.md", "notes/n.md", Removes::File)])
    );
}

#[test]
fn a_recursive_removal_over_a_name_with_no_logical_path_is_refused_and_beside_it_is_admitted() {
    let scenario = Scenario::new();
    scenario
        .write("tasks/a\tb.md", Item::File("x"))
        .write("notes/n.md", Item::File("# n\n"));
    let repositories = scenario.repositories();
    let refused = repositories
        .snapshot()
        .unwrap()
        .removal_targets(&path("/tasks"))
        .unwrap_err();
    assert_eq!(refused.code(), Code::PathMalformed);
    assert_eq!(
        refused.to_string(),
        "/tasks cannot be removed: \"tasks/a\\tb.md\" beneath it has no logical path"
    );
    assert_eq!(
        removals(&scenario, "/notes"),
        Ok(vec![named("/notes/n.md", "notes/n.md", Removes::File)])
    );
}

#[test]
fn a_recursive_removal_through_a_mount_is_refused_and_of_the_mount_itself_is_admitted() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("d/x.json", "{}")]);
    let outer = Scenario::new();
    let mount = format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{pinned}"}}"#,
        inner.path()
    );
    outer.write("m.json", Item::File(&mount));
    assert_eq!(removals(&outer, "/m/d"), Err(Code::PutThroughMount));
    assert_eq!(
        removals(&outer, "/m"),
        Ok(vec![named("/m", "m.json", Removes::Pointer)])
    );
}

#[test]
fn removing_a_file_removes_the_directories_it_leaves_empty_and_keeps_one_still_holding_a_file() {
    let scenario = Scenario::new();
    scenario
        .write("notes/deep/a.md", Item::File("a"))
        .write("tasks/deep/b.md", Item::File("b"))
        .write("tasks/c.md", Item::File("c"));
    let repository = scenario.open();
    let lock = repository.lock(crate::LOCK_WAIT).unwrap();
    repository.remove_file(&lock, "notes/deep/a.md").unwrap();
    repository.remove_file(&lock, "tasks/deep/b.md").unwrap();
    assert!(!scenario.workdir().join("notes").exists());
    assert!(!scenario.workdir().join("tasks/deep").exists());
    assert!(scenario.workdir().join("tasks/c.md").exists());
}
