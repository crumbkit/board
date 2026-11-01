//! The plans plugin through the binary: what `plans.open` lists, and what
//! the shipped schemas admit. These were the plugin's pytest parity suite.

use super::*;

const INVALID: Option<&str> = Some("board::record::invalid");

fn board() -> Fixture {
    Fixture::enabling(&["plans"])
}

/// A `plans/question/v1` record in `status`, asked at `asked`, with
/// `fields` added or replacing its own.
fn question(status: &str, asked: &str, fields: &Value) -> String {
    record(
        json!({
            "$type": "plans/question/v1",
            "question": "Which way?",
            "session": "local_0a1b",
            "asked": asked,
            "status": status,
        }),
        fields,
    )
}

/// An open question asked on 2026-10-01 with `fields`.
fn open(fields: &Value) -> String {
    question("open", "2026-10-01T09:00:00Z", fields)
}

/// A `plans/decision/v1` record with a reason, and `fields`.
fn decision(fields: &Value) -> String {
    record(
        json!({
            "$type": "plans/decision/v1",
            "decision": "Do it.",
            "why": "It is needed.",
            "session": "local_0a1b",
            "decided": "2026-10-01T09:00:00Z",
        }),
        fields,
    )
}

/// The paths of the questions `plans.open` lists with `flags`, and its output.
fn listed(board: &Fixture, flags: &[&str]) -> (Vec<String>, Value) {
    let output = board.call("plans.open", flags);
    let paths = (output["questions"].as_array().unwrap().iter())
        .map(|question| question["path"].as_str().unwrap().to_owned())
        .collect();
    (paths, output)
}

#[test]
fn open_lists_open_questions_oldest_first_and_leaves_out_answered_and_withdrawn_ones() {
    let board = board();
    board
        .write(
            "projects/a/questions/late.json",
            &question("open", "2026-10-03T09:00:00Z", &json!({})),
        )
        .write("projects/a/questions/early.json", &open(&json!({})))
        .write(
            "projects/a/questions/done.json",
            &question("answered", "2026-10-01T09:00:00Z", &json!({})),
        )
        .write(
            "projects/a/questions/gone.json",
            &question("withdrawn", "2026-10-01T09:00:00Z", &json!({})),
        );
    let (paths, output) = listed(&board, &[]);
    assert_eq!(
        paths,
        ["/projects/a/questions/early", "/projects/a/questions/late"]
    );
    assert_eq!(
        (
            &output["total"],
            &output["truncated"],
            &output["unreadable"]
        ),
        (&json!(2), &json!(false), &json!(0))
    );
}

#[test]
fn open_shows_what_the_owner_needs_to_answer() {
    let board = board();
    let options = json!([{"id": "a", "text": "Go."}, {"id": "b", "text": "Stay."}]);
    let fields = json!({"options": options, "recommendation": "a", "blocks": "step 2"});
    board.write("projects/a/questions/q.json", &open(&fields));
    let (_, output) = listed(&board, &[]);
    assert_eq!(
        output["questions"],
        json!([{
            "path": "/projects/a/questions/q",
            "question": "Which way?",
            "asked": "2026-10-01T09:00:00Z",
            "session": "local_0a1b",
            "options": options,
            "recommendation": "a",
            "blocks": "step 2",
        }])
    );
}

#[test]
fn open_lists_only_questions_under_the_path_it_is_given() {
    let board = board();
    board
        .write("projects/a/questions/q.json", &open(&json!({})))
        .write("projects/ab/questions/q.json", &open(&json!({})));
    for path in ["/projects/a", "/projects/a/"] {
        assert_eq!(
            listed(&board, &["--path", path]).0,
            ["/projects/a/questions/q"],
            "{path}"
        );
    }
}

#[test]
fn open_returns_at_most_the_limit_and_says_how_many_it_left_out() {
    let board = board();
    for day in 1..=3 {
        let asked = format!("2026-10-0{day}T09:00:00Z");
        board.write(
            &format!("projects/a/questions/{day}.json"),
            &question("open", &asked, &json!({})),
        );
    }
    let (paths, output) = listed(&board, &["--limit", "2"]);
    assert_eq!(
        paths,
        ["/projects/a/questions/1", "/projects/a/questions/2"]
    );
    assert_eq!(
        (&output["total"], &output["truncated"]),
        (&json!(3), &json!(true))
    );
}

