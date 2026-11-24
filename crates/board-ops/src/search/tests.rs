//! Searching a board's records with Board's search syntax.

use board_scenario::{Item, Scenario};
use board_search::{ErrorCode, Span, parse};

use super::{Cursor, Hit, SearchReport, Statement, compile};
use crate::Board;

/// A schema that admits any object.
const ANY: &str =
    r#"{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object"}"#;

/// The owner `from:me` names in these tests.
const OWNER: Option<&str> = Some("olive");

/// Twelve records of six types, two of them questions of different
/// versions.
const RECORDS: &[(&str, &str)] = &[
    (
        "tasks/budget-review.json",
        r#"{"$type": "tasks/task/v2", "title": "Review the Q3 budget", "status": "open"}"#,
    ),
    (
        "tasks/hiring-plan.json",
        r#"{"$type": "tasks/task/v2", "title": "Draft the hiring plan", "status": "open"}"#,
    ),
    (
        "plans/questions/q1.json",
        r#"{"$type": "plans/question/v1", "question": "Which budget line pays for the review?", "by": {"person": "erin"}}"#,
    ),
    (
        "plans/questions/q2.json",
        r#"{"$type": "plans/question/v2", "question": "Can the budget review wait a week?", "by": {"person": "Erin"}}"#,
    ),
    (
        "plans/questions/q3.json",
        r#"{"$type": "plans/question/v1", "question": "Who owns the hiring plan?", "by": {"person": "olive"}}"#,
    ),
    (
        "plans/decisions/d1.json",
        r#"{"$type": "plans/decision/v1", "decision": "Ops pays for the review", "why": "The budget has room.", "by": {"person": "olive"}}"#,
    ),
    (
        "plans/decisions/d2.json",
        r#"{"$type": "plans/decision/v1", "decision": "Hire two engineers", "by": {"agent": "hiring-planner", "program": "claude-code"}}"#,
    ),
    (
        "plans/answers/a1.json",
        r#"{"$type": "plans/answer/v1", "question": "/plans/questions/q1", "words": "The ops budget pays", "by": {"person": "olive"}}"#,
    ),
    (
        "plans/answers/a2.json",
        r#"{"$type": "plans/answer/v1", "question": "/plans/questions/q2", "words": "It can wait", "by": {"person": "erin"}}"#,
    ),
    (
        "changes/budget-report.json",
        r#"{"$type": "changes/change/v2", "title": "The report lists each line", "phase": "build"}"#,
    ),
    (
        "changes/passkey-login.json",
        r#"{"$type": "changes/change/v2", "title": "Sign in with a passkey", "phase": "plan"}"#,
    ),
    (
        "suites/gates/runs/r1.json",
        r#"{"$type": "runs/run/v1", "summary": "Every gate passed", "by": {"program": "codex"}}"#,
    ),
];

/// The ten examples of docs/commands/search.md, each with the paths it
/// returns from [`RECORDS`], in order, with `olive` as the owner.
const EXAMPLES: [(&str, &[&str]); 10] = [
    (
        "budget",
        &[
            "/plans/answers/a1",
            "/plans/questions/q1",
            "/plans/questions/q2",
            "/tasks/budget-review",
            "/plans/decisions/d1",
            "/changes/budget-report",
        ],
    ),
    (
        "\"the budget\"",
        &["/plans/questions/q2", "/plans/decisions/d1"],
    ),
    (
        "budget type:question",
        &["/plans/questions/q1", "/plans/questions/q2"],
    ),
    (
        "type:question from:erin",
        &["/plans/questions/q1", "/plans/questions/q2"],
    ),
    (
        "from:me",
        &[
            "/plans/answers/a1",
            "/plans/decisions/d1",
            "/plans/questions/q3",
        ],
    ),
    (
        "budget -type:task",
        &[
            "/plans/answers/a1",
            "/plans/questions/q1",
            "/plans/questions/q2",
            "/plans/decisions/d1",
            "/changes/budget-report",
        ],
    ),
    (
        "type:decision,answer budget",
        &["/plans/answers/a1", "/plans/decisions/d1"],
    ),
    (
        "hiring -from:me",
        &["/tasks/hiring-plan", "/plans/decisions/d2"],
    ),
    (
        "from:claude-code,codex",
        &["/plans/decisions/d2", "/suites/gates/runs/r1"],
    ),
    ("-budget type:change", &["/changes/passkey-login"]),
];

/// A board holding [`RECORDS`] and a schema for each of their types.
fn fixture() -> Scenario {
    let scenario = Scenario::board();
    for kind in [
        "tasks/task/v2",
        "plans/question/v1",
        "plans/question/v2",
        "plans/decision/v1",
        "plans/answer/v1",
        "changes/change/v2",
        "runs/run/v1",
    ] {
        scenario.write(&format!(".board/types/{kind}.schema.json"), Item::File(ANY));
    }
    for (path, record) in RECORDS {
        scenario.write(path, Item::File(record));
    }
    scenario
}

fn open(scenario: &Scenario) -> Board {
    crate::tests::open(scenario)
}

fn statement(text: &str) -> Statement {
    compile(&parse(text).unwrap(), OWNER).unwrap()
}

fn search(board: &Board, text: &str, limit: usize, after: Option<&Cursor>) -> SearchReport {
    board.search(&statement(text), limit, after, None).unwrap()
}

