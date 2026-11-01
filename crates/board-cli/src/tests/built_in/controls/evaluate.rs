//! `controls.evaluate` through the binary: it checks named parts of text
//! against budget, pattern and judged rules.

use super::*;

/// An expression whose text differs from anything it matches, so a quote
/// of a match never reveals it.
const PRIVATE: &str = r"acme-\d{3}";

/// The run of `board --json call controls.evaluate` with `controls` and
/// `parts` on its standard input, and its one document.
fn evaluate(board: &Fixture, controls: &[&str], parts: &Value) -> (Output, Value) {
    let stdin = json!({"controls": controls, "parts": parts}).to_string();
    let ran = board.run_with(&["--json", "call", "controls.evaluate"], &stdin);
    let document = json(&ran);
    (ran, document)
}

/// Everything `ran` printed.
fn printed(ran: &Output) -> String {
    text(&ran.stdout) + &text(&ran.stderr)
}

/// The capability's output in a `--json` report.
fn output(document: &Value) -> &Value {
    assert!(document.get("data").is_some(), "{document}");
    &document["data"]["output"]
}

/// A finding as the output gives it, with every key present.
fn finding(text: &str, line: Option<u64>, pattern: Option<&str>) -> Value {
    json!({"text": text, "line": line, "pattern": pattern, "rewrite": null})
}

/// The results of a call that checked one control, by part.
fn by_part(document: &Value) -> BTreeMap<String, Value> {
    (output(document)["results"].as_array().unwrap().iter())
        .map(|result| (result["part"].as_str().unwrap().to_owned(), result.clone()))
        .collect()
}

/// The controls each result of `document` names, in order.
fn checked(document: &Value) -> Vec<Value> {
    (output(document)["results"].as_array().unwrap().iter())
        .map(|result| result["control"].clone())
        .collect()
}

/// A control whose rule is the pattern file `/private/names.txt` shown as
/// `show`.
fn names(show: &str) -> String {
    control(
        "No names.",
        &json!({"rule": {"pattern": "/private/names.txt", "show": show}}),
    )
}

#[test]
fn a_part_over_a_budget_fails_and_one_within_it_passes() {
    let board = board();
    board.write(
        "controls/short.json",
        &control("Short.", &json!({"rule": {"budget": "words", "max": 3}})),
    );
    let (ran, document) = evaluate(
        &board,
        &["/controls/short"],
        &json!({"pr.title": "one two three", "pr.body": "one two three four"}),
    );
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    assert_eq!(output(&document)["passed"], false);
    let results = by_part(&document);
    assert_eq!(
        results["pr.title"],
        json!({
            "control": "/controls/short",
            "part": "pr.title",
            "rule": "budget",
            "status": "passed",
            "findings": [],
            "used": 3,
            "max": 3,
            "omitted": null,
        })
    );
    assert_eq!(results["pr.body"]["status"], "failed");
    assert_eq!(
        results["pr.body"]["findings"],
        json!([finding("4 words, more than the 3 allowed", None, None)])
    );
}

#[test]
fn a_bytes_budget_counts_the_bytes_of_the_text() {
    let board = board();
    board.write(
        "controls/small.json",
        &control("Small.", &json!({"rule": {"budget": "bytes", "max": 4}})),
    );
    let (_, document) = evaluate(
        &board,
        &["/controls/small"],
        &json!({"over": "héllo", "within": "héh"}),
    );
    let results = by_part(&document);
    assert_eq!(
        (&results["over"]["status"], &results["over"]["used"]),
        (&json!("failed"), &json!(6))
    );
    assert_eq!(
        results["over"]["findings"],
        json!([finding("6 bytes, more than the 4 allowed", None, None)])
    );
    assert_eq!(
        (&results["within"]["status"], &results["within"]["used"]),
        (&json!("passed"), &json!(4))
    );
}

