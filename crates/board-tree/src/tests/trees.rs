//! Trees as the index keeps them (docs/architecture.md, The index).

use super::*;
use crate::{TreeRow, TreeRows};

fn trees(scenario: &Scenario, known: &[ObjectId]) -> Vec<TreeRows> {
    let repository = scenario.open();
    let root = repository.head_tree().unwrap().unwrap();
    repository
        .trees(root, &mut |id| known.contains(&id))
        .unwrap()
}

fn row<'a>(trees: &'a [TreeRows], name: &str) -> &'a TreeRow {
    trees
        .iter()
        .flat_map(|tree| &tree.rows)
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no row {name}"))
}

#[test]
fn each_tree_is_listed_once_with_its_entries_addressed() {
    let scenario = Scenario::new();
    scenario.commit(&[
        ("a.json", "{}"),
        ("Case.md", ""),
        ("case.md", ""),
        ("work/b.json", "{}"),
        ("work/b/c.md", ""),
    ]);
    let trees = trees(&scenario, &[]);
    assert_eq!(trees.len(), 3, "the root, work and work/b");
    let a = row(&trees, "a.json");
    assert_eq!(
        (a.mode, a.object, a.segment.as_deref(), a.code),
        (0o100_644, scenario.blob("{}"), Some("a"), None)
    );
    assert_eq!(row(&trees, "case.md").code, Some(Code::PathCaseCollision));
    let work = row(&trees, "work");
    assert_eq!(
        (work.mode, work.segment.as_deref()),
        (TreeRow::TREE_MODE, Some("work"))
    );
    assert_eq!(
        row(&trees, "b.json").code,
        None,
        "a record beside its children is no problem"
    );
}

#[test]
fn a_pointer_beside_a_directory_is_shadowed_in_its_tree() {
    let scenario = Scenario::new();
    scenario.commit(&[
        ("x.json", r#"{"$type": "board/mount/v1", "path": "/y"}"#),
        ("x/c.md", ""),
    ]);
    assert_eq!(
        row(&trees(&scenario, &[]), "x.json").code,
        Some(Code::PathShadowed)
    );
}

#[test]
fn a_known_tree_is_skipped_with_everything_beneath_it() {
    let scenario = Scenario::new();
    scenario.commit(&[("a.md", ""), ("work/b/c.md", "")]);
    let work = row(&trees(&scenario, &[]), "work").object;
    let trees = trees(&scenario, &[work]);
    assert_eq!(trees.len(), 1);
    assert_eq!(trees[0].rows.len(), 2, "the root still names work");
}

#[test]
fn a_tree_reached_by_two_paths_is_listed_once() {
    let scenario = Scenario::new();
    scenario.commit(&[("x/f.md", "same"), ("y/f.md", "same")]);
    assert_eq!(trees(&scenario, &[]).len(), 2);
}

#[test]
fn a_tree_that_cannot_be_read_is_left_out() {
    let scenario = Scenario::new();
    scenario.commit_items(&[(b"lost", Item::MissingTree), (b"a.md", Item::File(""))]);
    let trees = trees(&scenario, &[]);
    assert_eq!(trees.len(), 1);
    assert_eq!(trees[0].rows.len(), 2);
}

#[test]
fn head_names_no_tree_before_the_first_commit_and_its_commits_tree_after() {
    let scenario = Scenario::new();
    assert_eq!(scenario.open().head_tree().unwrap(), None);
    let commit = scenario.commit(&[("a.md", "")]);
    let attached = scenario.open().head_tree().unwrap();
    assert!(attached.is_some());
    scenario.write(".git/HEAD", Item::File(&format!("{commit}\n")));
    assert_eq!(
        scenario.open().head_tree().unwrap(),
        attached,
        "a detached HEAD names the same tree"
    );
}
