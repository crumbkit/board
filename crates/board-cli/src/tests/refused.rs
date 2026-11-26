//! How a refusal is printed, from a hand-built value.

use board_model::{Code, Fields, Lined, PluginCode, Refusal};
use serde_json::{Map, Value, json};

use crate::render::{self, Printer};

fn past() -> PluginCode {
    PluginCode::parse(&"tasks".parse().unwrap(), "tasks::due::past".to_owned()).unwrap()
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

fn refused(json: bool, refusal: &Refusal) -> crate::report::Output {
    render::refused(Printer::of(json, 1 << 20, &[]), refusal.clone())
}

#[test]
fn a_refusal_prints_its_code_message_hint_and_stderr_as_text_and_its_fields_as_json() {
    let refusal = Refusal {
        stderr: Some(b"Traceback: boom".to_vec()),
        ..Refusal::new(
            Code::PluginFailed,
            "plugin tasks exited 1",
            &Fields {
                plugin: Some("tasks".parse().unwrap()),
                ..Fields::default()
            },
        )
    };
    let plain = refused(false, &refusal);
    assert_eq!(plain.code, 1);
    assert_eq!(plain.stdout, b"");
    assert_eq!(
        text(&plain.stderr),
        "error: plugin tasks exited 1\n\
         \x20 help:  the plugin exited without an answer; the end of its stderr follows this error\n\
         \x20 code:  board::plugin::failed\n\
         Traceback: boom\n"
    );
    let document = refused(true, &refusal);
    assert_eq!(document.code, 1);
    assert_eq!(document.stderr, b"");
    assert_eq!(
        serde_json::from_slice::<Value>(&document.stdout).unwrap(),
        json!({
            "schema": "board/failure/v1",
            "code": "board::plugin::failed",
            "message": "plugin tasks exited 1",
            "fields": {"plugin": "tasks"},
            "recovery": {
                "kind": "hint",
                "text": "the plugin exited without an answer; the end of its stderr follows this error",
            },
        })
    );
}

#[test]
fn a_plugins_refusal_says_it_has_no_hint_and_a_refused_command_line_exits_2() {
    let plugin = Refusal::plugin(past(), "the date is past".to_owned(), Map::new());
    let plain = refused(false, &plugin);
    assert_eq!(
        (text(&plain.stderr), plain.code),
        (
            "error: the date is past\n  help:  no next step: the plugin gave no hint\n  code:  tasks::due::past\n",
            1
        )
    );
    let usage = Refusal::new(Code::CliUsage, "no such command", &Fields::default());
    assert_eq!(refused(false, &usage).code, 2);
    assert_eq!(refused(true, &usage).code, 2);
}

#[test]
fn a_refusal_that_lists_refused_lines_prints_each_after_crumbkits_text() {
    let line = |line, message: &str| Lined::new(line, Code::PutInvalid, message, Fields::default());
    let batch = Refusal::new(
        Code::PutInvalid,
        "the batch is refused",
        &Fields {
            refusals: vec![line(2, "it has no $type"), line(4, "it is empty")],
            ..Fields::default()
        },
    );
    assert!(text(&refused(false, &batch).stderr).ends_with(
        "  line 2: board::put::invalid: it has no $type\n\
             \x20 line 4: board::put::invalid: it is empty\n"
    ));
    let single = Refusal::new(Code::PutInvalid, "it has no $type", &Fields::default());
    assert!(!text(&refused(false, &single).stderr).contains("line"));
}

#[test]
fn a_plugins_refusal_whose_fields_hold_refusals_prints_no_refused_lines() {
    let fields = Map::from_iter([(
        "refusals".to_owned(),
        json!([{"line": 1, "code": "tasks::due::past", "message": "past"}, {"line": null}]),
    )]);
    let plugin = Refusal {
        stderr: Some(b"a log line".to_vec()),
        ..Refusal::plugin(past(), "the date is past".to_owned(), fields)
    };
    assert_eq!(
        text(&refused(false, &plugin).stderr),
        "error: the date is past\n  help:  no next step: the plugin gave no hint\n\
         \x20 code:  tasks::due::past\na log line\n"
    );
}

#[test]
fn a_batch_refusal_prints_every_refused_line_in_order_as_text_and_as_json() {
    let line = |line, message: &str| Lined::new(line, Code::PutInvalid, message, Fields::default());
    let batch = Refusal::new(
        Code::PutInvalid,
        "the batch is refused",
        &Fields {
            refusals: vec![
                line(2, "it has no $type"),
                line(4, "it is empty"),
                line(7, "it is stale"),
            ],
            ..Fields::default()
        },
    );
    let plain = refused(false, &batch);
    assert_eq!(
        text(&plain.stderr),
        "error: the batch is refused\n\
         \x20 help:  fix each line listed, then send the whole batch again; nothing was written\n\
         \x20 code:  board::put::invalid\n\
         \x20 line 2: board::put::invalid: it has no $type\n\
         \x20 line 4: board::put::invalid: it is empty\n\
         \x20 line 7: board::put::invalid: it is stale\n"
    );
    let document: Value = serde_json::from_slice(&refused(true, &batch).stdout).unwrap();
    assert_eq!(
        document["fields"],
        json!({"refusals": [
            {"line": 2, "code": "board::put::invalid", "message": "it has no $type"},
            {"line": 4, "code": "board::put::invalid", "message": "it is empty"},
            {"line": 7, "code": "board::put::invalid", "message": "it is stale"},
        ]})
    );
}

/// A board whose `min_board` names only a build date, with an identity to
/// commit as.
fn date_gated() -> board_scenario::Scenario {
    use board_scenario::{Item, Scenario};

    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(
            r#"{"$type": "board/config/v1", "min_board": {"built": "2026-11-13T00:00:00Z"}}"#,
        ),
    );
    scenario.identified();
    scenario
}

