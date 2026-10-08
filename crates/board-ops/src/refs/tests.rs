//! References from and to an object.

use std::path::Path;

use board_model::{Code, Fields, LogicalPath};
use board_scenario::{Item, Scenario};
use board_tree::Status;

use crate::RefsReport;
use crate::tests::{board, open, task};

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

fn refs(scenario: &Scenario, text: &str) -> Result<RefsReport, Code> {
    open(scenario)
        .refs(&path(text), None)
        .map_err(|error| error.code())
}

/// Where `refs --missing` says `text` would be: the physical path, the
/// display path and the segments past the deepest ancestor that resolves.
fn missing(scenario: &Scenario, text: &str) -> Result<(String, String, Vec<String>), Code> {
    let report = open(scenario)
        .refs_or_missing(&path(text), None)
        .map_err(|error| error.code())?;
    let missing = report.missing.expect("nothing is at the path");
    assert!(report.from.is_empty() && report.to.is_empty());
    Ok((missing.path, missing.display.to_string(), missing.segments))
}

fn segments(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
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

#[test]
fn a_missing_path_through_a_link_is_where_the_link_leads_and_without_the_flag_it_is_refused() {
    let scenario = linked();
    assert_eq!(
        missing(&scenario, "/links/l/notes/new.md"),
        Ok((
            "projects/p/notes/new.md".to_owned(),
            "/projects/p/notes/new.md".to_owned(),
            segments(&["notes", "new.md"])
        )),
        "a record's children are in the directory beside it"
    );
    assert_eq!(
        refs(&scenario, "/links/l/notes/new.md").unwrap_err(),
        Code::PathNotFound
    );
}

#[test]
fn a_path_that_exists_reports_nothing_missing_with_the_flag_as_without_it() {
    let scenario = linked();
    let flagged = open(&scenario)
        .refs_or_missing(&path("/links/l"), None)
        .unwrap();
    assert_eq!(flagged.missing, None);
    assert_eq!(Ok(flagged), refs(&scenario, "/links/l"));
}

#[test]
fn a_missing_path_under_a_file_is_not_found_and_one_under_a_directory_is_admitted() {
    let scenario = linked();
    scenario.write("notes.md", Item::File("text"));
    assert_eq!(
        missing(&scenario, "/notes.md/new.md"),
        Err(Code::PathNotFound)
    );
    assert_eq!(
        missing(&scenario, "/tasks/new.md"),
        Ok((
            "tasks/new.md".to_owned(),
            "/tasks/new.md".to_owned(),
            segments(&["new.md"])
        ))
    );
}

#[test]
fn a_missing_path_under_a_dangling_link_is_refused_as_dangling_and_once_it_resolves_is_admitted() {
    let scenario = linked();
    scenario.write(
        "links/gone.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/elsewhere"}"#),
    );
    assert_eq!(
        missing(&scenario, "/links/gone/new.md"),
        Err(Code::RefDangling)
    );
    scenario.write("elsewhere/kept.md", Item::File("text"));
    assert_eq!(
        missing(&scenario, "/links/gone/new.md"),
        Ok((
            "elsewhere/new.md".to_owned(),
            "/elsewhere/new.md".to_owned(),
            segments(&["new.md"])
        ))
    );
}

#[test]
fn a_path_naming_an_existing_record_by_its_file_is_not_found_and_a_new_records_file_is_admitted() {
    let scenario = linked();
    assert_eq!(missing(&scenario, "/tasks/a.json"), Err(Code::PathNotFound));
    assert_eq!(
        missing(&scenario, "/tasks/new.json"),
        Ok((
            "tasks/new.json".to_owned(),
            "/tasks/new.json".to_owned(),
            segments(&["new.json"])
        ))
    );
}

#[test]
fn a_missing_path_through_a_mount_is_where_it_would_be_in_the_mounted_commit() {
    let library = Scenario::new();
    let commit = library.commit(&[("docs/a.md", "text")]);
    let scenario = linked();
    scenario.write(
        "vendor/lib.json",
        Item::File(&format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            library.path()
        )),
    );
    let report = open(&scenario)
        .refs_or_missing(&path("/vendor/lib/docs/new.md"), None)
        .unwrap();
    assert_eq!(
        report.location.board.source,
        board_model::Source::Commit(commit)
    );
    let missing = report.missing.unwrap();
    assert_eq!(
        (missing.path.as_str(), missing.display.to_string()),
        ("docs/new.md", "/vendor/lib/docs/new.md".to_owned())
    );
}

/// What `refs` answers for `text` through the index under `cache`, less
/// whether that index was in memory.
fn answered(scenario: &Scenario, text: &str, cache: Option<&Path>) -> RefsReport {
    RefsReport {
        memory: None,
        ..open(scenario).refs(&path(text), cache).unwrap()
    }
}

#[test]
fn refs_answers_from_a_warm_index_as_from_a_cold_one_after_the_references_change() {
    let scenario = linked();
    let cache = tempfile::tempdir().unwrap();
    let before = answered(&scenario, "/projects/p", Some(cache.path()));
    scenario
        .write("tasks/a.json", Item::File(&task(r#", "see": "/links/l""#)))
        .write("tasks/b.json", Item::File(&task("")));
    let warm = answered(&scenario, "/projects/p", Some(cache.path()));
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm, answered(&scenario, "/projects/p", None));
}
