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
        &json!({"rule": {"check": "pattern", "file": "/private/names.txt", "quote": show}}),
    )
}

#[test]
fn a_part_over_a_budget_fails_and_one_within_it_passes() {
    let board = board();
    board.write(
        "controls/short.json",
        &control(
            "Short.",
            &json!({"rule": {"check": "budget", "unit": "words", "max": 3}}),
        ),
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
        &control(
            "Small.",
            &json!({"rule": {"check": "budget", "unit": "bytes", "max": 4}}),
        ),
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
            &control(
                "Short.",
                &json!({"rule": {"check": "budget", "unit": "words", "max": 3}}),
            ),
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

/// A stub in place of a model: it keeps the prompt, counts its calls and
/// prints `answer.txt` beside it.
const STUB: &str = r#"#!/bin/sh
here="$(dirname "$0")"
cat > "$here/prompt.txt"
echo call >> "$here/calls.txt"
cat "$here/answer.txt"
"#;

/// The stub agent in a directory of its own, which `evaluate` starts
/// through `BOARD_CONTROLS_AGENT`. It answers with no findings until told
/// otherwise.
struct Agent {
    directory: tempfile::TempDir,
}

impl Agent {
    #[expect(
        clippy::disallowed_methods,
        reason = "a test writes the stub agent's script"
    )]
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt as _;
        let agent = Self {
            directory: tempfile::tempdir().unwrap(),
        };
        let script = agent.directory.path().join("agent");
        std::fs::write(&script, STUB).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        agent.answers(&json!([]));
        agent
    }

    /// The agent's argv, which names its model so verdicts are kept.
    fn argv(&self) -> Vec<String> {
        let script = self.directory.path().join("agent");
        vec![
            script.to_str().unwrap().to_owned(),
            "--model".to_owned(),
            "stub-model-1".to_owned(),
        ]
    }

    /// Makes the agent answer with some prose and then `findings`.
    fn answers(&self, findings: &Value) {
        self.says(&format!("Read it.\n{}\n", json!({"findings": findings})));
    }

    /// Makes the agent answer with `answer`.
    #[expect(
        clippy::disallowed_methods,
        reason = "a test writes what the stub agent answers"
    )]
    fn says(&self, answer: &str) {
        std::fs::write(self.directory.path().join("answer.txt"), answer).unwrap();
    }

    /// How many times the agent ran.
    fn calls(&self) -> usize {
        std::fs::read_to_string(self.directory.path().join("calls.txt"))
            .map_or(0, |calls| calls.lines().count())
    }

    /// What the agent last read.
    fn prompt(&self) -> String {
        std::fs::read_to_string(self.directory.path().join("prompt.txt")).unwrap()
    }
}

/// [`evaluate`] with `argv` as `BOARD_CONTROLS_AGENT`.
fn judging(board: &Fixture, controls: &[&str], parts: &Value, argv: &[String]) -> (Output, Value) {
    let stdin = json!({"controls": controls, "parts": parts}).to_string();
    let argv = serde_json::to_string(argv).unwrap();
    let ran = board.run_inheriting(
        &["--json", "call", "controls.evaluate"],
        &stdin,
        &[("BOARD_CONTROLS_AGENT", &argv)],
    );
    let document = json(&ran);
    (ran, document)
}

/// A board with a judged control and its rubric at /controls/plain and
/// /rubrics/plain.md.
fn judged() -> Fixture {
    let board = board();
    board
        .write("rubrics/plain.md", "Write plain declarative sentences.\n")
        .write(
            "controls/plain.json",
            &control(
                "Plain prose.",
                &json!({"rule": {"check": "judged", "rubric": "/rubrics/plain.md"}}),
            ),
        );
    board
}

/// Where judged verdicts are kept under `board`'s cache home.
fn verdicts(board: &Fixture) -> std::path::PathBuf {
    board.cache.path().join("board-controls")
}

#[test]
fn a_judged_part_fails_with_the_models_findings_and_passes_without_them() {
    let (board, agent) = (judged(), Agent::new());
    let found =
        json!({"line": 2, "text": "Measured, not guessed.", "rewrite": "Name who measured it."});
    agent.answers(&json!([found]));
    let parts = json!({"pr.body": "We measured it.\nMeasured, not guessed."});
    let (ran, document) = judging(&board, &["/controls/plain"], &parts, &agent.argv());
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    let result = &by_part(&document)["pr.body"];
    assert_eq!(
        (&result["rule"], &result["status"]),
        (&json!("judged"), &json!("failed"))
    );
    assert_eq!(
        result["findings"],
        json!([{"line": 2, "text": "Measured, not guessed.", "pattern": null, "rewrite": "Name who measured it."}])
    );
    let prompt = agent.prompt();
    assert!(
        prompt.contains("Write plain declarative sentences."),
        "{prompt}"
    );
    assert!(
        prompt.contains("    2| Measured, not guessed.\n"),
        "{prompt}"
    );
    agent.answers(&json!([]));
    let parts = json!({"pr.body": "We measured it."});
    let (_, document) = judging(&board, &["/controls/plain"], &parts, &agent.argv());
    assert_eq!(by_part(&document)["pr.body"]["status"], "passed");
}

