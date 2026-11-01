//! The jira plugin through the binary: `jira.ref` with a stub `acli` or
//! none, its refusals in pairs, and what the shipped schema admits. These
//! were the plugin's pytest suite.
//!
//! The plugin's processes see a `PATH` of a directory of this fixture's own,
//! then `/usr/bin` and `/bin`. The real `acli` is never on it.

use super::*;

const INVALID: Option<&str> = Some("board::record::invalid");
const VIEW: &str = r#"{"key": "ACME-123", "fields": {"summary": "Fix the login redirect"}}"#;

/// A board that enables jira, with a directory first on its plugins'
/// `PATH` that holds a stub `acli` once a test writes one.
struct Board {
    board: Fixture,
    bin: Scenario,
}

fn board() -> Board {
    let mut board = Fixture::enabling(&["jira"]);
    let bin = Scenario::new();
    board.path = format!("{}:{PATH}", bin.workdir().display());
    Board { board, bin }
}

/// The issue `/jira/acme/ACME-123` as `ref` writes it, with `fields`.
fn issue(fields: &Value) -> Value {
    serde_json::from_str(&record(
        json!({
            "$type": "jira/issue/v1",
            "site": "acme",
            "key": "ACME-123",
            "url": "https://acme.atlassian.net/browse/ACME-123",
        }),
        fields,
    ))
    .unwrap()
}

impl Board {
    /// Puts a stub `acli` on the plugin's `PATH` that logs its arguments and
    /// runs `script` in sh.
    fn acli(&self, script: &str) {
        let log = self.bin.workdir().join("acli.log");
        let stub = format!(
            "#!/bin/sh\necho \"$@\" >> \"{}\"\n{script}\n",
            log.display()
        );
        self.bin.write("acli", Item::Executable(&stub));
    }

    /// Each argument line the stub `acli` was run with.
    fn acli_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.bin.workdir().join("acli.log"))
            .map(|log| log.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// The output of `board call jira.ref` with `flags`, which must answer.
    fn answer(&self, flags: &[&str]) -> Value {
        self.board.call("jira.ref", flags)
    }

    /// The exit code and failure of `board call jira.ref` with `flags`.
    fn refused(&self, flags: &[&str]) -> (u8, Value) {
        let arguments: Vec<&str> = ["--json", "call", "jira.ref"]
            .into_iter()
            .chain(flags.iter().copied())
            .collect();
        let output = self.board.run(&arguments);
        (output.code, json(&output))
    }

    /// The record at `path`, from the board's root.
    fn record(&self, path: &str) -> Value {
        serde_json::from_str(&self.board.read(path)).unwrap()
    }
}

#[test]
fn ref_writes_the_issue_with_the_title_acli_reads() {
    let board = board();
    board.acli(&format!("echo '{VIEW}'"));
    let output = board.answer(&["--key", "ACME-123", "--site", "acme"]);
    assert_eq!(
        output,
        json!({
            "path": "/jira/acme/ACME-123",
            "site": "acme",
            "key": "ACME-123",
            "url": "https://acme.atlassian.net/browse/ACME-123",
            "title": "Fix the login redirect",
            "written": true,
            "title_note": null,
        })
    );
    assert_eq!(
        board.record("jira/acme/ACME-123.json"),
        issue(&json!({"title": "Fix the login redirect"}))
    );
    assert_eq!(
        board.acli_calls(),
        ["jira workitem view ACME-123 --fields summary --json"]
    );
}

#[test]
fn a_second_ref_writes_nothing_runs_no_acli_and_reports_the_record() {
    let board = board();
    board.acli(&format!("echo '{VIEW}'"));
    let flags = ["--key", "ACME-123", "--site", "acme"];
    board.answer(&flags);
    let before = board.board.read("jira/acme/ACME-123.json");
    let output = board.answer(&flags);
    assert_eq!(
        (&output["written"], &output["title"], &output["title_note"]),
        (
            &json!(false),
            &json!("Fix the login redirect"),
            &Value::Null
        )
    );
    assert_eq!(board.board.read("jira/acme/ACME-123.json"), before);
    assert_eq!(board.acli_calls().len(), 1);
}

#[test]
fn without_acli_ref_writes_the_issue_without_a_title_and_says_why() {
    let board = board();
    let output = board.answer(&["--key", "ACME-123", "--site", "acme"]);
    assert_eq!(
        (&output["written"], &output["title"]),
        (&json!(true), &Value::Null)
    );
    let note = output["title_note"].as_str().unwrap();
    assert!(note.contains("acli is not on PATH"), "{note}");
    assert_eq!(board.record("jira/acme/ACME-123.json"), issue(&json!({})));
}

#[test]
fn when_acli_fails_ref_writes_the_issue_without_a_title_and_gives_acli_s_error() {
    let board = board();
    board.acli("echo 'unauthorized: run acli jira auth login' >&2; exit 1");
    let output = board.answer(&["--key", "ACME-123", "--site", "acme"]);
    assert_eq!(output["title"], Value::Null);
    assert_eq!(
        output["title_note"],
        "acli exited 1: unauthorized: run acli jira auth login"
    );
    assert_eq!(board.record("jira/acme/ACME-123.json"), issue(&json!({})));
}

