//! References from and to an object.

use std::path::Path;

use board_model::{Code, Fields, LogicalPath};
use board_scenario::{Item, Scenario};
use board_tree::Status;
use serde_json::{Value, json};

use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};
use crate::{Absent, Board, Fielded, RefsError, RefsReport};

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

/// `refs` of one path, as most tests ask, or of several, collected.
trait One {
    fn one(
        &self,
        text: &str,
        absent: Absent,
        cache: Option<&Path>,
    ) -> Result<RefsReport, RefsError>;

    fn refs_all(
        &self,
        paths: &[LogicalPath],
        absent: Absent,
        cache: Option<&Path>,
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError>;
}

impl One for Board {
    fn refs_all(
        &self,
        paths: &[LogicalPath],
        absent: Absent,
        cache: Option<&Path>,
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        Ok(self.refs(paths, absent, &[], None, cache)?.collect())
    }

    fn one(
        &self,
        text: &str,
        absent: Absent,
        cache: Option<&Path>,
    ) -> Result<RefsReport, RefsError> {
        let mut answers = self.refs_all(&[path(text)], absent, cache)?;
        assert_eq!(answers.len(), 1, "one answer for one path");
        answers.remove(0)
    }
}

/// How often the run that traced `board` entered the phase `name`.
fn entered(board: &Board, name: &str) -> usize {
    board
        .take_trace()
        .phases
        .iter()
        .find(|phase| phase.name == name)
        .map_or(0, |phase| phase.entered)
}

#[test]
fn several_paths_are_answered_in_order_from_one_snapshot_each_as_alone() {
    let scenario = linked();
    let asked = ["/tasks/c", "/projects/p", "/tasks/c"];
    let board = open(&scenario);
    let answers = board
        .refs_all(&asked.map(path), Absent::Refuse, None)
        .unwrap();
    assert_eq!(
        entered(&board, "snapshot"),
        1,
        "one snapshot for three paths"
    );
    let alone: Vec<RefsReport> = asked
        .iter()
        .map(|text| open(&scenario).one(text, Absent::Refuse, None).unwrap())
        .collect();
    let answers: Vec<RefsReport> = answers.into_iter().map(Result::unwrap).collect();
    assert_eq!(answers, alone);
}

#[test]
fn a_path_that_names_nothing_among_several_is_refused_alone_and_the_others_are_answered() {
    let scenario = linked();
    let board = open(&scenario);
    let answers = board
        .refs_all(
            &[path("/tasks/a"), path("/tasks/zz"), path("/projects/p")],
            Absent::Refuse,
            None,
        )
        .unwrap();
    let codes: Vec<Result<String, Code>> = answers
        .into_iter()
        .map(|answer| {
            answer
                .map(|report| report.display.to_string())
                .map_err(|error| error.code())
        })
        .collect();
    assert_eq!(
        codes,
        [
            Ok("/tasks/a".to_owned()),
            Err(Code::PathNotFound),
            Ok("/projects/p".to_owned())
        ]
    );
    let admitted = board
        .refs_all(
            &[path("/tasks/a"), path("/projects/p")],
            Absent::Refuse,
            None,
        )
        .unwrap();
    assert!(admitted.iter().all(Result::is_ok));
}

#[test]
fn paths_that_all_name_nothing_take_no_snapshot_and_one_that_resolves_takes_one() {
    let scenario = linked();
    let board = open(&scenario);
    let answers = board
        .refs_all(
            &[path("/tasks/a.json"), path("/tasks/new.md")],
            Absent::Locate,
            None,
        )
        .unwrap();
    assert_eq!(answers[0].as_ref().unwrap_err().code(), Code::PathNotFound);
    assert_eq!(
        answers[1].as_ref().unwrap().missing.as_ref().unwrap().path,
        "tasks/new.md"
    );
    assert_eq!(entered(&board, "snapshot"), 0);
    board
        .refs_all(&[path("/tasks/zz"), path("/tasks/a")], Absent::Locate, None)
        .unwrap();
    assert_eq!(entered(&board, "snapshot"), 1);
}

fn refs(scenario: &Scenario, text: &str) -> Result<RefsReport, Code> {
    open(scenario)
        .one(text, Absent::Refuse, None)
        .map_err(|error| error.code())
}

/// Where `refs --missing` says `text` would be: the physical path, the
/// display path and the segments past the deepest ancestor that resolves.
fn missing(scenario: &Scenario, text: &str) -> Result<(String, String, Vec<String>), Code> {
    let report = open(scenario)
        .one(text, Absent::Locate, None)
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
            .one("/tasks/zz", Absent::Refuse, None)
            .unwrap_err()
            .refusal()
            .fields,
        Fields {
            ancestor: Some(path("/tasks")),
            ..Fields::at(&path("/tasks/zz"))
        }
        .json()
    );
}