#[test]
fn a_verdict_is_reused_for_the_same_text_rubric_and_agent_and_judged_again_when_one_changes() {
    let (board, agent) = (judged(), Agent::new());
    let parts = json!({"pr.body": "We measured it."});
    judging(&board, &["/controls/plain"], &parts, &agent.argv());
    let (_, document) = judging(&board, &["/controls/plain"], &parts, &agent.argv());
    assert_eq!(agent.calls(), 1);
    assert_eq!(by_part(&document)["pr.body"]["status"], "passed");
    let changed = json!({"pr.body": "We measured it twice."});
    judging(&board, &["/controls/plain"], &changed, &agent.argv());
    assert_eq!(agent.calls(), 2, "the changed text");
    board.write("rubrics/plain.md", "Write short sentences.\n");
    judging(&board, &["/controls/plain"], &parts, &agent.argv());
    assert_eq!(agent.calls(), 3, "the changed rubric");
    let other = [
        agent.argv(),
        vec!["--model".to_owned(), "another".to_owned()],
    ]
    .concat();
    judging(&board, &["/controls/plain"], &parts, &other);
    assert_eq!(agent.calls(), 4, "the changed agent command");
}

#[test]
fn an_agent_that_does_not_name_its_model_judges_every_time_and_one_that_does_is_cached() {
    let (board, agent) = (judged(), Agent::new());
    let parts = json!({"pr.body": "We measured it."});
    let unpinned = agent.argv()[..1].to_vec();
    for _ in 0..2 {
        let (ran, _) = judging(&board, &["/controls/plain"], &parts, &unpinned);
        assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    }
    assert_eq!(agent.calls(), 2, "judged on both calls");
    assert!(!verdicts(&board).exists());
    for _ in 0..2 {
        judging(&board, &["/controls/plain"], &parts, &agent.argv());
    }
    assert_eq!(agent.calls(), 3, "judged once more, then kept");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a test deletes the verdict cache"
)]
fn a_deleted_cache_is_rebuilt_by_judging_again() {
    let (board, agent) = (judged(), Agent::new());
    let parts = json!({"pr.body": "We measured it."});
    judging(&board, &["/controls/plain"], &parts, &agent.argv());
    std::fs::remove_dir_all(verdicts(&board)).unwrap();
    for _ in 0..2 {
        let (_, document) = judging(&board, &["/controls/plain"], &parts, &agent.argv());
        assert_eq!(by_part(&document)["pr.body"]["status"], "passed");
        assert_eq!(agent.calls(), 2);
    }
}

#[test]
fn a_judged_rule_skips_a_part_that_failed_a_free_rule_whatever_order_the_controls_come_in() {
    let (board, agent) = (judged(), Agent::new());
    board.write(
        "controls/short.json",
        &control(
            "Short.",
            &json!({"rule": {"check": "budget", "unit": "words", "max": 3}}),
        ),
    );
    let parts = json!({"pr.title": "Short title.", "pr.body": "A body that is far too long."});
    let (ran, document) = judging(
        &board,
        &["/controls/plain", "/controls/short"],
        &parts,
        &agent.argv(),
    );
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    let statuses: Vec<Value> = (output(&document)["results"].as_array().unwrap().iter())
        .map(|result| json!([result["control"], result["part"], result["status"]]))
        .collect();
    assert_eq!(
        statuses,
        [
            json!(["/controls/plain", "pr.body", "skipped"]),
            json!(["/controls/plain", "pr.title", "passed"]),
            json!(["/controls/short", "pr.body", "failed"]),
            json!(["/controls/short", "pr.title", "passed"]),
        ]
    );
    assert_eq!(agent.calls(), 1);
    assert!(agent.prompt().contains("Short title."));
}

#[test]
fn free_rules_run_without_an_agent_and_a_judged_rule_with_a_part_to_read_needs_one() {
    let (board, agent) = (judged(), Agent::new());
    board.write(
        "controls/short.json",
        &control(
            "Short.",
            &json!({"rule": {"check": "budget", "unit": "words", "max": 1}}),
        ),
    );
    let both = ["/controls/short", "/controls/plain"];
    let (ran, document) = evaluate(&board, &both, &json!({"pr.body": "Too long."}));
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    let statuses: Vec<&Value> = (output(&document)["results"].as_array().unwrap().iter())
        .map(|result| &result["status"])
        .collect();
    assert_eq!(statuses, [&json!("failed"), &json!("skipped")]);
    let short = json!({"pr.body": "Short."});
    let (ran, document) = evaluate(&board, &both, &short);
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "controls::agent::not_set");
    let (ran, _) = judging(&board, &both, &short, &agent.argv());
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
}

#[test]
fn an_answer_without_findings_is_refused_and_one_with_them_is_admitted() {
    let (board, agent) = (judged(), Agent::new());
    let parts = json!({"pr.body": "Text."});
    agent.says("Looks fine.\n{\"verdict\": \"approve\"}\n");
    let (ran, document) = judging(&board, &["/controls/plain"], &parts, &agent.argv());
    assert_eq!(ran.code, 1, "{document}");
    assert_eq!(document["code"], "controls::agent::bad_output");
    agent.answers(&json!([{"text": "Text.", "rewrite": "Say more."}]));
    let (ran, document) = judging(&board, &["/controls/plain"], &parts, &agent.argv());
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    assert_eq!(
        by_part(&document)["pr.body"]["findings"],
        json!([{"text": "Text.", "line": null, "pattern": null, "rewrite": "Say more."}])
    );
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
        &control(
            "Short.",
            &json!({"rule": {"check": "budget", "unit": "words", "max": 3}}),
        ),
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
                &json!({"rule": {"check": "judged", "rubric": "/rubrics/plain.md"}}),
            ),
        )
        .write(
            "controls/short.json",
            &control(
                "Short.",
                &json!({"rule": {"check": "budget", "unit": "words", "max": 1}}),
            ),
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
