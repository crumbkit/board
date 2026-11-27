//! Searching a board's records with Board's search syntax.

use std::time::{Duration, SystemTime};

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

/// The time every record of [`RECORDS`] was last modified, so results of
/// equal rank among them come by path.
fn written() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

/// A time `seconds` after [`written`].
fn later(seconds: u64) -> SystemTime {
    written() + Duration::from_secs(seconds)
}

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
        scenario.write(path, Item::Dated(record, written()));
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
    for text in [
        "", "1", "1.2", "1.0.2f61", "a.1.5.2f", "1.1.5.2", "1.1.x.2f", "1.1.5.zz", "1.1.5.ff",
    ] {
        assert_eq!(Cursor::parse(text), None, "{text}");
    }
    assert_eq!(
        Cursor::parse("1.0.5.2f61").map(|cursor| (cursor.modified_ns, cursor.path)),
        Some((5, "/a".to_owned()))
    );
    assert_eq!(
        Cursor::parse("1.0.-9.2f61").map(|cursor| cursor.modified_ns),
        Some(-9)
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

#[test]
fn a_percent_sign_an_underscore_and_a_backslash_match_only_themselves() {
    let scenario = fixture();
    scenario.write(
        "tasks/discount.json",
        Item::File(r#"{"$type": "tasks/task/v2", "title": "Cut 50% off the_plan at C:\\ops", "status": "open"}"#),
    );
    let board = open(&scenario);
    for text in ["50%", "the_plan", "\"c:\\ops\""] {
        assert_eq!(
            paths(&search(&board, text, 50, None).results),
            ["/tasks/discount"],
            "{text}"
        );
    }
    for text in ["%", "_", "b_dget", "5%off"] {
        let report = search(&board, text, 50, None);
        let found = paths(&report.results);
        assert!(
            !found.contains(&"/tasks/budget-review"),
            "{text}: {found:?}"
        );
    }
}

#[test]
fn a_word_with_a_quote_matches_a_title_whose_json_escapes_the_quote() {
    let scenario = fixture();
    scenario.write(
        "tasks/quoted.json",
        Item::File(
            r#"{"$type": "tasks/task/v2", "title": "Read \"Dune\" twice", "status": "open"}"#,
        ),
    );
    let board = open(&scenario);
    let report = search(&board, r#"dune""#, 50, None);
    assert_eq!(paths(&report.results), ["/tasks/quoted"]);
    assert_eq!(report.results[0].title_hits, 1);
}

#[test]
fn a_filter_s_condition_comes_before_any_word_s_in_the_statement() {
    let sql = statement("budget type:task").sql(None);
    let condition = &sql[sql.find("FROM c WHERE ").unwrap()..];
    let filter = condition.find("substr(k, 1,").unwrap();
    let word = condition.find(" LIKE ").unwrap();
    assert!(filter < word, "{sql}");
}

#[test]
fn a_word_reads_an_untyped_entry_s_heading_and_not_its_file_name_which_its_path_holds() {
    let sql = statement("budget").sql(None);
    let condition = &sql[sql.find("FROM c WHERE ").unwrap()..];
    assert!(condition.contains("(k = '' AND (hd LIKE "), "{sql}");
    assert!(!condition.contains("(t LIKE "), "{sql}");
}

#[test]
fn each_word_queries_the_trigram_index_once_however_often_the_statement_reads_it() {
    let scenario = documents();
    let sql = statement("the plan board -kettle").sql(None);
    let plan = open(&scenario)
        .query(&format!("EXPLAIN QUERY PLAN {sql}"), 1000, None, None)
        .unwrap();
    let scans = (plan.rows.iter().flatten())
        .filter(|cell| cell.as_str().is_some_and(|text| text.contains("texts_fts")))
        .count();
    assert_eq!(scans, 4, "{plan:?}");
}

#[test]
fn an_entry_looks_up_only_the_rows_its_kind_role_and_source_can_have() {
    let sql = statement("budget").sql(None);
    for key in [
        "o.blob = CASE WHEN e.role IS NOT NULL THEN e.blob END",
        "b.blob = CASE WHEN e.kind = 'record' THEN e.blob END",
        "w.path = CASE WHEN e.source = 'worktree' THEN e.physical END",
    ] {
        assert!(sql.contains(key), "{key}: {sql}");
    }
}

/// The fixture with a Markdown note, a Markdown task, an untyped JSON file
/// and an image, each written `later` than the records.
fn documents() -> Scenario {
    let scenario = fixture();
    scenario
        .write(
            "notes/offsite.md",
            Item::Dated(
                "# Offsite *agenda* review\n\nWe walk the riverbank before lunch.\n",
                later(10),
            ),
        )
        .write(
            "notes/plain.md",
            Item::Dated("No heading here, only a lighthouse.\n", later(20)),
        )
        .write(
            "tasks/kettle.md",
            Item::Dated(
                "---\n$type: tasks/task/v2\ntitle: Fix the kitchen\nstatus: open\n---\nDescale the kettle first.\n",
                later(30),
            ),
        )
        .write(
            "data/settings.json",
            Item::Dated(r#"{"palette": "aubergine"}"#, later(40)),
        )
        .write("images/aubergine.png", Item::Bytes(b"\x89PNG\r\n\x1a\n\xffaubergine"));
    scenario
}

#[test]
fn a_markdown_file_is_found_by_its_heading_and_its_prose_and_titled_by_its_heading() {
    let scenario = documents();
    let board = open(&scenario);
    for word in ["agenda", "riverbank"] {
        let report = search(&board, word, 50, None);
        assert_eq!(paths(&report.results), ["/notes/offsite.md"], "{word}");
        let found = &report.results[0];
        assert_eq!(found.kind, None);
        assert_eq!(found.title.as_deref(), Some("Offsite agenda review"));
    }
    let phrase = search(&board, "\"offsite agenda\"", 50, None);
    assert_eq!(paths(&phrase.results), ["/notes/offsite.md"]);
    assert_eq!(phrase.results[0].title_hits, 1);
}

#[test]
fn a_file_with_no_heading_is_titled_by_its_file_name() {
    let scenario = documents();
    let report = search(&open(&scenario), "lighthouse", 50, None);
    assert_eq!(paths(&report.results), ["/notes/plain.md"]);
    assert_eq!(report.results[0].title.as_deref(), Some("plain.md"));
}

#[test]
fn a_markdown_record_is_found_by_a_word_only_its_prose_holds() {
    let scenario = documents();
    let report = search(&open(&scenario), "descale", 50, None);
    assert_eq!(paths(&report.results), ["/tasks/kettle.md"]);
    let found = &report.results[0];
    assert_eq!(found.kind.as_deref(), Some("tasks/task/v2"));
    assert_eq!(found.title.as_deref(), Some("Fix the kitchen"));
    assert_eq!((found.title_hits, found.body_hits), (0, 1));
}

#[test]
fn an_untyped_json_file_is_found_by_its_text_and_a_binary_file_only_by_its_path() {
    let scenario = documents();
    let board = open(&scenario);
    assert_eq!(
        paths(&search(&board, "palette", 50, None).results),
        ["/data/settings"]
    );
    let both = search(&board, "aubergine", 50, None);
    assert_eq!(
        paths(&both.results),
        ["/images/aubergine.png", "/data/settings"]
    );
    assert_eq!(both.results[0].title.as_deref(), Some("aubergine.png"));
    assert_eq!(
        paths(&search(&board, "PNG", 50, None).results),
        ["/images/aubergine.png"]
    );
    assert_eq!(
        paths(&search(&board, "IHDR", 50, None).results),
        Vec::<&str>::new()
    );
}

#[test]
fn type_doc_matches_only_entries_with_no_type_and_minus_type_doc_leaves_them_out() {
    let scenario = documents();
    let board = open(&scenario);
    let docs = search(&board, "type:doc", 1000, None);
    assert!(docs.results.iter().all(|found| found.kind.is_none()));
    let found = paths(&docs.results);
    for path in ["/notes/offsite.md", "/notes/plain.md", "/data/settings"] {
        assert!(found.contains(&path), "{path}: {found:?}");
    }
    assert!(!found.contains(&"/tasks/kettle.md"), "{found:?}");
    let typed = search(&board, "-type:doc", 1000, None);
    assert_ne!(typed.results.len(), 0);
    assert!(typed.results.iter().all(|found| found.kind.is_some()));
    let either = search(&board, "type:doc,task kettle", 50, None);
    assert_eq!(paths(&either.results), ["/tasks/kettle.md"]);
}

#[test]
fn a_path_filter_matches_by_prefix_and_a_path_without_a_leading_slash_is_refused() {
    let scenario = documents();
    let board = open(&scenario);
    let notes = search(&board, "path:/notes", 50, None);
    assert_eq!(
        paths(&notes.results),
        ["/notes/plain.md", "/notes/offsite.md"]
    );
    let budget = search(&board, "budget path:/plans/q,/tasks", 50, None);
    assert_eq!(
        paths(&budget.results),
        [
            "/plans/questions/q1",
            "/plans/questions/q2",
            "/tasks/budget-review"
        ]
    );
    let outside = search(&board, "budget -path:/plans", 50, None);
    assert_eq!(
        paths(&outside.results),
        ["/tasks/budget-review", "/changes/budget-report"]
    );
    let errors = compile(&parse("path:notes").unwrap(), OWNER).unwrap_err();
    let found: Vec<(ErrorCode, Span)> = errors
        .iter()
        .map(|error| (error.code, error.span))
        .collect();
    assert_eq!(found, [(ErrorCode::RelativePath, Span::new(5, 10))]);
    assert!(compile(&parse("path:/notes").unwrap(), OWNER).is_ok());
}

#[test]
fn results_of_equal_rank_and_a_query_of_filters_alone_come_newest_first() {
    let scenario = fixture();
    scenario
        .write(
            "tasks/old-budget.json",
            Item::Dated(
                r#"{"$type": "tasks/task/v2", "title": "Budget ledger", "status": "open"}"#,
                later(5),
            ),
        )
        .write(
            "tasks/new-budget.json",
            Item::Dated(
                r#"{"$type": "tasks/task/v2", "title": "Budget ledger", "status": "open"}"#,
                later(50),
            ),
        )
        .write(
            "notes/budget.md",
            Item::Dated("# Budget ledger\n", later(30)),
        );
    let board = open(&scenario);
    let ranked = search(&board, "\"budget ledger\"", 50, None);
    assert_eq!(
        paths(&ranked.results),
        ["/tasks/new-budget", "/notes/budget.md", "/tasks/old-budget"]
    );
    let filtered = search(&board, "type:task", 50, None);
    assert_eq!(
        paths(&filtered.results),
        [
            "/tasks/new-budget",
            "/tasks/old-budget",
            "/tasks/budget-review",
            "/tasks/hiring-plan"
        ]
    );
    let mut joined = Vec::new();
    let mut after: Option<Cursor> = None;
    loop {
        let page = search(&board, "type:task", 1, after.as_ref());
        joined.extend(page.results);
        let Some(next) = page.next else {
            break;
        };
        assert_eq!(Cursor::parse(&next.text()), Some(next.clone()));
        after = Some(next);
    }
    assert_eq!(joined, filtered.results);
}

#[test]
fn a_cache_holding_an_index_of_the_previous_format_builds_one_of_this_format_and_searches_it() {
    let scenario = documents();
    let cache = Scenario::new();
    let root = std::fs::canonicalize(scenario.workdir()).unwrap();
    let file = board_index::location(cache.workdir(), &root);
    let previous = file.with_file_name(format!("index-{}.sqlite", board_index::INDEX_VERSION - 1));
    let inside = previous.strip_prefix(cache.workdir()).unwrap();
    cache.write(
        inside.to_str().unwrap(),
        Item::Bytes(b"an index of the previous format"),
    );
    let board = open(&scenario);
    let found = board.search(&statement("riverbank"), 50, None, Some(cache.workdir()));
    assert_eq!(paths(&found.unwrap().results), ["/notes/offsite.md"]);
    assert!(file.exists(), "{}", file.display());
    assert!(previous.exists(), "{}", previous.display());
}

#[test]
fn each_documented_example_answers_the_same_warm_as_cold() {
    let scenario = documents();
    for (text, _) in EXAMPLES {
        let cache = tempfile::tempdir().unwrap();
        let cold = open(&scenario)
            .search(&statement(text), 50, None, Some(cache.path()))
            .unwrap();
        let warm = open(&scenario)
            .search(&statement(text), 50, None, Some(cache.path()))
            .unwrap();
        assert_eq!(warm, cold, "{text}");
        assert_eq!(cold.memory, None, "{text}");
    }
}

/// The logical paths, in order, of the entries under `/d` whose text holds
/// `word` by a `LIKE` scan of the `texts` view.
fn scanned(board: &Board, word: &str) -> Vec<String> {
    let sql = format!(
        "SELECT e.logical FROM entries AS e JOIN texts AS x ON x.blob = e.blob \
         WHERE e.logical LIKE '/d/%' AND x.body LIKE '%{word}%' ORDER BY e.logical"
    );
    let rows = board.query(&sql, 1000, None, None).unwrap().rows;
    rows.iter()
        .map(|row| row[0].as_str().unwrap().to_owned())
        .collect()
}

/// The logical paths, in order, that `board search` finds for `word` among
/// the files under `/d`.
fn searched(board: &Board, word: &str) -> Vec<String> {
    let report = search(board, &format!("path:/d {word}"), 1000, None);
    let mut found: Vec<String> = report.results.into_iter().map(|hit| hit.path).collect();
    found.sort();
    found
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(24))]

    /// The alphabet holds no character of the paths `/d/n0.md` to
    /// `/d/n3.md` or their file names, so a word matches only by its text.
    #[test]
    fn a_word_of_three_or_more_characters_finds_the_texts_a_like_scan_finds(
        texts in proptest::collection::vec("[abcABC ]{0,40}", 1..5),
        words in proptest::collection::vec("[abcABC]{3,5}", 1..5),
    ) {
        let scenario = Scenario::board();
        for (at, text) in texts.iter().enumerate() {
            scenario.write(&format!("d/n{at}.md"), Item::File(text));
        }
        let board = open(&scenario);
        for word in &words {
            proptest::prop_assert_eq!(searched(&board, word), scanned(&board, word), "{}", word);
        }
    }
}

#[test]
fn a_non_ascii_word_finds_a_text_holding_it_in_another_case_and_an_ascii_word_only_texts_holding_it()
 {
    let scenario = Scenario::board();
    scenario
        .write("d/summer.md", Item::File("Notes from the ÉTÉ offsite.\n"))
        .write("d/plain.md", Item::File("Notes from the ete offsite.\n"));
    let board = open(&scenario);
    assert_eq!(searched(&board, "été"), ["/d/summer.md"]);
    assert_eq!(searched(&board, "ÉTÉ"), ["/d/summer.md"]);
    assert_eq!(searched(&board, "ete"), ["/d/plain.md"]);
    assert_eq!(searched(&board, "ETE"), ["/d/plain.md"]);
}

#[test]
fn a_word_of_one_or_two_characters_finds_its_texts_by_scan() {
    let scenario = Scenario::board();
    scenario
        .write("d/one.md", Item::File("a rare aQz pair\n"))
        .write("d/two.md", Item::File("no such pair\n"));
    let board = open(&scenario);
    assert_eq!(searched(&board, "qz"), ["/d/one.md"]);
    assert_eq!(searched(&board, "QZ"), ["/d/one.md"]);
    assert_eq!(searched(&board, "Q"), ["/d/one.md"]);
}

/// Whether the program `SQLite` compiles for the search `text` reads the
/// `body` column of `main.texts`: an `OpenRead` of the table's root page in
/// `main` names a cursor, and a `Column` of that cursor at the body's
/// position reads it.
fn reads_prose(board: &Board, text: &str, cache: &std::path::Path) -> bool {
    let root = board
        .query(
            "SELECT rootpage FROM main.sqlite_schema WHERE name = 'texts'",
            1,
            None,
            Some(cache),
        )
        .unwrap()
        .rows[0][0]
        .as_i64()
        .unwrap();
    let columns = board
        .query(
            "SELECT cid FROM pragma_table_info('texts', 'main') WHERE name = 'body'",
            1,
            None,
            Some(cache),
        )
        .unwrap()
        .rows[0][0]
        .as_i64()
        .unwrap();
    let sql = format!("EXPLAIN {}", statement(text).sql(None));
    let program = board.query(&sql, 100_000, None, Some(cache)).unwrap().rows;
    let field = |row: &[serde_json::Value], name: usize| row[name].as_i64();
    let cursors: Vec<i64> = program
        .iter()
        .filter(|row| {
            row[1] == "OpenRead" && field(row, 3) == Some(root) && field(row, 4) == Some(0)
        })
        .filter_map(|row| field(row, 2))
        .collect();
    program.iter().any(|row| {
        row[1] == "Column"
            && field(row, 2).is_some_and(|cursor| cursors.contains(&cursor))
            && field(row, 3) == Some(columns)
    })
}

#[test]
fn a_warm_search_for_words_of_three_or_more_characters_reads_no_prose_and_one_for_a_shorter_word_does()
 {
    let scenario = documents();
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    board
        .search(&statement("riverbank"), 50, None, Some(cache.path()))
        .unwrap();
    for text in [
        "riverbank",
        "the plan board",
        "type:task the plan",
        "\"offsite agenda\"",
        "-lunch",
    ] {
        assert!(!reads_prose(&board, text, cache.path()), "{text}");
    }
    for text in ["ri", "riverbank ri"] {
        assert!(reads_prose(&board, text, cache.path()), "{text}");
    }
}