/// Writes a note into the board at `root` as an undated build, outside any
/// run, which leaves a warning on this thread.
fn write_undated(root: &std::path::Path) {
    let mut board = board_ops::Board::open(root, None).unwrap();
    board.running("0.0.0+abc", None);
    let path = "/notes/stale.md".parse().unwrap();
    board
        .put_file(&path, board_ops::Expect::Any, b"old\n")
        .unwrap();
}

/// Runs `board` through the real run as an undated build. Each test runs
/// in its own process under nextest, which bin/test and CI use, so the build
/// it declares binds only that test. Under plain `cargo test` an earlier
/// test in the process may have declared the real build first, and these
/// tests then fail without a regression.
fn undated(root: &std::path::Path, arguments: &[&str], stdin: &[u8]) -> crate::report::Output {
    board_ops::declare_build("0.0.0+abc", None);
    super::board_with(root, None, arguments, stdin)
}

#[test]
fn an_undated_put_through_the_run_warns_once_and_the_next_command_prints_no_stale_note() {
    let scenario = date_gated();
    let put = undated(scenario.workdir(), &["put", "/notes/a.md"], b"hi\n");
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
    let stderr = text(&put.stderr);
    assert_eq!(stderr.matches("warning: ").count(), 1, "{stderr}");
    assert!(stderr.contains("no build date"), "{stderr}");
    let next = undated(scenario.workdir(), &["get", "/notes/a.md"], b"");
    assert_eq!(text(&next.stderr), "");
}

#[test]
fn a_note_left_on_the_thread_before_a_run_is_not_printed_by_it_and_the_runs_own_write_is() {
    let scenario = date_gated();
    write_undated(scenario.workdir());
    let read = undated(scenario.workdir(), &["get", "/notes/stale.md"], b"");
    assert_eq!(read.code, 0, "{}", text(&read.stderr));
    assert_eq!(text(&read.stderr), "");
    write_undated(scenario.workdir());
    let put = undated(scenario.workdir(), &["put", "/notes/b.md"], b"hi\n");
    assert_eq!(text(&put.stderr).matches("warning: ").count(), 1);
}

#[test]
fn a_put_that_could_not_make_a_fill_warns_why_and_one_that_made_every_fill_does_not() {
    let mended = |unfilled: Vec<String>| board_ops::PutReport {
        writes: Vec::new(),
        removed: Vec::new(),
        absent: Vec::new(),
        commit: None,
        mended: vec![board_ops::Mended {
            path: "/reviews/r/report".parse().unwrap(),
            mends: board_ops::Mends {
                line: Some(2),
                filled: Vec::new(),
                dropped: Vec::new(),
                unfilled,
            },
        }],
    };
    let why = "/patch_ids: 'c' names no commit in /checkout".to_owned();
    let output = render::put(
        Printer::of(false, 1 << 20, &["put"]),
        &mended(vec![why]),
        None,
    );
    assert!(
        text(&output.stderr).contains(
            "warning: line 2, /reviews/r/report: could not fill /patch_ids: 'c' names no commit in /checkout"
        ),
        "{}",
        text(&output.stderr)
    );
    let output = render::put(
        Printer::of(false, 1 << 20, &["put"]),
        &mended(Vec::new()),
        None,
    );
    assert!(!text(&output.stderr).contains("could not fill"));
}
