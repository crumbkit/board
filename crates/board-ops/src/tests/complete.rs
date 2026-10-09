//! Completing a typed logical path one directory level at a time.

use board_scenario::{Item, Scenario};

use super::{PROJECT_SCHEMA, board, open, task};

/// A board with tasks, a directory of notes and a link to /projects.
fn tasks() -> Scenario {
    let scenario = board();
    scenario
        .write("tasks/t1.json", Item::File(&task("")))
        .write("tasks/t10.json", Item::File(&task("")))
        .write("tasks/t2.json", Item::File(&task("")))
        .write("tasks/done/t3.json", Item::File(&task("")))
        .write("notes/readme.md", Item::File("# r"))
        .write(
            "l.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects"}"#),
        );
    scenario
}

fn complete(scenario: &Scenario, typed: &str) -> Vec<String> {
    open(scenario).complete(typed)
}

#[test]
fn a_root_prefix_offers_the_matching_directories_with_a_trailing_slash() {
    let scenario = tasks();
    assert_eq!(complete(&scenario, "/ta"), ["/tasks/"]);
    assert_eq!(
        complete(&scenario, "/"),
        ["/l", "/notes/", "/projects/", "/tasks/"]
    );
    assert_eq!(complete(&scenario, ""), complete(&scenario, "/"));
}

#[test]
fn a_nested_prefix_offers_records_without_json_and_directories_with_a_slash() {
    let scenario = tasks();
    assert_eq!(
        complete(&scenario, "/tasks/t1"),
        ["/tasks/t1", "/tasks/t10"]
    );
    assert_eq!(
        complete(&scenario, "/tasks/"),
        ["/tasks/done/", "/tasks/t1", "/tasks/t10", "/tasks/t2"]
    );
    assert_eq!(complete(&scenario, "/tasks/done/"), ["/tasks/done/t3"]);
}

#[test]
fn a_file_that_is_not_json_keeps_its_whole_name() {
    assert_eq!(complete(&tasks(), "/notes/r"), ["/notes/readme.md"]);
}

#[test]
fn a_record_beside_its_directory_is_offered_both_ways() {
    let scenario = tasks();
    scenario.write("tasks/done.json", Item::File(&task("")));
    assert_eq!(
        complete(&scenario, "/tasks/d"),
        ["/tasks/done", "/tasks/done/"]
    );
}

#[test]
fn a_hidden_name_is_offered_only_once_a_dot_is_typed() {
    let scenario = tasks();
    assert_eq!(complete(&scenario, "/."), ["/.board/"]);
    assert!(!complete(&scenario, "/").contains(&"/.board/".to_owned()));
}

#[test]
fn a_prefix_through_a_link_lists_the_target_under_the_name_typed() {
    assert_eq!(complete(&tasks(), "/l/"), ["/l/p"]);
}

#[test]
fn a_prefix_through_a_mount_lists_the_mounted_commit() {
    let mounted = Scenario::new();
    let commit = mounted.commit(&[
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("work/p.json", r#"{"$type": "lr/project/v1"}"#),
        ("work/sub/x.md", ""),
    ]);
    let scenario = tasks();
    scenario.write(
        "shared.json",
        Item::File(&format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            mounted.path()
        )),
    );
    assert_eq!(
        complete(&scenario, "/shared/work/"),
        ["/shared/work/p", "/shared/work/sub/"]
    );
    assert_eq!(complete(&scenario, "/shared/w"), ["/shared/work/"]);
}

#[test]
fn a_prefix_that_matches_nothing_offers_nothing() {
    let scenario = tasks();
    assert!(complete(&scenario, "/tasks/z").is_empty());
    assert!(complete(&scenario, "/missing/t").is_empty());
}

#[test]
fn a_prefix_beneath_a_file_offers_nothing_while_one_beneath_its_directory_does() {
    let scenario = tasks();
    assert!(complete(&scenario, "/tasks/t1/").is_empty());
    assert_eq!(complete(&scenario, "/tasks/done/"), ["/tasks/done/t3"]);
}

#[test]
fn a_prefix_without_a_leading_slash_offers_nothing_while_the_same_with_one_does() {
    let scenario = tasks();
    assert!(complete(&scenario, "tasks/t2").is_empty());
    assert_eq!(complete(&scenario, "/tasks/t2"), ["/tasks/t2"]);
}

#[test]
fn completing_writes_nothing() {
    let scenario = tasks();
    let (head, index) = (scenario.head().map(|head| head.id), scenario.index());
    complete(&scenario, "/tasks/");
    assert_eq!(scenario.head().map(|head| head.id), head);
    assert_eq!(scenario.index(), index);
}