#[test]
fn a_question_whose_only_fault_is_a_dangling_reference_is_listed_and_an_invalid_one_is_not() {
    let board = board();
    board
        .write(
            "projects/a/questions/dangling.json",
            &open(&json!({"context_path": "/projects/a/missing.md"})),
        )
        .write(
            "projects/a/questions/bad.json",
            &open(&json!({"session": "someone"})),
        );
    let (paths, output) = listed(&board, &[]);
    assert_eq!(paths, ["/projects/a/questions/dangling"]);
    assert_eq!(output["unreadable"], 1);
}

#[test]
fn a_question_asked_at_a_time_python_cannot_read_is_unreadable_and_one_with_a_lowercase_z_is_listed()
 {
    let board = board();
    board
        .write(
            "projects/a/questions/leap.json",
            &question("open", "2026-12-31T23:59:60Z", &json!({})),
        )
        .write(
            "projects/a/questions/lower.json",
            &question("open", "2026-10-01t09:00:00z", &json!({})),
        );
    let (paths, output) = listed(&board, &[]);
    assert_eq!(paths, ["/projects/a/questions/lower"]);
    assert_eq!(output["unreadable"], 1);
}

#[test]
fn a_mounted_question_of_another_shape_is_unreadable_and_a_mounted_question_of_this_shape_is_listed()
 {
    let board = board();
    let misshapen = [
        (
            "projects/m/questions/naive.json",
            question("open", "2026-10-01T09:00:00", &json!({})),
        ),
        (
            "projects/m/questions/dated.json",
            question("open", "2026-10-01", &json!({})),
        ),
        (
            "projects/m/questions/offset.json",
            question("open", "2026-10-01T09:00:00+0000", &json!({})),
        ),
        (
            "projects/m/questions/strings.json",
            open(&json!({"options": ["a", "b"]})),
        ),
        (
            "projects/m/questions/number.json",
            open(&json!({"question": 5})),
        ),
    ];
    // The mounted board's question schema admits any object.
    let files: Vec<(&str, String)> = [
        (
            ".board/types/plans/question/v1.schema.json",
            r#"{"type": "object"}"#.to_owned(),
        ),
        ("projects/m/questions/fits.json", open(&json!({}))),
    ]
    .into_iter()
    .chain(misshapen.iter().cloned())
    .collect();
    let (library, commit) = repository(&[], &files);
    mount(&board, "lib.json", &library, commit);
    let (paths, output) = listed(&board, &[]);
    assert_eq!(paths, ["/lib/projects/m/questions/fits"]);
    assert_eq!(output["unreadable"], misshapen.len());
}

