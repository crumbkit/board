//! `put --batch`: every line is checked and every expect compared before
//! anything is written, and the writes share one commit.

use board_model::{Code, Lined, RefusalCode};
use board_scenario::Item;
use board_tree::LOCK_WAIT;

use super::{board, canonical, committing, file};
use crate::put::{MAX_BATCH_LINES, PutError, PutReport, Schemas};
use crate::tests::{open, task};

fn line(path: &str, expect: Option<&str>, record: &str) -> String {
    let expect = expect.map_or_else(String::new, |expect| format!(r#""expect": "{expect}", "#));
    format!(r#"{{"path": "{path}", {expect}"record": {record}}}"#) + "\n"
}

/// A batch line that writes `text` as a file.
fn text_line(path: &str, text: &str) -> String {
    let text = serde_json::Value::String(text.to_owned());
    format!(r#"{{"path": "{path}", "text": {text}}}"#) + "\n"
}

fn batch(scenario: &board_scenario::Scenario, input: &str) -> Result<PutReport, PutError> {
    open(scenario).put_batch(&mut input.as_bytes())
}

/// The refusals of a refused batch, as (line, code).
fn refused(result: Result<PutReport, PutError>) -> (Code, Vec<(usize, Code)>) {
    match result.unwrap_err() {
        PutError::Batch { code, refusals } => (
            code,
            refusals
                .iter()
                .map(|Lined { line, code, .. }| (*line, *code))
                .collect(),
        ),
        other => panic!("not a batch refusal: {other}"),
    }
}

#[test]
fn a_batch_writes_every_line_in_order_with_one_commit() {
    let scenario = committing();
    let input = line("/tasks/a", Some("none"), &task(""))
        + "\n"
        + &line("/tasks/b", None, &task(r#", "see": "/tasks/a""#));
    let report = batch(&scenario, &input).unwrap();
    let displays: Vec<String> = report
        .writes
        .iter()
        .map(|written| written.display.to_string())
        .collect();
    assert_eq!(displays, ["/tasks/a", "/tasks/b"]);
    let head = scenario.head().unwrap();
    assert_eq!(Some(head.id), report.commit);
    assert_eq!(head.message, "board put --batch (2 records)");
    assert_eq!(
        head.files.keys().collect::<Vec<_>>(),
        ["tasks/a.json", "tasks/b.json"]
    );
    assert_eq!(head.files["tasks/a.json"], canonical(&task("")));
}

#[test]
fn a_batch_writes_records_and_files_in_one_commit_and_the_record_s_references_resolve_to_the_files()
{
    let scenario = committing();
    let input = line(
        "/reports/r1",
        Some("none"),
        &task(r#", "see": "/reports/r1/notes.md""#),
    ) + &text_line("/reports/r1/notes.md", "# Notes\n")
        + &text_line("/reports/r1/data.csv", "a,b\n1,2\n")
        + &text_line("/reports/r1/log.txt", "ok\n");
    let report = batch(&scenario, &input).unwrap();
    let displays: Vec<String> = report
        .writes
        .iter()
        .map(|written| written.display.to_string())
        .collect();
    assert_eq!(
        displays,
        [
            "/reports/r1",
            "/reports/r1/notes.md",
            "/reports/r1/data.csv",
            "/reports/r1/log.txt"
        ]
    );
    assert!(
        report
            .writes
            .iter()
            .all(|written| written.degraded.is_none())
    );
    let head = scenario.head().unwrap();
    assert_eq!(Some(head.id), report.commit);
    assert_eq!(head.message, "board put --batch (1 records, 3 files)");
    assert_eq!(
        head.files.keys().collect::<Vec<_>>(),
        [
            "reports/r1.json",
            "reports/r1/data.csv",
            "reports/r1/log.txt",
            "reports/r1/notes.md"
        ]
    );
    assert_eq!(head.files["reports/r1/notes.md"], b"# Notes\n");
    assert_eq!(
        file(&scenario, "reports/r1/data.csv").unwrap(),
        b"a,b\n1,2\n"
    );
    let files = text_line("/a.md", "a") + &text_line("/b.md", "b");
    assert_eq!(
        scenario.head().unwrap().message,
        "board put --batch (1 records, 3 files)"
    );
    batch(&scenario, &files).unwrap();
    assert_eq!(
        scenario.head().unwrap().message,
        "board put --batch (2 files)"
    );
}

#[test]
fn a_line_with_both_record_and_text_or_neither_is_refused_and_a_line_with_one_is_admitted() {
    let scenario = board();
    let both = format!(
        r#"{{"path": "/n.md", "record": {}, "text": "x"}}"#,
        task("")
    ) + "\n";
    let neither = "{\"path\": \"/n.md\"}\n";
    let json_named = text_line("/n.json", "x");
    let result = batch(&scenario, &(both + neither + &json_named));
    let messages: Vec<String> = match &result {
        Err(PutError::Batch { refusals, .. }) => {
            refusals.iter().map(|lined| lined.message.clone()).collect()
        }
        other => panic!("not a batch refusal: {other:?}"),
    };
    assert_eq!(
        messages[..2],
        [
            "the line is not {path, expect, record} or {path, expect, text}: it holds both record and text",
            "the line is not {path, expect, record} or {path, expect, text}: it holds neither record nor text",
        ]
    );
    let (code, refusals) = refused(result);
    assert_eq!(code, Code::PutInvalid);
    assert_eq!(
        refusals,
        [
            (1, Code::PutInvalid),
            (2, Code::PutInvalid),
            (3, Code::PutNotAFile)
        ]
    );
    assert!(file(&scenario, "n.md").is_none());
    let twin = text_line("/n.md", "x");
    assert_eq!(batch(&scenario, &twin).unwrap().writes.len(), 1);
    assert_eq!(file(&scenario, "n.md").unwrap(), b"x");
}

#[test]
fn a_batch_with_one_invalid_record_writes_nothing_and_its_twin_writes_all() {
    let scenario = board();
    let valid = line("/tasks/a", None, &task(""));
    let invalid = line("/tasks/b", None, r#"{"$type": "lr/task/v1", "title": 1}"#);
    let (code, refusals) = refused(batch(&scenario, &(valid.clone() + &invalid)));
    assert_eq!(code, Code::PutInvalid);
    assert_eq!(refusals, [(2, Code::RecordInvalid)]);
    assert!(file(&scenario, "tasks/a.json").is_none());
    let twin = line("/tasks/b", None, &task(""));
    assert_eq!(batch(&scenario, &(valid + &twin)).unwrap().writes.len(), 2);
}

#[test]
fn every_refused_line_is_listed_with_its_own_code() {
    let scenario = board();
    let input = [
        line("/tasks/a", None, &task("")),
        "not json\n".to_owned(),
        r#"{"path": "/tasks/c", "record": {"$type": "lr/task/v1", "title": "t"}, "extra": 1}"#
            .to_owned()
            + "\n",
        line("/tasks/d", Some("xyz"), &task("")),
        line(
            "/tasks/e",
            None,
            r#"{"$type": "lr/task/v1", "title": 1.00000000000000000001}"#,
        ),
        line("/tasks/f", None, r#"{"$type": "lr/nothing/v1"}"#),
    ]
    .concat();
    let (code, refusals) = refused(batch(&scenario, &input));
    assert_eq!(code, Code::PutInvalid);
    assert_eq!(
        refusals,
        [
            (2, Code::JsonParse),
            (3, Code::PutInvalid),
            (4, Code::PutInvalid),
            (5, Code::JsonLossyNumber),
            (6, Code::RecordUnknownType),
        ]
    );
    assert!(file(&scenario, "tasks/a.json").is_none());
}

#[test]
fn a_stale_expect_refuses_the_batch_and_lists_its_lines() {
    let scenario = board();
    let existing = batch(&scenario, &line("/tasks/a", None, &task("")))
        .unwrap()
        .writes[0]
        .blob
        .to_string();
    let stale = line("/tasks/b", None, &task(""))
        + &line("/tasks/a", Some("none"), &task(r#", "see": "/x""#));
    let (code, refusals) = refused(batch(&scenario, &stale));
    assert_eq!(code, Code::PutStale);
    assert_eq!(refusals, [(2, Code::PutStale)]);
    assert!(file(&scenario, "tasks/b.json").is_none());
    let fresh = line("/tasks/b", None, &task(""))
        + &line("/tasks/a", Some(&existing), &task(r#", "see": "/x""#));
    assert_eq!(batch(&scenario, &fresh).unwrap().writes.len(), 2);
}

#[test]
fn a_stale_line_names_what_it_found_in_its_own_fields_and_a_line_expecting_that_is_admitted() {
    let scenario = board();
    let held = batch(&scenario, &line("/tasks/a", None, &task("")))
        .unwrap()
        .writes[0]
        .blob
        .to_string();
    let stale =
        line("/tasks/a", Some("none"), &task("")) + &line("/tasks/b", Some(&held), &task(""));
    let refusal = batch(&scenario, &stale).unwrap_err().refusal();
    assert_eq!(refusal.code, RefusalCode::Board(Code::PutStale));
    assert_eq!(
        refusal.fields["refusals"],
        serde_json::json!([
            {
                "line": 1,
                "code": "board::put::stale",
                "message": format!("/tasks/a is not what --expect says: it holds {held}"),
                "fields": { "path": "/tasks/a", "blob": held, "found": held },
            },
            {
                "line": 2,
                "code": "board::put::stale",
                "message": "/tasks/b is not what --expect says: it holds nothing",
                "fields": { "path": "/tasks/b", "found": null },
            },
        ])
    );
    let admitted =
        line("/tasks/a", Some(&held), &task("")) + &line("/tasks/b", Some("none"), &task(""));
    assert_eq!(batch(&scenario, &admitted).unwrap().writes.len(), 2);
}

#[test]
fn two_lines_that_write_one_file_are_refused_and_two_files_are_admitted() {
    let scenario = board();
    scenario.write(
        "links/a.json",
        board_scenario::Item::File(r#"{"$type": "board/mount/v1", "path": "/tasks/a"}"#),
    );
    scenario.write("tasks/a.json", board_scenario::Item::File(&task("")));
    let same = line("/tasks/a", None, &task("")) + &line("/links/a", None, &task(""));
    let refusal = batch(&scenario, &same).unwrap_err().refusal();
    assert_eq!(refusal.code, RefusalCode::Board(Code::PutInvalid));
    assert_eq!(
        refusal.fields["refusals"],
        serde_json::json!([{
            "line": 2,
            "code": "board::put::invalid",
            "message": "it writes tasks/a.json as line 1 writes tasks/a.json",
            "fields": { "path": "/links/a" },
        }])
    );
    let apart = line("/tasks/a", None, &task("")) + &line("/tasks/b", None, &task(""));
    assert!(batch(&scenario, &apart).is_ok());
}

#[test]
fn a_batch_of_more_than_the_most_lines_is_refused_and_one_of_exactly_that_many_is_admitted() {
    let scenario = board();
    let most = "\n".repeat(MAX_BATCH_LINES);
    assert_eq!(batch(&scenario, &most).unwrap().writes, []);
    let over = most + "\n";
    let refusal = batch(&scenario, &over).unwrap_err();
    assert!(matches!(refusal, PutError::TooManyLines));
    assert_eq!(refusal.code(), Code::PutInvalid);
}

#[test]
fn an_empty_batch_writes_nothing_and_makes_no_commit() {
    let scenario = committing();
    let report = batch(&scenario, "").unwrap();
    assert_eq!((report.writes.len(), report.commit), (0, None));
    assert!(scenario.head().is_none());
}

#[test]
fn line_numbers_count_blank_lines() {
    let scenario = board();
    let input = "\n\n".to_owned() + &line("/tasks/a", None, r#"{"$type": "lr/task/v1"}"#);
    assert_eq!(
        refused(batch(&scenario, &input)).1,
        [(3, Code::RecordInvalid)]
    );
}

#[test]
fn a_line_over_the_record_limit_is_too_large_and_one_at_it_is_admitted() {
    let scenario = board();
    let sized = |bytes: usize| {
        let empty = line("/tasks/a", None, &task("")).len() - 1;
        let title = "t".repeat(bytes - empty + 1);
        line(
            "/tasks/a",
            None,
            &format!(r#"{{"$type": "lr/task/v1", "title": "{title}"}}"#),
        )
    };
    let at = sized(board_model::MAX_RECORD_BYTES);
    assert_eq!(at.len(), board_model::MAX_RECORD_BYTES + 1);
    batch(&scenario, &at).unwrap();
    let over = sized(board_model::MAX_RECORD_BYTES + 1);
    assert_eq!(
        refused(batch(&scenario, &over)).1,
        [(1, Code::JsonTooLarge)]
    );
}

#[test]
fn two_lines_differing_only_in_case_collide_and_two_names_are_admitted() {
    let scenario = board();
    let folded = line("/tasks/a", None, &task("")) + &line("/tasks/A", None, &task(""));
    let refusal = batch(&scenario, &folded).unwrap_err();
    assert_eq!(
        refusal.refusal().fields["refusals"][0]["message"],
        "it writes tasks/A.json differing only in case from line 1 writes tasks/a.json"
    );
    assert_eq!(refused(Err(refusal)).1, [(2, Code::PathCaseCollision)]);
    assert!(file(&scenario, "tasks/a.json").is_none());
    let apart = line("/tasks/a", None, &task("")) + &line("/tasks/b", None, &task(""));
    assert_eq!(batch(&scenario, &apart).unwrap().writes.len(), 2);
}

#[test]
fn a_batch_line_into_git_refuses_the_batch_and_its_twin_writes_all() {
    let scenario = board();
    let valid = line("/tasks/a", None, &task(""));
    let into_git = line("/.git/hooks/x", None, &task(""));
    let (code, refusals) = refused(batch(&scenario, &(valid.clone() + &into_git)));
    assert_eq!(code, Code::PutInvalid);
    assert_eq!(refusals, [(2, Code::PathMalformed)]);
    assert!(file(&scenario, "tasks/a.json").is_none());
    assert!(file(&scenario, ".git/hooks/x.json").is_none());
    let twin = line("/git/hooks/x", None, &task(""));
    assert_eq!(batch(&scenario, &(valid + &twin)).unwrap().writes.len(), 2);
}

#[test]
fn a_batch_compiles_each_types_schema_once_however_many_lines_use_it() {
    let scenario = board();
    let project = r#"{"$type": "lr/project/v1"}"#;
    let input = line("/tasks/a", None, &task(""))
        + &line("/tasks/b", None, &task(""))
        + &line("/tasks/c", None, &task(""))
        + &line("/projects/q", None, project)
        + &line("/projects/r", None, project);
    let board = open(&scenario);
    let lines = crate::Board::read_lines(&mut input.as_bytes()).unwrap();
    let lock = board.repository().lock(LOCK_WAIT).unwrap();
    let mut view = board.repositories().snapshot().unwrap();
    let mut schemas = Schemas::default();
    let (prepared, _) = board
        .check_lines(&lock, &mut view, lines, &mut schemas)
        .unwrap();
    assert_eq!(prepared.len(), 5);
    assert_eq!(schemas.compiled, 2);
}

/// How many directories a batch of `count` lines reads from disk while it
/// checks them, in a board whose `tasks` directory already holds 40 records.
/// Even lines overwrite a record and odd lines make a new one.
fn listed_checking(count: usize) -> usize {
    let scenario = board();
    for at in 0..40 {
        scenario.write(&format!("tasks/t{at}.json"), Item::File(&task("")));
    }
    let input: String = (0..count)
        .map(|at| {
            let name = if at % 2 == 0 { "t" } else { "new" };
            line(&format!("/tasks/{name}{at}"), None, &task(""))
        })
        .collect();
    let board = open(&scenario);
    let lines = crate::Board::read_lines(&mut input.as_bytes()).unwrap();
    let lock = board.repository().lock(LOCK_WAIT).unwrap();
    let before = board.repository().directories_listed();
    let mut view = board.repositories().snapshot().unwrap();
    let (prepared, _) = board
        .check_lines(&lock, &mut view, lines, &mut Schemas::default())
        .unwrap();
    assert_eq!(prepared.len(), count);
    board.repository().directories_listed() - before
}

#[test]
fn a_batch_lists_each_directory_once_however_many_lines_it_holds() {
    assert_eq!(listed_checking(2), 2, "the root and tasks, once each");
    assert_eq!(listed_checking(40), 2);
}

#[test]
fn a_batch_with_no_line_that_parses_is_refused_with_its_line_codes_while_the_lock_is_held() {
    let scenario = board();
    let board = open(&scenario);
    let _held = board.repository().lock(LOCK_WAIT).unwrap();
    let unparsed = "{\n".to_owned() + r#"{"path": "/tasks/a"}"# + "\n";
    assert_eq!(
        refused(board.put_batch(&mut unparsed.as_bytes())),
        (
            Code::PutInvalid,
            vec![(1, Code::JsonParse), (2, Code::PutInvalid)]
        )
    );
}

#[test]
fn a_batch_with_a_line_that_parses_is_refused_as_locked_while_the_lock_is_held() {
    let scenario = board();
    let board = open(&scenario);
    let _held = board.repository().lock(LOCK_WAIT).unwrap();
    let input = "{\n".to_owned() + &line("/tasks/a", None, &task(""));
    let refused = board.put_batch(&mut input.as_bytes()).unwrap_err();
    assert_eq!(refused.code(), Code::PutLocked);
}

#[test]
fn a_line_beneath_an_ignored_file_is_refused_before_any_line_is_written_and_its_twin_is_admitted() {
    let scenario = board();
    scenario
        .write(".gitignore", Item::File("x\n"))
        .write("x", Item::File(""));
    let input = line("/tasks/a", None, &task("")) + &line("/x/y", None, &task(""));
    let (_, refusals) = refused(batch(&scenario, &input));
    assert_eq!(refusals, [(2, Code::RepoUnreadable)]);
    assert!(
        file(&scenario, "tasks/a.json").is_none(),
        "nothing is written"
    );
    scenario.write("x", Item::Deleted);
    batch(&scenario, &input).unwrap();
    assert!(file(&scenario, "x/y.json").is_some());
}