#[test]
fn a_control_named_twice_is_checked_once_and_two_controls_twice() {
    let board = board();
    board
        .write(
            "controls/short.json",
            &control("Short.", &json!({"rule": {"budget": "words", "max": 3}})),
        )
        .write("controls/kind.json", &control("Be kind.", &json!({})));
    let parts = json!({"pr.body": "Text."});
    let (_, document) = evaluate(&board, &["/controls/short", "/controls/short"], &parts);
    assert_eq!(checked(&document), [json!("/controls/short")]);
    let (_, document) = evaluate(&board, &["/controls/short", "/controls/kind"], &parts);
    assert_eq!(
        checked(&document),
        [json!("/controls/short"), json!("/controls/kind")]
    );
}

#[test]
fn a_pattern_shown_match_only_quotes_the_match_and_its_line_and_never_the_expression() {
    let board = board();
    board
        .write(
            "private/names.txt",
            &format!("{PRIVATE}\n\nnever-matches\n"),
        )
        .write("controls/names.json", &names("match-only"));
    let (ran, document) = evaluate(
        &board,
        &["/controls/names"],
        &json!({"pr.body": "Fine.\nSee acme-123 here.", "ok": "Fine."}),
    );
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    let results = by_part(&document);
    assert_eq!(
        results["pr.body"]["findings"],
        json!([finding("acme-123", Some(2), None)])
    );
    assert_eq!(results["ok"]["status"], "passed");
    assert!(!printed(&ran).contains(PRIVATE));
}

#[test]
fn a_pattern_shown_as_pattern_names_the_expression() {
    let board = board();
    board
        .write("private/names.txt", &format!("{PRIVATE}\n"))
        .write("controls/names.json", &names("pattern"));
    let (_, document) = evaluate(
        &board,
        &["/controls/names"],
        &json!({"pr.body": "See acme-123."}),
    );
    assert_eq!(
        by_part(&document)["pr.body"]["findings"],
        json!([finding("acme-123", Some(1), Some(PRIVATE))])
    );
}

#[test]
fn an_invalid_expression_is_refused_by_its_line_without_its_text_and_a_valid_one_is_admitted() {
    let board = board();
    let parts = json!({"pr.body": "acme-123"});
    board
        .write("controls/names.json", &names("match-only"))
        .write("private/names.txt", &format!("{PRIVATE}\n{PRIVATE}[\n"));
    let (ran, document) = evaluate(&board, &["/controls/names"], &parts);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "controls::pattern::invalid");
    assert_eq!(document["fields"]["line"], 2, "the second line of the file");
    assert!(!printed(&ran).contains(PRIVATE));
    board.write("private/names.txt", &format!("{PRIVATE}\n{PRIVATE}\\[\n"));
    let (ran, _) = evaluate(&board, &["/controls/names"], &parts);
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
}

#[test]
fn the_parsers_message_for_an_invalid_expression_never_reaches_the_output_under_match_only() {
    let board = board();
    let parts = json!({"pr.body": "acme"});
    board.write("controls/names.json", &names("match-only"));
    for line in ["(?P=acmename)", "a{99999999999}"] {
        board.write("private/names.txt", &format!("{line}\n"));
        let (ran, document) = evaluate(&board, &["/controls/names"], &parts);
        assert_eq!(document["code"], "controls::pattern::invalid", "{line}");
        assert!(!printed(&ran).contains("acmename"), "{line}");
        assert!(
            (document["message"].as_str().unwrap()).contains("line 1 of /private/names.txt"),
            "{document}"
        );
    }
    board.write("private/names.txt", "(?P<acmename>acme)\n");
    let (ran, _) = evaluate(&board, &["/controls/names"], &parts);
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
}

#[test]
fn findings_past_twenty_are_counted_and_left_out() {
    let board = board();
    board
        .write("private/names.txt", "x\n")
        .write("controls/names.json", &names("match-only"));
    let (_, document) = evaluate(
        &board,
        &["/controls/names"],
        &json!({"many": "x".repeat(25), "few": "x".repeat(20)}),
    );
    let results = by_part(&document);
    for (part, omitted) in [("many", json!(5)), ("few", Value::Null)] {
        let result = &results[part];
        assert_eq!(result["findings"].as_array().unwrap().len(), 20, "{part}");
        assert_eq!(result["omitted"], omitted, "{part}");
    }
}