#[test]
fn a_key_that_is_not_project_hyphen_number_is_refused_and_its_twin_is_admitted() {
    for key in [
        "acme-123", "ACME123", "ACME-0", "A-1", "ACME-12a", "1ACME-2",
    ] {
        let board = board();
        let (code, refused) = board.refused(&["--key", key, "--site", "acme"]);
        assert_eq!(
            (code, &refused["code"]),
            (1, &json!("jira::key::invalid")),
            "{refused}"
        );
        assert_eq!(refused["fields"]["key"], key);
        let output = board.answer(&["--key", "AC_ME2-7", "--site", "acme"]);
        assert_eq!(output["path"], "/jira/acme/AC_ME2-7", "{key}");
    }
}

#[test]
fn a_site_that_is_not_an_atlassian_site_name_is_refused_and_its_twin_is_admitted() {
    for site in ["Acme", "acme.atlassian.net", "-acme", "acme/x"] {
        let board = board();
        let given = format!("--site={site}");
        let (code, refused) = board.refused(&["--key", "ACME-1", &given]);
        assert_eq!(
            (code, &refused["code"]),
            (1, &json!("jira::site::invalid")),
            "{refused}"
        );
        let output = board.answer(&["--key", "ACME-1", "--site", "acme-eu2"]);
        assert_eq!(
            output["url"], "https://acme-eu2.atlassian.net/browse/ACME-1",
            "{site}"
        );
    }
}

#[test]
fn board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships() {
    let board = board();
    let report = board.board.json(&["plugin", "list"]);
    let listed = (report["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "jira")
        .unwrap();
    assert_eq!(
        (&listed["status"], &listed["enabled"], &listed["offers"]),
        (&json!("ok"), &json!(true), &json!(["ref"]))
    );
    let shipped = shipped("jira");
    assert_eq!(shipped.len(), 1, "the issue schema");
    for (path, text) in shipped {
        assert_eq!(board.board.read(&path), text, "{path}");
    }
}

#[test]
fn an_issue_with_or_without_a_title_is_admitted_and_one_with_another_field_is_refused() {
    let board = board();
    let written = |fields: Value| issue(&fields).to_string();
    board
        .board
        .write("jira/acme/ACME-1.json", &written(json!({})))
        .write(
            "jira/acme/ACME-2.json",
            &written(json!({"title": "Fix it"})),
        )
        .write("jira/acme/ACME-3.json", &written(json!({"status": "Done"})));
    assert_eq!(
        board.board.checked("/jira/"),
        codes(&[
            ("/jira/acme/ACME-1", None),
            ("/jira/acme/ACME-2", None),
            ("/jira/acme/ACME-3", INVALID),
        ])
    );
}

#[test]
fn an_issue_with_a_lowercase_key_a_url_off_atlassian_or_an_empty_title_is_refused_and_its_twin_admitted()
 {
    let board = board();
    let written = |fields: Value| issue(&fields).to_string();
    board
        .board
        .write("jira/acme/twin.json", &written(json!({"title": "Fix it"})))
        .write("jira/acme/a.json", &written(json!({"key": "acme-123"})))
        .write(
            "jira/acme/b.json",
            &written(json!({"url": "https://example.com/browse/ACME-123"})),
        )
        .write("jira/acme/c.json", &written(json!({"title": ""})));
    assert_eq!(
        board.board.checked("/jira/"),
        codes(&[
            ("/jira/acme/a", INVALID),
            ("/jira/acme/b", INVALID),
            ("/jira/acme/c", INVALID),
            ("/jira/acme/twin", None),
        ])
    );
}

#[test]
fn a_key_or_site_longer_than_a_path_segment_is_refused_with_boards_code_and_its_twin_is_admitted() {
    let board = board();
    let key = |digits: usize| format!("AB-{}", "1".repeat(digits));
    let site = |letters: usize| "a".repeat(letters);
    for flags in [
        [
            "--key".to_owned(),
            key(253),
            "--site".to_owned(),
            "acme".to_owned(),
        ],
        [
            "--key".to_owned(),
            "ACME-1".to_owned(),
            "--site".to_owned(),
            site(256),
        ],
    ] {
        let flags: Vec<&str> = flags.iter().map(String::as_str).collect();
        let (code, refused) = board.refused(&flags);
        assert_eq!(
            (code, &refused["code"]),
            (1, &json!("jira::board::refused")),
            "{refused}"
        );
        assert_eq!(refused["fields"]["code"], "board::path::malformed");
    }
    // A key's file name adds `.json`, and a put writes it through a
    // temporary file with a longer name, so the twin's key keeps well inside
    // the file system's 255 bytes; a site is a directory and may take them.
    let written = board.answer(&["--key", &key(200), "--site", &site(255)]);
    assert_eq!(written["written"], true);
}

#[test]
fn a_held_issue_without_its_site_is_refused_and_a_whole_one_is_admitted() {
    let board = board();
    let mut held = issue(&json!({"title": "Fix it"}));
    held.as_object_mut().unwrap().remove("site");
    let path = "jira/acme/ACME-123.json";
    board.board.write(path, &held.to_string());
    let flags = ["--key", "ACME-123", "--site", "acme"];
    let (code, refused) = board.refused(&flags);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("jira::record::unreadable")),
        "{refused}"
    );
    assert_eq!(refused["fields"]["path"], "/jira/acme/ACME-123");
    board
        .board
        .write(path, &issue(&json!({"title": "Fix it"})).to_string());
    assert_eq!(board.answer(&flags)["title"], "Fix it");
    assert!(board.acli_calls().is_empty());
}