#[test]
fn refs_inside_a_mounted_board_finds_the_records_there_that_reach_it() {
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
        .one("/links/l", Absent::Locate, None)
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
    let refused = open(&scenario)
        .one("/tasks/a.json", Absent::Locate, None)
        .unwrap_err();
    assert_eq!(
        refused.refusal().fields,
        Fields {
            ancestor: Some(path("/tasks")),
            ..Fields::at(&path("/tasks/a.json"))
        }
        .json()
    );
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
        .one("/vendor/lib/docs/new.md", Absent::Locate, None)
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
        ..open(scenario).one(text, Absent::Refuse, cache).unwrap()
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

/// What `refs` answers for each of `texts`, without where the index was.
fn answered_each(scenario: &Scenario, texts: &[&str], cache: Option<&Path>) -> Vec<RefsReport> {
    let paths: Vec<LogicalPath> = texts.iter().map(|text| path(text)).collect();
    open(scenario)
        .refs_all(&paths, Absent::Refuse, cache)
        .unwrap()
        .into_iter()
        .map(|answer| RefsReport {
            memory: None,
            ..answer.unwrap()
        })
        .collect()
}

#[test]
fn refs_of_several_paths_answers_from_a_warm_index_as_from_a_cold_one_after_the_references_change()
{
    let scenario = linked();
    let cache = tempfile::tempdir().unwrap();
    let asked = ["/projects/p", "/tasks/a", "/tasks/b"];
    let before = answered_each(&scenario, &asked, Some(cache.path()));
    scenario
        .write("tasks/a.json", Item::File(&task(r#", "see": "/links/l""#)))
        .write("tasks/b.json", Item::File(&task("")));
    let warm = answered_each(&scenario, &asked, Some(cache.path()));
    for (index, (warm, before)) in warm.iter().zip(&before).enumerate() {
        assert_ne!(warm, before, "the change is seen at {}", asked[index]);
    }
    assert_eq!(warm, answered_each(&scenario, &asked, None));
}

#[test]
fn refs_of_a_json_path_whose_stem_names_an_entry_hints_the_stem_and_one_whose_stem_names_nothing_does_not()
 {
    let scenario = linked();
    let hint = |text: &str| {
        let refused = open(&scenario).one(text, Absent::Refuse, None).unwrap_err();
        assert_eq!(refused.code(), Code::PathNotFound, "{text}");
        refused.refusal().hint.unwrap()
    };
    assert_eq!(
        hint("/tasks/a.json"),
        "a logical path leaves out .json: try `board refs /tasks/a`"
    );
    // `/links/l` is a link, so its stem resolves through it.
    assert_eq!(
        hint("/links/l.json"),
        "a logical path leaves out .json: try `board refs /links/l`"
    );
    assert_eq!(
        hint("/tasks/zz.json"),
        Code::PathNotFound.hint(&Fields {
            ancestor: Some(path("/tasks")),
            ..Fields::at(&path("/tasks/zz.json"))
        })
    );
    assert_eq!(
        hint("/tasks/zz"),
        Code::PathNotFound.hint(&Fields {
            ancestor: Some(path("/tasks")),
            ..Fields::at(&path("/tasks/zz"))
        })
    );
}

/// What `refs` of `text`, asking for `fields`, gives each reference the
/// record makes: its pointer, and its target's values and code.
fn fielded(
    scenario: &Scenario,
    text: &str,
    fields: &[&str],
    cache: Option<&Path>,
) -> Vec<(String, Fielded)> {
    limited(scenario, text, fields, None, cache)
        .into_iter()
        .map(|(pointer, fielded)| (pointer, fielded.unwrap()))
        .collect()
}

/// [`fielded`], within `limit`, where a reference whose target was not
/// read has `None`.
fn limited(
    scenario: &Scenario,
    text: &str,
    fields: &[&str],
    limit: Option<usize>,
    cache: Option<&Path>,
) -> Vec<(String, Option<Fielded>)> {
    let fields: Vec<String> = fields.iter().map(|field| (*field).to_owned()).collect();
    let paths = [path(text)];
    let board = open(scenario);
    let report = board
        .refs(&paths, Absent::Refuse, &fields, limit, cache)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    report
        .from
        .into_iter()
        .map(|outgoing| (outgoing.pointer, outgoing.fields))
        .collect()
}

/// The values of a target that reads as a record, with no code.
fn values(values: &[(&str, Option<Value>)]) -> Fielded {
    Fielded {
        values: Some(
            values
                .iter()
                .map(|(pointer, value)| ((*pointer).to_owned(), value.clone()))
                .collect(),
        ),
        code: None,
    }
}

#[test]
fn fields_are_read_from_each_target_where_refs_resolves_it_through_links_and_mounts() {
    let library = Scenario::new();
    let commit = library.commit(&[
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        (
            "tasks/ok.json",
            r#"{"$type": "lr/task/v1", "title": "mounted"}"#,
        ),
    ]);
    let scenario = linked();
    scenario
        .write(
            "vendor/lib.json",
            Item::File(&format!(
                r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
                library.path()
            )),
        )
        .write(
            "projects/p.json",
            Item::File(r#"{"$type": "lr/project/v1", "title": "p"}"#),
        )
        .write(
            "tasks/d.json",
            Item::File(&task(r#", "project": "/links/l", "see": "/vendor/lib/tasks/ok""#)),
        );
    let fields = ["/title", "/$type", "/nope"];
    assert_eq!(
        fielded(&scenario, "/tasks/d", &fields, None),
        [
            (
                "/project".to_owned(),
                values(&[
                    ("/title", Some(json!("p"))),
                    ("/$type", Some(json!("lr/project/v1"))),
                    ("/nope", None),
                ])
            ),
            (
                "/see".to_owned(),
                values(&[
                    ("/title", Some(json!("mounted"))),
                    ("/$type", Some(json!("lr/task/v1"))),
                    ("/nope", None),
                ])
            ),
        ]
    );
}

#[test]
fn a_target_that_is_not_a_record_has_its_code_without_values_and_a_degraded_one_has_both() {
    let scenario = linked();
    scenario
        .write("notes.md", Item::File("# n\n"))
        .write(
            "tasks/bad.json",
            Item::File(r#"{"$type": "lr/task/v1", "status": "done"}"#),
        )
        .write(
            "tasks/to-bad.json",
            Item::File(&task(r#", "see": "/tasks/bad""#)),
        )
        .write(
            "tasks/to-note.json",
            Item::File(&task(r#", "see": "/notes.md""#)),
        )
        .write(
            "tasks/to-dir.json",
            Item::File(&task(r#", "see": "/projects""#)),
        )
        .write(
            "tasks/to-none.json",
            Item::File(&task(r#", "see": "/nowhere""#)),
        );
    let one = |text: &str| fielded(&scenario, text, &["/status"], None).remove(0).1;
    let unread = |code| Fielded {
        values: None,
        code: Some(code),
    };
    assert_eq!(
        one("/tasks/to-bad"),
        Fielded {
            code: Some(Code::RecordInvalid),
            ..values(&[("/status", Some(json!("done")))])
        }
    );
    assert_eq!(one("/tasks/to-note"), unread(Code::RecordNotARecord));
    assert_eq!(one("/tasks/to-dir"), unread(Code::RecordNotARecord));
    // A reference that resolves to nothing has no target to read.
    let dangling = limited(&scenario, "/tasks/to-none", &["/status"], None, None);
    assert_eq!(dangling, [("/see".to_owned(), None)]);
    // The twin: a target check admits has its values and no code.
    assert_eq!(
        one("/tasks/b"),
        values(&[("/status", None)]),
        "/links/l leads to /projects/p"
    );
}

#[test]
fn a_targets_code_is_the_same_from_a_warm_index_as_from_a_cold_one_after_it_changes() {
    let scenario = linked();
    scenario.write("tasks/d.json", Item::File(&task(r#", "see": "/tasks/a""#)));
    let cache = tempfile::tempdir().unwrap();
    let code = |cache| {
        fielded(&scenario, "/tasks/d", &["/title"], cache)
            .remove(0)
            .1
            .code
    };
    assert_eq!(code(Some(cache.path())), None);
    scenario.write("tasks/a.json", Item::File(r#"{"$type": "lr/task/v1"}"#));
    assert_eq!(code(Some(cache.path())), Some(Code::RecordInvalid));
    assert_eq!(code(None), Some(Code::RecordInvalid));
}

#[test]
fn a_run_without_fields_reads_no_target() {
    let scenario = linked();
    let board = open(&scenario);
    let paths = [path("/tasks/a")];
    let report = board
        .refs(&paths, Absent::Refuse, &[], None, None)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert!(report.from.iter().all(|outgoing| outgoing.fields.is_none()));
}

#[test]
fn a_reference_past_the_limit_has_no_read_for_its_target_and_one_within_it_has() {
    let scenario = linked();
    scenario.write(
        "tasks/d.json",
        Item::File(&task(r#", "project": "/links/l", "see": "/tasks/a""#)),
    );
    let read = |limit| -> Vec<bool> {
        limited(&scenario, "/tasks/d", &["/status"], Some(limit), None)
            .iter()
            .map(|(_, fielded)| fielded.is_some())
            .collect()
    };
    assert_eq!(read(1), [true, false]);
    assert_eq!(read(2), [true, true]);
}

#[test]
fn a_target_changed_after_the_check_is_target_changed_without_values_and_counted_and_its_twin_is_read()
 {
    // The first answer takes the check; the second reads its target after
    // `change` has run, from that same check.
    let second = |change: bool| {
        let scenario = linked();
        let board = open(&scenario);
        let paths = [path("/tasks/a"), path("/tasks/b")];
        let fields = ["/title".to_owned()];
        let mut answers = board
            .refs(&paths, Absent::Refuse, &fields, None, None)
            .unwrap();
        answers.next().unwrap().unwrap();
        if change {
            scenario.write(
                "projects/p.json",
                Item::File(r#"{"$type": "lr/project/v1", "title": "new"}"#),
            );
        }
        let fielded = answers.next().unwrap().unwrap().from.remove(0).fields;
        (
            fielded,
            board.take_trace().work.get("target_changed").copied(),
        )
    };
    assert_eq!(
        second(true),
        (
            Some(Fielded {
                values: None,
                code: Some(Code::RefsTargetChanged),
            }),
            Some(1)
        )
    );
    assert_eq!(second(false), (Some(values(&[("/title", None)])), None));
}

#[test]
fn a_target_that_resolved_to_another_blob_than_the_walk_listed_is_target_changed_and_its_twin_is_read()
 {
    // No seam lies between the check's walk and its resolver, so the test
    // gives the snapshot what that resolver finds when the file is rewritten
    // between them: it hashes the file again and leads to the new blob,
    // while the walk, and so the code, keep the old one.
    let answered = |rewrite: bool| {
        let scenario = linked();
        let board = open(&scenario);
        let mut snapshot = board.snapshot(board.index(None).unwrap(), None).unwrap();
        if rewrite {
            let new = r#"{"$type": "lr/project/v1", "title": "new"}"#;
            scenario.write("projects/p.json", Item::File(new));
            for referenced in &mut snapshot.references {
                if let Some(location) = &mut referenced.reached.location
                    && location.path == "projects/p.json"
                {
                    location.object = board_model::Object::Blob(scenario.blob(new));
                }
            }
        }
        let fields = ["/title".to_owned()];
        let mut answering = super::Answering {
            board: &board,
            view: board.repositories().snapshot().unwrap(),
            absent: Absent::Refuse,
            fields: &fields,
            limit: usize::MAX,
            cache: None,
            snapshot: Some(snapshot),
        };
        let report = answering.answer(&path("/tasks/b")).unwrap();
        let fielded = report.from.into_iter().next().unwrap().fields;
        (
            fielded,
            board.take_trace().work.get("target_changed").copied(),
        )
    };
    assert_eq!(
        answered(true),
        (
            Some(Fielded {
                values: None,
                code: Some(Code::RefsTargetChanged),
            }),
            Some(1)
        )
    );
    assert_eq!(answered(false), (Some(values(&[("/title", None)])), None));
}