fn paths(results: &[Hit]) -> Vec<&str> {
    results.iter().map(|found| found.path.as_str()).collect()
}

#[test]
fn each_documented_example_returns_its_paths_in_order() {
    let scenario = fixture();
    let board = open(&scenario);
    for (text, wanted) in EXAMPLES {
        let report = search(&board, text, 50, None);
        assert_eq!(paths(&report.results), wanted, "{text}");
        assert_eq!(report.next, None, "{text}");
    }
}

#[test]
fn every_example_is_in_the_command_s_documentation() {
    let doc = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/commands/search.md"
    ));
    for (text, _) in EXAMPLES {
        let line = format!("board search '{text}'");
        assert!(doc.contains(&line), "{line}");
    }
}

#[test]
fn a_result_carries_its_type_and_title_and_a_question_s_title_is_its_question() {
    let scenario = fixture();
    let report = search(&open(&scenario), "budget type:question", 50, None);
    let first = &report.results[0];
    assert_eq!(first.kind.as_deref(), Some("plans/question/v1"));
    assert_eq!(
        first.title.as_deref(),
        Some("Which budget line pays for the review?")
    );
    assert_eq!((first.title_hits, first.body_hits), (1, 1));
    let second = &report.results[1];
    assert_eq!(second.kind.as_deref(), Some("plans/question/v2"));
}

#[test]
fn type_question_matches_a_version_one_and_a_version_two_question() {
    let scenario = fixture();
    let report = search(&open(&scenario), "type:question", 50, None);
    let kinds: Vec<Option<&str>> = (report.results.iter())
        .map(|found| found.kind.as_deref())
        .collect();
    assert!(kinds.contains(&Some("plans/question/v1")), "{kinds:?}");
    assert!(kinds.contains(&Some("plans/question/v2")), "{kinds:?}");
    assert_eq!(report.results.len(), 3);
}

#[test]
fn a_query_holding_a_quote_a_semicolon_and_two_dashes_matches_as_its_words_do() {
    let scenario = fixture();
    let board = open(&scenario);
    let plain = search(&board, "budget", 50, None);
    for text in [
        "budget -\"'; --\"",
        "budget -';",
        "budget -\"DROP TABLE records; --\"",
    ] {
        assert_eq!(search(&board, text, 50, None), plain, "{text}");
        let sql = statement(text).sql(None);
        for held in ["budget", "';", "--", ";", "DROP"] {
            assert!(!sql.contains(held), "{text}: {held} in {sql}");
        }
    }
    let quoted = search(&board, "\"o'brien\" budget", 50, None);
    assert_eq!(quoted.results, Vec::new());
}

#[test]
fn pages_after_a_cursor_join_to_the_whole_result_and_the_last_page_has_no_next() {
    let scenario = fixture();
    let board = open(&scenario);
    let whole = search(&board, "budget", 50, None).results;
    let mut joined = Vec::new();
    let mut after: Option<Cursor> = None;
    let mut pages = 0;
    loop {
        let page = search(&board, "budget", 2, after.as_ref());
        pages += 1;
        joined.extend(page.results);
        let Some(next) = page.next else {
            break;
        };
        let text = next.text();
        assert_eq!(Cursor::parse(&text), Some(next.clone()));
        after = Some(next);
    }
    assert_eq!(joined, whole);
    assert_eq!(pages, 3);
}

#[test]
fn a_cursor_that_board_did_not_print_is_not_read() {
    for text in ["", "1", "1.2", "a.1.2f", "1.1.2", "1.1.zz", "1.1.ff"] {
        assert_eq!(Cursor::parse(text), None, "{text}");
    }
    assert_eq!(
        Cursor::parse("1.0.2f61").map(|cursor| cursor.path),
        Some("/a".to_owned())
    );
}

#[test]
fn from_me_without_an_owner_is_an_error_at_its_value_and_nothing_runs() {
    let query = parse("budget from:erin,me").unwrap();
    let errors = compile(&query, None).unwrap_err();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, ErrorCode::UnknownPerson);
    assert_eq!(errors[0].span, Span::new(17, 19));
    assert!(compile(&query, OWNER).is_ok());
}

#[test]
fn an_unknown_type_is_an_error_before_any_statement() {
    let query = parse("type:questoin -from:me").unwrap();
    let errors = compile(&query, None).unwrap_err();
    let found: Vec<(ErrorCode, Span)> = errors
        .iter()
        .map(|error| (error.code, error.span))
        .collect();
    assert_eq!(
        found,
        [
            (ErrorCode::UnknownType, Span::new(5, 13)),
            (ErrorCode::UnknownPerson, Span::new(20, 22)),
        ]
    );
}

#[test]
fn an_empty_query_matches_every_record() {
    let scenario = fixture();
    let report = search(&open(&scenario), "", 50, None);
    let found = paths(&report.results);
    for (path, _) in RECORDS {
        let logical = format!("/{}", path.trim_end_matches(".json"));
        assert!(found.contains(&logical.as_str()), "{logical}");
    }
}

#[test]
fn a_word_matches_in_any_ascii_case_and_by_substring() {
    let scenario = fixture();
    let board = open(&scenario);
    assert_eq!(
        paths(&search(&board, "PASSKEY", 50, None).results),
        ["/changes/passkey-login"]
    );
    assert_eq!(
        paths(&search(&board, "passk", 50, None).results),
        ["/changes/passkey-login"]
    );
}