#[test]
fn a_judged_rule_skips_a_part_that_failed_a_free_rule_and_is_refused_for_one_it_must_read() {
    let board = board();
    board
        .write("rubrics/plain.md", "Write plain declarative sentences.\n")
        .write(
            "controls/plain.json",
            &control(
                "Plain prose.",
                &json!({"rule": {"judged": "/rubrics/plain.md"}}),
            ),
        )
        .write(
            "controls/short.json",
            &control("Short.", &json!({"rule": {"budget": "words", "max": 1}})),
        );
    let both = ["/controls/plain", "/controls/short"];
    let (ran, document) = evaluate(&board, &both, &json!({"pr.body": "Too long."}));
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    let statuses: Vec<Value> = (output(&document)["results"].as_array().unwrap().iter())
        .map(|result| json!([result["control"], result["status"]]))
        .collect();
    assert_eq!(
        statuses,
        [
            json!(["/controls/plain", "skipped"]),
            json!(["/controls/short", "failed"]),
        ]
    );
    let (ran, document) = evaluate(&board, &both, &json!({"pr.body": "Short."}));
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "controls::judged::unavailable");
    assert_eq!(document["fields"]["control"], "/controls/plain");
    let (ran, document) = evaluate(&board, &["/controls/short"], &json!({"pr.body": "Short."}));
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    assert_eq!(output(&document)["passed"], true);
}

#[test]
fn a_control_that_is_not_there_is_refused_and_one_that_is_is_admitted() {
    let board = board();
    let parts = json!({"pr.body": "Text."});
    let (ran, document) = evaluate(&board, &["/controls/short"], &parts);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "controls::control::not_found");
    board.write(
        "controls/short.json",
        &control("Short.", &json!({"rule": {"budget": "words", "max": 3}})),
    );
    let (ran, _) = evaluate(&board, &["/controls/short"], &parts);
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
}

#[test]
fn a_rule_file_that_is_missing_is_refused_and_one_that_is_there_is_admitted() {
    let board = board();
    let parts = json!({"pr.body": "Text."});
    board.write("controls/names.json", &names("pattern"));
    let (ran, document) = evaluate(&board, &["/controls/names"], &parts);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "controls::rule::unreadable");
    assert_eq!(document["fields"]["file"], "/private/names.txt");
    board.write("private/names.txt", "acme\n");
    let (ran, _) = evaluate(&board, &["/controls/names"], &parts);
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
}

#[test]
fn a_rubric_that_is_missing_is_refused_before_any_part_is_judged() {
    let board = board();
    board
        .write(
            "controls/plain.json",
            &control(
                "Plain prose.",
                &json!({"rule": {"judged": "/rubrics/plain.md"}}),
            ),
        )
        .write(
            "controls/short.json",
            &control("Short.", &json!({"rule": {"budget": "words", "max": 1}})),
        );
    let both = ["/controls/plain", "/controls/short"];
    let parts = json!({"pr.body": "Too long."});
    let (ran, document) = evaluate(&board, &both, &parts);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "controls::rule::unreadable");
    assert_eq!(document["fields"]["file"], "/rubrics/plain.md");
    board.write("rubrics/plain.md", "Write plainly.\n");
    let (ran, _) = evaluate(&board, &both, &parts);
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
}

#[test]
fn a_control_without_a_rule_is_stated_and_never_fails() {
    let board = board();
    board.write("controls/written.json", &control("Be kind.", &json!({})));
    let (_, document) = evaluate(&board, &["/controls/written"], &json!({"pr.body": "Text."}));
    assert_eq!(
        output(&document),
        &json!({
            "passed": true,
            "results": [{
                "control": "/controls/written",
                "part": "pr.body",
                "rule": null,
                "status": "stated",
                "findings": [],
                "used": null,
                "max": null,
                "omitted": null,
            }],
        })
    );
}
