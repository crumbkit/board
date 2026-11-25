//! `board search`: the records that match a query in the search syntax
//! Board shares with Flare.

use std::path::Path;
use std::time::{Duration, SystemTime};

use board_model::{Code, Fields, Refusal};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::{Context, Ran, context_in, run, text};
use crate::report::Output;
use crate::run::{TIMEOUT_HINT, unanswered};

/// A schema that admits any object.
const ANY: &str = r#"{"type": "object"}"#;

/// Board's copy of the case file it shares with Flare.
const CASES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../board-search/cases/search-cases.json"
));

/// When every record of [`fixture`] was last modified, so results of equal
/// rank come by path.
fn written() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

/// A board with two questions of two versions, a task and a decision.
fn fixture() -> Scenario {
    let scenario = Scenario::board();
    for kind in [
        "plans/question/v1",
        "plans/question/v2",
        "tasks/task/v2",
        "plans/decision/v1",
    ] {
        scenario.write(&format!(".board/types/{kind}.schema.json"), Item::File(ANY));
    }
    scenario
        .write(
            "plans/q1.json",
            Item::Dated(
                r#"{"$type": "plans/question/v1", "question": "Which budget line pays?", "by": {"person": "erin"}}"#,
                written(),
            ),
        )
        .write(
            "plans/q2.json",
            Item::Dated(
                r#"{"$type": "plans/question/v2", "question": "Can the budget wait?", "by": {"person": "olive"}}"#,
                written(),
            ),
        )
        .write(
            "tasks/budget.json",
            Item::Dated(
                r#"{"$type": "tasks/task/v2", "title": "Review the budget", "status": "open"}"#,
                written(),
            ),
        )
        .write(
            "plans/d1.json",
            Item::Dated(
                r#"{"$type": "plans/decision/v1", "decision": "Hire two engineers", "by": {"agent": "planner"}}"#,
                written(),
            ),
        );
    scenario
}

/// Runs `board` in `directory` with the known boards file holding `known`,
/// or with none.
fn board(directory: &Path, known: Option<&Value>, arguments: &[&str]) -> Output {
    let config = Scenario::new();
    let boards = known.map(|known| {
        config.write("config.json", Item::File(&known.to_string()));
        config.workdir().join("config.json")
    });
    let context = Context {
        boards,
        ..context_in(directory)
    };
    let arguments = std::iter::once("board")
        .chain(arguments.iter().copied())
        .map(std::ffi::OsString::from);
    run(arguments, &context, &mut &b""[..], &mut Ran::default())
}

#[test]
fn search_prints_each_match_with_its_path_short_type_and_title() {
    let scenario = fixture();
    let output = board(
        scenario.workdir(),
        None,
        &["search", "budget type:question"],
    );
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "/plans/q1  question  Which budget line pays?\n/plans/q2  question  Can the budget wait?\n"
    );
}

#[test]
fn search_json_holds_the_canonical_query_its_parts_the_results_and_no_next_on_the_last_page() {
    let scenario = fixture();
    let output = board(
        scenario.workdir(),
        None,
        &["--json", "search", "  budget   -TYPE:task"],
    );
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let report = super::json(&output);
    assert_eq!(report["schema"], "board/search-report/v1");
    let data = &report["data"];
    assert_eq!(data["query"], "budget -type:task");
    assert_eq!(
        data["parsed"],
        json!([
            {"kind": "word", "text": "budget", "negated": false, "span": {"start": 2, "end": 8}},
            {
                "kind": "filter",
                "field": "type",
                "values": [{"text": "task", "span": {"start": 17, "end": 21}}],
                "negated": true,
                "span": {"start": 11, "end": 21}
            }
        ])
    );
    assert_eq!(
        data["results"],
        json!([
            {"path": "/plans/q1", "type": "plans/question/v1", "title": "Which budget line pays?"},
            {"path": "/plans/q2", "type": "plans/question/v2", "title": "Can the budget wait?"}
        ])
    );
    assert_eq!(data["next"], Value::Null);
}

