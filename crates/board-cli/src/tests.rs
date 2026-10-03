//! The `board` command line, run in-process against real repositories.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use board_model::{Code, ObjectId};
use board_scenario::{Item, Scenario};
use serde_json::Value;

use crate::config::Context;
use crate::report::Output;
use crate::run::run;

const CONFIG: &str = r#"{"$type": "board/config/v1"}"#;
const TASK_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["title"],
  "properties": {
    "title": {"type": "string"},
    "project": {"type": "string", "format": "board-ref", "x-board-type": "lr/project/v1"}
  }
}"#;

fn board() -> Scenario {
    let scenario = Scenario::new();
    scenario
        .write(".board/board.json", Item::File(CONFIG))
        .write(
            ".board/types/lr/task/v1.schema.json",
            Item::File(TASK_SCHEMA),
        )
        .write(
            ".board/types/lr/project/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        )
        .write(
            "projects/p.json",
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        );
    scenario
}

fn board_in(directory: &Path, arguments: &[&str]) -> Output {
    board_cached(directory, None, arguments)
}

fn board_cached(directory: &Path, cache: Option<&Path>, arguments: &[&str]) -> Output {
    board_with(directory, cache, arguments, b"")
}

/// Runs `board` with `stdin` as its standard input.
fn board_with(directory: &Path, cache: Option<&Path>, arguments: &[&str], stdin: &[u8]) -> Output {
    let context = Context {
        start: directory.to_path_buf(),
        home: None,
        cache: cache.map(Path::to_path_buf),
    };
    let arguments = std::iter::once(env!("CARGO_BIN_NAME"))
        .chain(arguments.iter().copied())
        .map(OsString::from);
    run(arguments, &context, &mut &stdin[..])
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn code_named(name: &str) -> Code {
    Code::ALL
        .into_iter()
        .find(|code| code.as_str() == name)
        .unwrap_or_else(|| panic!("{name} is not a code"))
}

/// Every code in a JSON report: an error's, or each degraded entry's.
fn codes(output: &Output) -> Vec<Code> {
    let document = json(output);
    if let Some(code) = document.get("code").and_then(Value::as_str) {
        return vec![code_named(code)];
    }
    document["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("code")?.as_str())
        .map(code_named)
        .collect()
}

#[test]
fn help_prints_to_stdout_and_succeeds() {
    let output = board_in(Path::new("."), &["--help"]);
    assert_eq!(output.code, 0);
    assert!(text(&output.stdout).contains("Usage: board"));
}

#[test]
fn a_refused_command_line_exits_two() {
    let scenario = board();
    assert_eq!(
        board_in(scenario.workdir(), &["get", "/projects/p"]).code,
        0
    );
    for refused in [
        &["frobnicate"][..],
        &["get"],
        &["get", "projects/p"],
        &["check", "--colour"],
    ] {
        let output = board_in(scenario.workdir(), refused);
        assert_eq!(output.code, 2, "{refused:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn check_prints_one_line_per_entry_and_fails_when_any_is_degraded() {
    let scenario = board();
    let clean = board_in(scenario.workdir(), &["check"]);
    assert_eq!(clean.code, 0);
    assert!(
        text(&clean.stdout)
            .lines()
            .all(|line| line.starts_with("ok "))
    );
    scenario.write("bad.json", Item::File("{"));
    let degraded = board_in(scenario.workdir(), &["check"]);
    assert_eq!(degraded.code, 1);
    assert!(text(&degraded.stdout).contains("degraded board::json::parse bad.json\n"));
}

#[test]
fn check_json_is_a_check_report() {
    let scenario = board();
    scenario.write("bad.json", Item::File("{"));
    let output = board_in(scenario.workdir(), &["check", "--json"]);
    let report = json(&output);
    assert_eq!(report["$type"], "board/check-report/v1");
    assert_eq!(report["degraded"], true);
    let bad = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "bad.json")
        .unwrap();
    assert_eq!(bad["status"], "degraded");
    assert_eq!(bad["code"], "board::json::parse");
    assert_eq!(bad["kind"], "json");
    assert_eq!(bad["logical"], "/bad");
}

#[test]
fn get_prints_the_stored_bytes_and_json_adds_where_they_are() {
    let scenario = board();
    let stored = "{ \"$type\": \"lr/project/v1\", \"name\": \"q\" }\n";
    scenario.write("projects/q.json", Item::File(stored)).write(
        "l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/q"}"#),
    );
    let plain = board_in(scenario.workdir(), &["get", "/l"]);
    assert_eq!((plain.code, text(&plain.stdout)), (0, stored.to_owned()));
    let report = json(&board_in(scenario.workdir(), &["get", "/l", "--json"]));
    assert_eq!(report["$type"], "board/get-report/v1");
    assert_eq!(report["record"]["name"], "q");
    assert_eq!(report["location"]["display"], "/projects/q");
    assert_eq!(report["location"]["path"], "projects/q.json");
    assert_eq!(report["location"]["source"], "working-tree");
    assert_eq!(report["location"]["pointers"][0]["path"], "l.json");
}

#[test]
fn ls_lists_directories_and_entries() {
    let scenario = board();
    scenario.write("projects/sub/x.md", Item::File(""));
    let output = board_in(scenario.workdir(), &["ls", "/projects"]);
    assert_eq!(output.code, 0);
    assert_eq!(
        text(&output.stdout),
        "record ok /projects/p\ndirectory /projects/sub/\n"
    );
    let report = json(&board_in(
        scenario.workdir(),
        &["ls", "/projects", "--json"],
    ));
    assert_eq!(report["$type"], "board/ls-report/v1");
    assert_eq!(report["entries"][1]["kind"], "directory");
}

#[test]
fn a_failure_is_an_error_line_or_an_error_document() {
    let scenario = board();
    let plain = board_in(scenario.workdir(), &["get", "/missing"]);
    assert_eq!(plain.code, 1);
    assert!(plain.stdout.is_empty());
    assert_eq!(
        text(&plain.stderr),
        "error: board::path::not_found: nothing is at /missing\n"
    );
    let document = board_in(scenario.workdir(), &["get", "/missing", "--json"]);
    assert_eq!(document.code, 1);
    let error = json(&document);
    assert_eq!(error["$type"], "board/error/v1");
    assert_eq!(error["code"], "board::path::not_found");
}

fn pointer_to(path: &str) -> String {
    format!(r#"{{"$type": "board/mount/v1", "path": "{path}"}}"#)
}

fn mount_of(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

/// Writes the fixture into a board: an entry for each code `check` gives,
/// and a mounted board with names a case-insensitive file system cannot
/// hold and a blob its commit points at but does not store. Returns the
/// board and the mounted board, which must outlive it.
fn fixture() -> (Scenario, Scenario) {
    let scenario = board();
    let task = |fields: &str| format!(r#"{{"$type": "lr/task/v1", "title": "t"{fields}}}"#);

    // A mounted board with names a case-insensitive file system cannot hold
    // and a blob its commit points at but does not store.
    let mounted = Scenario::new();
    let mounted_commit = mounted.commit_items(&[
        (b"A.json", Item::File("{}")),
        (b"a.json", Item::File("{}")),
        (b"gone.json", Item::MissingBlob),
    ]);
    let mounted_path = mounted.workdir().to_str().unwrap().to_owned();

    let mut chain: Vec<(String, String)> = (0..41)
        .map(|hop| {
            (
                format!("hops/h{hop}.json"),
                pointer_to(&format!("/hops/h{}", hop + 1)),
            )
        })
        .collect();
    chain.push(("hops/h41.json".to_owned(), "{}".to_owned()));
    for (path, text) in &chain {
        scenario.write(path, Item::File(text));
    }
    scenario
        .write("json/parse.json", Item::File("{"))
        .write("json/conflict.json", Item::File("{\n<<<<<<< HEAD\n}\n"))
        .write("json/duplicate.json", Item::File(r#"{"a": 1, "a": 2}"#))
        .write(
            "json/lossy.json",
            Item::File(r#"{"a": 18446744073709551616}"#),
        )
        .write(
            "json/large.json",
            Item::File(&" ".repeat(16 * 1024 * 1024 + 1)),
        )
        .write("json/latin1.json", Item::Bytes(b"\"\xe9\""))
        .write("record/bad-type.json", Item::File(r#"{"$type": "task"}"#))
        .write(
            "record/unknown.json",
            Item::File(r#"{"$type": "lr/note/v1"}"#),
        )
        .write(
            "record/invalid.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": 1}"#),
        )
        .write(
            ".board/types/lr/broken/v1.schema.json",
            Item::File(r#"{"type": 5}"#),
        )
        .write(
            "ref/malformed.json",
            Item::File(&task(r#", "project": "projects/p""#)),
        )
        .write(
            "ref/dangling.json",
            Item::File(&task(r#", "project": "/projects/q""#)),
        )
        .write(
            "ref/wrong.json",
            Item::File(&task(r#", "project": "/ref/dangling""#)),
        )
        .write("path/.json", Item::File("{}"))
        .write("path/x", Item::File(""))
        .write("path/x.json", Item::File("{}"))
        .write("repo/nested", Item::Submodule)
        .write(
            "mount/invalid.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/x", "extra": 1}"#),
        )
        .write(
            "mount/repo.json",
            Item::File(&mount_of(
                &format!("{mounted_path}/nowhere"),
                mounted_commit,
            )),
        )
        .write(
            "mount/commit.json",
            Item::File(&mount_of(&mounted_path, ObjectId::Sha1([9; 20]))),
        )
        .write(
            "mount/remote.json",
            Item::File(&mount_of("https://example.com/b.git", mounted_commit)),
        )
        .write(
            "mount/m.json",
            Item::File(&mount_of(&mounted_path, mounted_commit)),
        )
        .write("cycle/a.json", Item::File(&pointer_to("/cycle/b")))
        .write("cycle/b.json", Item::File(&pointer_to("/cycle/a")));

    (scenario, mounted)
}

/// The fixture board: one board whose `check`, plus a few `get` and `ls`
/// calls and two boards that cannot be opened, between them produce every
/// code `check`, `get` and `ls` can produce.
#[test]
fn the_fixture_board_produces_every_code() {
    let (scenario, _mounted) = fixture();
    let mut produced: BTreeSet<&str> = BTreeSet::new();
    let mut take = |output: Output| {
        for code in codes(&output) {
            produced.insert(code.as_str());
        }
    };
    let dir = scenario.workdir();
    take(board_in(dir, &["check", "--json"]));
    take(board_in(dir, &["get", "/projects", "--json"]));
    take(board_in(dir, &["get", "/nowhere", "--json"]));
    take(board_in(dir, &["get", "/mount/m/gone", "--json"]));
    take(board_in(dir, &["ls", "/mount/m", "--json"]));
    take(board_in(dir, &["query", "SELEC 1", "--json"]));
    let put = |path: &str, record: &str| {
        board_with(
            dir,
            None,
            &["put", path, "--expect", "none", "--json"],
            record.as_bytes(),
        )
    };
    take(put("/projects/p", r#"{"$type": "lr/project/v1"}"#));
    take(put("/projects/new", r#"{"name": "no type"}"#));
    take(put("/mount/m/new", r#"{"$type": "lr/project/v1"}"#));
    scenario.write(".git/board.lock", Item::File(""));
    take(put("/projects/locked", r#"{"$type": "lr/project/v1"}"#));

    let plain = Scenario::new();
    plain.write("x.json", Item::File("{}"));
    take(board_in(plain.workdir(), &["check", "--json"]));
    let misconfigured = Scenario::new();
    misconfigured.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "x": 1}"#),
    );
    take(board_in(misconfigured.workdir(), &["check", "--json"]));

    // The index fails only when SQLite does, even in memory, and a write
    // only when the disk or its permissions refuse it, which no board can
    // cause; each mapping is tested where its error is made.
    let missing: Vec<&str> = Code::ALL
        .iter()
        .filter(|code| ![Code::IndexFailed, Code::PutFailed].contains(code))
        .map(|code| code.as_str())
        .filter(|code| !produced.contains(code))
        .collect();
    assert!(missing.is_empty(), "not produced: {missing:?}");
}

#[test]
fn check_says_on_stderr_when_the_index_is_in_memory() {
    let scenario = board();
    let cache = tempfile::tempdir().unwrap();
    let kept = board_cached(scenario.workdir(), Some(cache.path()), &["check"]);
    assert_eq!(kept.code, 0);
    assert_eq!(text(&kept.stderr), "");
    let memory = board_cached(scenario.workdir(), None, &["check"]);
    assert_eq!(memory.stdout, kept.stdout);
    assert_eq!(
        text(&memory.stderr),
        "note: the index is in memory for this run: there is no cache directory\n"
    );
}

#[test]
fn check_names_a_mounted_entry_by_its_logical_path_and_its_source() {
    let library = Scenario::new();
    let commit = library.commit(&[("notes/a.md", "a")]);
    let scenario = board();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount_of(library.workdir().to_str().unwrap(), commit)),
    );
    let output = board_in(scenario.workdir(), &["check"]);
    assert!(text(&output.stdout).contains("ok /vendor/lib/notes/a.md\n"));
    assert!(text(&output.stdout).contains("ok vendor/lib.json\n"));
    let report = json(&board_in(scenario.workdir(), &["check", "--json"]));
    let entry = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "notes/a.md")
        .unwrap();
    assert_eq!(entry["source"], commit.to_string());
    assert_eq!(entry["logical"], "/vendor/lib/notes/a.md");
}

#[test]
fn query_prints_its_columns_then_one_json_row_per_line() {
    let scenario = board();
    let output = board_in(
        scenario.workdir(),
        &[
            "query",
            "SELECT path, status FROM entries WHERE path LIKE 'projects/%'",
        ],
    );
    assert_eq!(output.code, 0);
    assert_eq!(
        text(&output.stdout),
        "[\"path\",\"status\"]\n[\"projects/p.json\",\"ok\"]\n"
    );
    let report = json(&board_in(
        scenario.workdir(),
        &[
            "query",
            "SELECT path FROM entries",
            "--limit",
            "1",
            "--json",
        ],
    ));
    assert_eq!(report["$type"], "board/query-report/v1");
    assert_eq!(report["rows"].as_array().unwrap().len(), 1);
    assert_eq!(report["truncated"], true);
}

#[test]
fn a_limit_outside_one_to_ten_thousand_is_refused_and_the_bounds_are_admitted() {
    let scenario = board();
    let run =
        |limit: &str| board_in(scenario.workdir(), &["query", "SELECT 1", "--limit", limit]).code;
    assert_eq!(run("1"), 0);
    assert_eq!(run("10000"), 0);
    assert_eq!(run("0"), 2);
    assert_eq!(run("10001"), 2);
}

#[test]
fn sql_that_cannot_run_fails_with_its_code_and_a_read_succeeds() {
    let scenario = board();
    let refused = json(&board_in(
        scenario.workdir(),
        &["query", "DELETE FROM blobs", "--json"],
    ));
    assert_eq!(refused["code"], "board::query::invalid");
    let admitted = board_in(scenario.workdir(), &["query", "SELECT count(*) FROM blobs"]);
    assert_eq!(admitted.code, 0);
}

#[test]
fn refs_prints_what_a_record_reaches_and_what_reaches_it() {
    let scenario = board();
    scenario.write(
        "tasks/a.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": "a", "project": "/projects/p"}"#),
    );
    let from = board_in(scenario.workdir(), &["refs", "/tasks/a"]);
    assert_eq!(text(&from.stdout), "from /project /projects/p ok\n");
    let to = board_in(scenario.workdir(), &["refs", "/projects/p"]);
    assert_eq!(text(&to.stdout), "to tasks/a.json /project\n");
    let report = json(&board_in(
        scenario.workdir(),
        &["refs", "/projects/p", "--json"],
    ));
    assert_eq!(report["$type"], "board/refs-report/v1");
    assert_eq!(report["to"][0]["pointers"][0], "/project");
}

#[test]
fn put_writes_the_record_on_stdin_and_reports_where() {
    let scenario = board();
    let record = br#"{"title": "a", "$type": "lr/task/v1"}"#;
    let output = board_with(scenario.workdir(), None, &["put", "/tasks/a"], record);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let blob = scenario.blob("{\n  \"$type\": \"lr/task/v1\",\n  \"title\": \"a\"\n}\n");
    assert_eq!(text(&output.stdout), format!("wrote /tasks/a {blob}\n"));
    let again = json(&board_with(
        scenario.workdir(),
        None,
        &["put", "/tasks/a", "--expect", &blob.to_string(), "--json"],
        record,
    ));
    assert_eq!(again["$type"], "board/put-report/v1");
    assert_eq!(again["writes"][0]["previous"], blob.to_string());
    assert_eq!(again["writes"][0]["display"], "/tasks/a");
    assert_eq!(again["commit"], Value::Null);
}

#[test]
fn put_refuses_a_stale_expect_with_exit_one_and_a_malformed_one_with_exit_two() {
    let scenario = board();
    let record = br#"{"$type": "lr/project/v1"}"#;
    let run = |expect: &str| {
        board_with(
            scenario.workdir(),
            None,
            &["put", "/projects/p", "--expect", expect],
            record,
        )
    };
    let stale = run("none");
    assert_eq!(stale.code, 1);
    assert!(text(&stale.stderr).starts_with("error: board::put::stale"));
    assert_eq!(run("not-a-blob").code, 2);
    let current = scenario.blob(r#"{"$type": "lr/project/v1"}"#);
    assert_eq!(run(&current.to_string()).code, 0);
}