#[test]
fn board_lists_plans_as_enabled_and_enabling_wrote_the_schemas_it_ships() {
    let board = board();
    let report = board.json(&["plugin", "list"]);
    let plugin = (report["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "plans")
        .unwrap();
    assert_eq!(
        (&plugin["status"], &plugin["enabled"]),
        (&json!("ok"), &json!(true))
    );
    let shipped = shipped("plans");
    assert_eq!(shipped.len(), 3, "question, answer and decision");
    for (path, text) in shipped {
        assert_eq!(board.read(&path), text, "{path}");
    }
}

#[test]
fn a_decision_needs_a_reason_unless_a_hook_wrote_it_as_a_stub() {
    let board = board();
    let stub = json!({"why": "", "written_by": "runs/pr-create", "review": "wanted"});
    board
        .write("decisions/reasoned.json", &decision(&json!({})))
        .write("decisions/unreasoned.json", &decision(&json!({"why": ""})))
        .write("decisions/stub.json", &decision(&stub));
    assert_eq!(
        board.checked("/decisions/"),
        codes(&[
            ("/decisions/reasoned", None),
            ("/decisions/unreasoned", INVALID),
            ("/decisions/stub", None),
        ])
    );
}

#[test]
fn a_stub_still_carries_why_even_when_it_is_empty() {
    let board = board();
    let mut stub: Value = serde_json::from_str(&decision(
        &json!({"why": "", "written_by": "runs/pr-create"}),
    ))
    .unwrap();
    board.write("decisions/empty.json", &stub.to_string());
    stub.as_object_mut().unwrap().remove("why");
    board.write("decisions/missing.json", &stub.to_string());
    assert_eq!(
        board.checked("/decisions/"),
        codes(&[("/decisions/empty", None), ("/decisions/missing", INVALID)])
    );
}

#[test]
fn written_by_names_a_plugin_and_a_hook_and_nothing_else() {
    let board = board();
    board
        .write(
            "decisions/hooked.json",
            &decision(&json!({"written_by": "runs/pr-create"})),
        )
        .write(
            "decisions/prose.json",
            &decision(&json!({"written_by": "the runs hook"})),
        )
        .write(
            "decisions/empty.json",
            &decision(&json!({"written_by": ""})),
        );
    assert_eq!(
        board.checked("/decisions/"),
        codes(&[
            ("/decisions/hooked", None),
            ("/decisions/prose", INVALID),
            ("/decisions/empty", INVALID),
        ])
    );
}

#[test]
fn a_decision_keeps_the_fields_it_had_and_refuses_others() {
    let board = board();
    let full = json!({"alternatives": ["Wait."], "urls": ["https://example.com/pull/1"], "review": "none"});
    board.write("decisions/full.json", &decision(&full)).write(
        "decisions/extra.json",
        &decision(&json!({"owner": "someone"})),
    );
    assert_eq!(
        board.checked("/decisions/"),
        codes(&[("/decisions/full", None), ("/decisions/extra", INVALID)])
    );
}

#[test]
fn a_decision_refers_to_board_entries_in_refs_and_to_web_pages_in_urls() {
    let board = board();
    board
        .write(
            "projects/a/questions/q.json",
            &question("answered", "2026-10-01T09:00:00Z", &json!({})),
        )
        .write(
            "decisions/ref.json",
            &decision(&json!({"refs": ["/projects/a/questions/q"]})),
        )
        .write(
            "decisions/ref-url.json",
            &decision(&json!({"refs": ["https://example.com/pull/1"]})),
        )
        .write(
            "decisions/url.json",
            &decision(&json!({"urls": ["https://example.com/pull/1"]})),
        )
        .write(
            "decisions/url-path.json",
            &decision(&json!({"urls": ["/projects/a/questions/q"]})),
        )
        .write(
            "decisions/url-hostless.json",
            &decision(&json!({"urls": ["https://"]})),
        );
    assert_eq!(
        board.checked("/decisions/"),
        codes(&[
            ("/decisions/ref", None),
            ("/decisions/ref-url", Some("board::ref::malformed")),
            ("/decisions/url", None),
            ("/decisions/url-path", INVALID),
            ("/decisions/url-hostless", INVALID),
        ])
    );
}

#[test]
fn a_question_has_one_of_three_statuses_and_an_asking_session() {
    let board = board();
    let asked = "2026-10-01T09:00:00Z";
    board
        .write("projects/a/questions/open.json", &open(&json!({})))
        .write(
            "projects/a/questions/withdrawn.json",
            &question("withdrawn", asked, &json!({})),
        )
        .write(
            "projects/a/questions/closed.json",
            &question("closed", asked, &json!({})),
        )
        .write(
            "projects/a/questions/nameless.json",
            &open(&json!({"session": "someone"})),
        );
    assert_eq!(
        board.checked("/projects/a/questions/"),
        codes(&[
            ("/projects/a/questions/open", None),
            ("/projects/a/questions/withdrawn", None),
            ("/projects/a/questions/closed", INVALID),
            ("/projects/a/questions/nameless", INVALID),
        ])
    );
}

/// A `plans/answer/v1` record of the question at `question`, with `fields`.
fn answer(question: &str, fields: &Value) -> String {
    record(
        json!({
            "$type": "plans/answer/v1",
            "question": question,
            "choice": "none",
            "words": "Neither.",
            "answered": "2026-10-02T09:00:00Z",
        }),
        fields,
    )
}

#[test]
fn an_answer_references_a_question() {
    let board = board();
    board
        .write(
            "projects/a/questions/q.json",
            &question("answered", "2026-10-01T09:00:00Z", &json!({})),
        )
        .write("projects/a/questions/notes.md", "background")
        .write(
            "projects/a/answers/q.json",
            &answer("/projects/a/questions/q", &json!({})),
        )
        .write(
            "projects/a/answers/n.json",
            &answer("/projects/a/questions/notes.md", &json!({})),
        );
    assert_eq!(
        board.checked("/projects/a/answers/"),
        codes(&[
            ("/projects/a/answers/q", None),
            ("/projects/a/answers/n", Some("board::ref::wrong_type")),
        ])
    );
}

#[test]
fn an_answer_a_hook_wrote_names_the_plugin_and_hook() {
    let board = board();
    let by = |written_by: &str| {
        answer(
            "/projects/a/questions/q",
            &json!({"choice": "a", "words": "Merge", "written_by": written_by}),
        )
    };
    board
        .write(
            "projects/a/questions/q.json",
            &question("answered", "2026-10-01T09:00:00Z", &json!({})),
        )
        .write("projects/a/answers/hooked.json", &by("plans/pane-answer"))
        .write("projects/a/answers/prose.json", &by("the pane"));
    assert_eq!(
        board.checked("/projects/a/answers/"),
        codes(&[
            ("/projects/a/answers/hooked", None),
            ("/projects/a/answers/prose", INVALID),
        ])
    );
}