#[test]
fn a_page_cut_at_its_limit_names_the_next_cursor_and_after_it_the_rest_follow() {
    let scenario = fixture();
    let first = board(
        scenario.workdir(),
        None,
        &["--json", "search", "budget", "--limit", "2"],
    );
    let data = &super::json(&first)["data"];
    let paths = |data: &Value| -> Vec<String> {
        (data["results"].as_array().unwrap().iter())
            .map(|hit| hit["path"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(paths(data), ["/plans/q1", "/plans/q2"]);
    let next = data["next"].as_str().unwrap();
    let second = board(
        scenario.workdir(),
        None,
        &[
            "--json", "search", "budget", "--limit", "2", "--after", next,
        ],
    );
    let data = &super::json(&second)["data"];
    assert_eq!(paths(data), ["/tasks/budget"]);
    assert_eq!(data["next"], Value::Null);
    let text_page = board(
        scenario.workdir(),
        None,
        &["search", "budget", "--limit", "2"],
    );
    let shown = text(&text_page.stdout);
    let wanted = format!("next page: board search 'budget' --after {next} --limit 2\n");
    assert!(shown.ends_with(&wanted), "{shown}");
}

#[test]
fn the_next_page_command_leaves_out_the_default_limit() {
    let scenario = fixture();
    for index in 0..60 {
        scenario.write(
            &format!("tasks/budget-{index:02}.json"),
            Item::File(r#"{"$type": "tasks/task/v2", "title": "Budget line", "status": "open"}"#),
        );
    }
    let output = board(scenario.workdir(), None, &["search", "budget"]);
    let shown = text(&output.stdout);
    let last = shown.lines().last().unwrap();
    assert!(
        last.starts_with("next page: board search 'budget' --after "),
        "{shown}"
    );
    assert!(!last.contains("--limit"), "{last}");
}

#[test]
fn a_search_past_the_time_limit_says_to_narrow_the_search_and_not_its_sql() {
    let timed_out = Refusal::new(
        Code::QueryTimeout,
        "the query ran longer than 1s",
        &Fields::default(),
    );
    assert!(timed_out.hint.as_deref().unwrap().contains("WHERE"));
    let refusal = unanswered(timed_out);
    let hint = refusal.hint.as_deref().unwrap();
    assert_eq!(hint, TIMEOUT_HINT);
    assert!(hint.contains("type:task"), "{hint}");
    assert!(!hint.contains("WHERE"), "{hint}");
    assert_eq!(refusal.message, "the query ran longer than 1s");
    let other = Refusal::new(Code::QueryInvalid, "near x", &Fields::default());
    assert_eq!(unanswered(other.clone()), other);
}

#[test]
fn a_cursor_board_did_not_print_is_refused_as_usage() {
    let scenario = fixture();
    let output = board(
        scenario.workdir(),
        None,
        &["search", "budget", "--after", "x"],
    );
    assert_eq!(output.code, 2, "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("--after x is not a cursor"));
}

#[test]
fn every_invalid_case_of_the_shared_file_is_refused_with_its_codes_and_spans_and_exit_two() {
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let directory = Scenario::new();
    let mut checked = 0;
    for case in cases["cases"].as_array().unwrap() {
        let Some(errors) = case.get("errors") else {
            continue;
        };
        let input = case["input"].as_str().unwrap();
        let output = board(directory.workdir(), None, &["--json", "search", input]);
        assert_eq!(output.code, 2, "{input}");
        let failure: Value = serde_json::from_slice(&output.stdout)
            .or_else(|_| serde_json::from_slice(&output.stderr))
            .unwrap();
        let fields = &failure["failures"][0]["fields"];
        let fields = if fields.is_null() {
            &failure["fields"]
        } else {
            fields
        };
        assert_eq!(fields["query"], input, "{failure}");
        let found: Vec<Value> = (fields["errors"].as_array().unwrap().iter())
            .map(|error| json!({"code": error["code"], "span": error["span"]}))
            .collect();
        assert_eq!(&Value::Array(found), errors, "{input}");
        checked += 1;
    }
    assert!(checked >= 15, "{checked}");
}

#[test]
fn an_unknown_filter_is_refused_before_a_board_is_read_naming_its_span_and_a_fix() {
    let directory = Scenario::new();
    let output = board(directory.workdir(), None, &["search", "form:erin budget"]);
    assert_eq!(output.code, 2);
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("There is no filter called form; did you mean from? (characters 0 to 4)"),
        "{stderr}"
    );
    assert!(stderr.contains("board::cli::usage"), "{stderr}");
    assert!(!stderr.contains("board::config"), "{stderr}");
}

#[test]
fn a_query_that_starts_with_a_minus_is_a_query_and_from_me_reads_the_owner() {
    let scenario = fixture();
    let owner = json!({"owner": "olive"});
    let output = board(
        scenario.workdir(),
        Some(&owner),
        &["search", "-from:me", "--limit", "10"],
    );
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let shown = text(&output.stdout);
    assert!(!shown.contains("/plans/q2"), "{shown}");
    assert!(shown.contains("/plans/q1"), "{shown}");
    let mine = board(scenario.workdir(), Some(&owner), &["search", "from:me"]);
    assert_eq!(
        text(&mine.stdout),
        "/plans/q2  question  Can the budget wait?\n"
    );
}

#[test]
fn from_me_without_an_owner_is_refused_naming_where_to_set_one() {
    let scenario = fixture();
    let output = board(scenario.workdir(), None, &["--json", "search", "from:me"]);
    assert_eq!(output.code, 2);
    let failure: Value = serde_json::from_slice(&output.stdout)
        .or_else(|_| serde_json::from_slice(&output.stderr))
        .unwrap();
    let rendered = failure.to_string();
    assert!(rendered.contains("unknown_person"), "{rendered}");
    assert!(rendered.contains("\\\"owner\\\""), "{rendered}");
}

#[test]
fn an_unknown_short_type_is_refused_listing_the_short_names() {
    let scenario = fixture();
    let output = board(scenario.workdir(), None, &["search", "type:questoin"]);
    assert_eq!(output.code, 2);
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("There is no type called questoin; use one of task, question"),
        "{stderr}"
    );
}

#[test]
fn filters_lists_each_filter_and_each_short_type_with_its_prefix_without_a_board() {
    let directory = Scenario::new();
    let output = board(
        directory.workdir(),
        None,
        &["--json", "search", "--filters"],
    );
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let data = &super::json(&output)["data"];
    assert_eq!(data["fields"][0]["name"], "type");
    assert_eq!(data["fields"][0]["kind"], "keyword");
    assert_eq!(data["fields"][0]["values"][1], "question");
    assert_eq!(data["fields"][1]["name"], "from");
    assert_eq!(data["fields"][1]["kind"], "person");
    assert_eq!(data["fields"][1]["values"], json!(["me"]));
    assert_eq!(data["fields"][2]["name"], "path");
    assert_eq!(data["fields"][2]["kind"], "path");
    assert_eq!(
        data["types"][1],
        json!({"name": "question", "prefix": "plans/question/"})
    );
    assert_eq!(data["types"][10], json!({"name": "doc", "prefix": null}));
    assert_eq!(data["types"].as_array().unwrap().len(), 11);
    let shown = board(directory.workdir(), None, &["search", "--filters"]);
    let shown = text(&shown.stdout);
    assert!(shown.contains("  question  plans/question/*\n"), "{shown}");
    assert!(shown.contains("  doc       no $type\n"), "{shown}");
}

#[test]
fn a_search_that_matches_nothing_says_so_and_exits_zero() {
    let scenario = fixture();
    let output = board(scenario.workdir(), None, &["search", "\"no such words\""]);
    assert_eq!(output.code, 0);
    assert_eq!(text(&output.stdout), "no record matches\n");
}

#[test]
fn an_untyped_file_prints_doc_as_its_type_and_its_heading_as_its_title_and_json_gives_null() {
    let scenario = fixture();
    scenario.write(
        "notes/offsite.md",
        Item::File("# Offsite budget review\n\nWe walk the riverbank.\n"),
    );
    let output = board(scenario.workdir(), None, &["search", "riverbank"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "/notes/offsite.md  doc  Offsite budget review\n"
    );
    let json = board(scenario.workdir(), None, &["--json", "search", "riverbank"]);
    assert_eq!(
        super::json(&json)["data"]["results"],
        json!([{"path": "/notes/offsite.md", "type": null, "title": "Offsite budget review"}])
    );
}

#[test]
fn a_path_without_a_leading_slash_is_refused_and_one_with_it_runs() {
    let scenario = fixture();
    let output = board(
        scenario.workdir(),
        None,
        &["--json", "search", "path:plans"],
    );
    assert_eq!(output.code, 2);
    let rendered = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(rendered.contains("relative_path"), "{rendered}");
    let admitted = board(scenario.workdir(), None, &["search", "path:/plans budget"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    assert_eq!(
        text(&admitted.stdout),
        "/plans/q1  question  Which budget line pays?\n/plans/q2  question  Can the budget wait?\n"
    );
}
