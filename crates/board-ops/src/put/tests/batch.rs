//! `put --batch`: every line is checked and every expect compared before
//! anything is written, and the writes share one commit.

use board_model::Code;

use super::{board, canonical, committing, file};
use crate::put::{MAX_BATCH_LINES, PutError, PutReport, Refusal};
use crate::tests::{open, task};

fn line(path: &str, expect: Option<&str>, record: &str) -> String {
    let expect = expect.map_or_else(String::new, |expect| format!(r#""expect": "{expect}", "#));
    format!(r#"{{"path": "{path}", {expect}"record": {record}}}"#) + "\n"
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
                .map(|Refusal { line, code, .. }| (*line, *code))
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
fn two_lines_that_write_one_file_are_refused_and_two_files_are_admitted() {
    let scenario = board();
    scenario.write(
        "links/a.json",
        board_scenario::Item::File(r#"{"$type": "board/mount/v1", "path": "/tasks/a"}"#),
    );
    scenario.write("tasks/a.json", board_scenario::Item::File(&task("")));
    let same = line("/tasks/a", None, &task("")) + &line("/links/a", None, &task(""));
    let refusal = batch(&scenario, &same).unwrap_err();
    assert_eq!(refusal.code(), Code::PutInvalid);
    assert!(refusal.to_string().contains("line 2"), "{refusal}");
    assert!(refusal.to_string().contains("as line 1 does"), "{refusal}");
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
