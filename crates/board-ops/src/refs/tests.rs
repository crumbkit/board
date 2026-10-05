//! References from and to an object.

use std::path::Path;

use board_model::{Code, Fields, LogicalPath};
use board_scenario::{Item, Scenario};
use board_tree::Status;
use proptest::prelude::*;

use crate::RefsReport;
use crate::tests::generated::{Generated, generated};
use crate::tests::{board, open, task};

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

fn refs(scenario: &Scenario, text: &str) -> Result<RefsReport, Code> {
    open(scenario)
        .refs(&path(text), None)
        .map_err(|error| error.code())
}

/// The board, with a link to the project and three tasks that name it: by
/// its path, through the link, and both.
fn linked() -> Scenario {
    let scenario = board();
    scenario
        .write(
            "links/l.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#),
        )
        .write(
            "tasks/a.json",
            Item::File(&task(r#", "project": "/projects/p""#)),
        )
        .write("tasks/b.json", Item::File(&task(r#", "see": "/links/l""#)))
        .write(
            "tasks/c.json",
            Item::File(&task(r#", "project": "/projects/p", "see": "/links/l""#)),
        );
    scenario
}

#[test]
fn refs_lists_each_reference_a_record_makes_with_its_status() {
    let scenario = linked();
    scenario.write(
        "tasks/d.json",
        Item::File(&task(r#", "project": "/tasks/a", "see": "/nowhere""#)),
    );
    let report = refs(&scenario, "/tasks/d").unwrap();
    let from: Vec<(&str, String, Status)> = report
        .from
        .iter()
        .map(|reference| {
            (
                reference.pointer.as_str(),
                reference.target.to_string(),
                reference.status,
            )
        })
        .collect();
    assert_eq!(
        from,
        [
            (
                "/project",
                "/tasks/a".to_owned(),
                Status::Degraded(Code::RefWrongType)
            ),
            (
                "/see",
                "/nowhere".to_owned(),
                Status::Degraded(Code::RefDangling)
            ),
        ]
    );
    assert!(report.from[1].location.is_none());
    assert_eq!(
        report.from[0].location.as_ref().unwrap().path,
        "tasks/a.json"
    );
}

#[test]
fn a_record_named_two_ways_lists_each_referring_record_once() {
    let scenario = linked();
    let report = refs(&scenario, "/projects/p").unwrap();
    let to: Vec<(&str, Vec<&str>)> = report
        .to
        .iter()
        .map(|incoming| {
            (
                incoming.path.as_str(),
                incoming.pointers.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        to,
        [
            ("tasks/a.json", vec!["/project"]),
            ("tasks/b.json", vec!["/see"]),
            ("tasks/c.json", vec!["/project", "/see"]),
        ]
    );
    let through_link = refs(&scenario, "/links/l").unwrap();
    let identity =
        |report: &RefsReport| (report.location.board.clone(), report.location.path.clone());
    assert_eq!(identity(&through_link), identity(&report), "one identity");
    assert_eq!(through_link.to, report.to);
    assert_eq!(through_link.display.to_string(), "/projects/p");
}

#[test]
fn refs_of_a_path_that_names_nothing_is_refused_and_one_that_names_a_record_is_admitted() {
    let scenario = linked();
    assert!(refs(&scenario, "/tasks/a").is_ok());
    assert_eq!(
        refs(&scenario, "/tasks/zz").unwrap_err(),
        Code::PathNotFound
    );
    assert_eq!(
        open(&scenario)
            .refs(&path("/tasks/zz"), None)
            .unwrap_err()
            .fields(),
        Fields::at(&path("/tasks/zz"))
    );
}

#[test]
fn refs_inside_a_mounted_board_finds_the_records_there_that_reach_it() {
    use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA};
    let library = Scenario::new();
    let commit = library.commit(&[
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
        ("tasks/ok.json", &task(r#", "project": "/projects/p""#)),
    ]);
    let scenario = linked();
    scenario.write(
        "vendor/lib.json",
        Item::File(&format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            library.path()
        )),
    );
    let report = refs(&scenario, "/vendor/lib/projects/p").unwrap();
    let to: Vec<(String, &str)> = report
        .to
        .iter()
        .map(|incoming| {
            (
                incoming.logical.as_ref().unwrap().to_string(),
                incoming.path.as_str(),
            )
        })
        .collect();
    assert_eq!(
        to,
        [("/vendor/lib/tasks/ok".to_owned(), "tasks/ok.json")],
        "only the mounted task reaches the mounted project"
    );
}

/// What `refs` answers for `text` through the index under `cache`, less
/// whether that index was in memory.
fn answered(generated: &Generated, text: &str, cache: Option<&Path>) -> Result<RefsReport, Code> {
    open(generated.scenario())
        .refs(&path(text), cache)
        .map(|report| RefsReport {
            memory: None,
            ..report
        })
        .map_err(|error| error.code())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn refs_answers_from_a_warm_index_as_from_a_cold_one(
        before in generated(),
        after in generated(),
        committed: bool,
    ) {
        let mut generated = Generated::new(r#"{"$type": "board/config/v1"}"#, &before, committed);
        let cache = tempfile::tempdir().unwrap();
        let _warming = answered(&generated, "/projects/p", Some(cache.path()));
        generated.write(&after);
        let logical: Vec<String> = open(generated.scenario())
            .check(None, None)
            .unwrap()
            .entries
            .into_iter()
            .filter_map(|entry| entry.logical.map(|logical| logical.to_string()))
            .collect();
        for text in logical.iter().map(String::as_str).chain(["/nowhere"]) {
            prop_assert_eq!(
                answered(&generated, text, Some(cache.path())),
                answered(&generated, text, None),
                "{}",
                text
            );
        }
    }
}
