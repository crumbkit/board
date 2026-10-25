//! The `board` command line, run in-process against real repositories.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use board_model::{BINARY_NAME, Code, Fields, LogicalPath, ObjectId};
use board_scenario::seed::{Exhibit, NOT_CHECKED, NOT_REPRODUCED, Seed, mount_of};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::config::Context;
use crate::report::Output;
use crate::run::run;
use crate::timing::Ran;

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
    board_full(directory, cache, None, arguments, stdin)
}

/// Runs `board` with plugins installed in `plugins`.
fn board_full(
    directory: &Path,
    cache: Option<&Path>,
    plugins: Option<&Path>,
    arguments: &[&str],
    stdin: &[u8],
) -> Output {
    let environment = board_plugin::Environment {
        path: Some("/usr/bin:/bin".into()),
        ..board_plugin::Environment::default()
    };
    board_environment(directory, cache, plugins, environment, arguments, stdin)
}

/// Runs `board` with plugins that inherit `environment`.
fn board_environment(
    directory: &Path,
    cache: Option<&Path>,
    plugins: Option<&Path>,
    environment: board_plugin::Environment,
    arguments: &[&str],
    stdin: &[u8],
) -> Output {
    let context = Context {
        start: directory.to_path_buf(),
        home: None,
        cache: cache.map(Path::to_path_buf),
        plugins: plugins.map(Path::to_path_buf),
        boards: None,
        environment,
        complete: None,
        terminal: false,
        cursor: None,
    };
    let arguments = std::iter::once(BINARY_NAME)
        .chain(arguments.iter().copied())
        .map(OsString::from);
    run(arguments, &context, &mut &stdin[..], &mut Ran::default())
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Each answer of a batch document in the order of its keys: crumbkit
/// prints the successes in `data` and the failures in `failures`, each
/// failure with the index of its key.
fn answers(output: &Output) -> Vec<Value> {
    let report = json(output);
    let mut data = report["data"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter();
    let failures = report["failures"].as_array().cloned().unwrap_or_default();
    let count = data.len() + failures.len();
    (0..count)
        .map(|index| {
            (failures.iter())
                .find(|failure| failure["index"] == index)
                .cloned()
                .unwrap_or_else(|| data.next().unwrap_or_default())
        })
        .collect()
}

/// The one answer in a `board refs --json` run of one path.
fn answer(output: &Output) -> Value {
    assert_eq!(json(output)["schema"], "board/refs-report/v2");
    let answers = answers(output);
    assert_eq!(answers.len(), 1);
    answers[0].clone()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The hint line of a text error.
fn hint_of(output: &Output) -> String {
    let stderr = text(&output.stderr);
    stderr
        .lines()
        .find_map(|line| line.strip_prefix("  help:  "))
        .unwrap_or_default()
        .to_owned()
}

fn code_named(name: &str) -> Code {
    Code::ALL
        .into_iter()
        .find(|code| code.as_str() == name)
        .unwrap_or_else(|| panic!("{name} is not a code"))
}

#[test]
fn help_prints_to_stdout_and_succeeds() {
    let output = board_in(Path::new("."), &["--help"]);
    assert_eq!(output.code, 0);
    assert!(text(&output.stdout).contains("Usage: board"));
}

#[test]
fn the_command_line_names_the_app_through_its_constants() {
    let help = text(&board_in(Path::new("."), &["--help"]).stdout);
    assert!(
        help.starts_with(&format!("{}: ", board_model::DISPLAY_NAME)),
        "{help}"
    );
    assert!(help.contains(&format!("Usage: {BINARY_NAME} ")), "{help}");
    assert_eq!(BINARY_NAME, env!("CARGO_BIN_NAME"));
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
        &["get", "projects//p"],
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
    let printed = text(&clean.stdout);
    let mut lines = printed.lines();
    assert_eq!(lines.next(), Some("4 entries, 0 degraded"));
    assert!(lines.all(|line| line.starts_with("ok ")), "{printed}");
    scenario.write("bad.json", Item::File("{"));
    let degraded = board_in(scenario.workdir(), &["check"]);
    assert_eq!(degraded.code, 1);
    assert!(text(&degraded.stdout).contains(
        "degraded board::json::parse /bad \
         is invalid: json does not parse: EOF while parsing an object at line 1 column 1\n"
    ));
}

#[test]
fn check_json_is_a_check_report() {
    let scenario = board();
    scenario.write("bad.json", Item::File("{"));
    let output = board_in(scenario.workdir(), &["check", "--json"]);
    let report = json(&output);
    assert_eq!(report["schema"], "board/check-report/v2");
    assert_eq!(report["summary"]["degraded"], 1);
    let bad = report["data"]
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
fn check_says_where_and_why_an_entry_is_degraded_and_how_many_failures_it_has() {
    let scenario = board();
    scenario
        .write(
            "tasks/t5.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": 5}"#),
        )
        .write(
            "tasks/t6.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": 5, "project": 1}"#),
        )
        .write("tasks/t7.json", Item::File(r#"{"$type": "lr/task/v1"}"#))
        .write(
            "tasks/t8.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "t", "project": "/nowhere"}"#),
        )
        .write(
            "tasks/t9.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "t", "project": "/projects/p"}"#),
        )
        .write(
            "tasks/t10.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "t", "project": "a//b"}"#),
        );
    let output = board_in(scenario.workdir(), &["check"]);
    let lines = text(&output.stdout);
    for line in [
        r#"degraded board::record::invalid /tasks/t5 fails its schema at "/title": 5 is not of type "string""#,
        r#"degraded board::record::invalid /tasks/t6 fails its schema at "/project": 1 is not of type "string" (and 1 more)"#,
        r#"degraded board::record::invalid /tasks/t7 fails its schema at its root: "title" is a required property"#,
        r#"degraded board::ref::malformed /tasks/t10 holds a reference at "/project": "a//b" is not a logical path"#,
        r#"degraded board::ref::dangling /tasks/t8 holds a reference at "/project": nothing is at /nowhere"#,
        "ok /tasks/t9",
    ] {
        assert!(
            lines.lines().any(|printed| printed == line),
            "{line}\n{lines}"
        );
    }
    let report = json(&board_in(scenario.workdir(), &["check", "--json"]));
    let entry = |path: &str| {
        report["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == path)
            .unwrap()
            .clone()
    };
    let t6 = entry("tasks/t6.json");
    assert_eq!(t6["pointer"], "/project");
    assert_eq!(
        t6["reason"],
        r#"fails its schema at "/project": 1 is not of type "string""#
    );
    assert_eq!(t6["failures"], 2);
    let t9 = entry("tasks/t9.json");
    assert_eq!(t9.get("pointer"), None);
    assert_eq!(t9.get("reason"), None);
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
    let report = get::got(&board_in(scenario.workdir(), &["get", "/l", "--json"]));
    assert_eq!(report["record"]["name"], "q");
    assert_eq!(report["location"]["display"], "/projects/q");
    assert_eq!(report["location"]["path"], "projects/q.json");
    assert_eq!(report["location"]["source"], "worktree");
    assert_eq!(report["location"]["pointers"][0]["path"], "l.json");
}

#[test]
fn get_json_names_why_bytes_that_do_not_parse_are_not_a_record_and_gives_its_parsed_twin_its_record()
 {
    let scenario = board();
    scenario
        .write("torn.json", Item::File(r#"{"$type": "lr/project/v1", "#))
        .write("whole.json", Item::File(r#"{"$type": "lr/project/v1"}"#));
    let torn = board_in(scenario.workdir(), &["get", "/torn", "--json"]);
    assert_eq!(torn.code, 0);
    let answer = get::got(&torn);
    assert_eq!(answer["code"], "board::json::parse");
    assert_eq!(answer["record"], Value::Null);
    assert_eq!(answer["text"], r#"{"$type": "lr/project/v1", "#);
    assert!(answer["blob"].is_string());
    let whole = board_in(scenario.workdir(), &["get", "/whole", "--json"]);
    assert_eq!(whole.code, 0);
    let answer = get::got(&whole);
    assert_eq!(answer["record"]["$type"], "lr/project/v1");
    assert_eq!(answer["code"], Value::Null);
}

#[test]
fn get_prints_a_files_bytes_and_json_gives_them_with_why_it_is_not_a_record() {
    let scenario = board();
    scenario.write("notes.md", Item::File("# n\n"));
    let plain = board_in(scenario.workdir(), &["get", "/notes.md"]);
    assert_eq!((plain.code, text(&plain.stdout)), (0, "# n\n".to_owned()));
    let document = board_in(scenario.workdir(), &["get", "/notes.md", "--json"]);
    assert_eq!(document.code, 0);
    let answer = get::got(&document);
    assert_eq!(answer["code"], "board::record::not_a_record");
    assert_eq!(answer["text"], "# n\n");
    scenario.write("bad.json", Item::File(r#"{"$type": "task"}"#));
    let broken = board_in(scenario.workdir(), &["get", "/bad"]);
    assert_eq!(broken.code, 0);
    let broken = board_in(scenario.workdir(), &["get", "/bad", "--json"]);
    assert_eq!(get::got(&broken)["code"], "board::record::bad_type");
    let record = board_in(scenario.workdir(), &["get", "/projects/p", "--json"]);
    assert_eq!(record.code, 0);
    assert_eq!(get::got(&record)["record"]["$type"], "lr/project/v1");
}

#[test]
fn get_refuses_a_path_beside_an_invalid_pointer_with_the_code_check_gives_it() {
    let scenario = board();
    scenario
        .write("x.json", Item::File("{}"))
        .write("x/c.json", Item::File("{}"));
    let get = board_in(scenario.workdir(), &["get", "/x/c"]);
    assert_eq!((get.code, text(&get.stdout)), (0, "{}".to_owned()));
    assert!(text(&board_in(scenario.workdir(), &["check"]).stdout).contains("ok /x/c\n"));
    scenario.write(
        "x.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/a", "repo": "r"}"#),
    );
    let get = board_in(scenario.workdir(), &["get", "/x/c", "--json"]);
    assert_eq!(get::got(&get)["code"], Code::PathShadowed.as_str());
    let check = text(&board_in(scenario.workdir(), &["check"]).stdout);
    assert!(
        check.contains("degraded board::path::shadowed x/c.json\n"),
        "{check}"
    );
}

#[test]
fn init_makes_a_repository_a_board_and_says_what_to_run_next() {
    let scenario = Scenario::new();
    let refused = board_in(scenario.workdir(), &["check"]);
    assert_eq!(refused.code, 1);
    let created = board_in(scenario.workdir(), &["init"]);
    assert_eq!(created.code, 0);
    assert!(text(&created.stdout).starts_with("created .board/board.json in "));
    assert!(!text(&created.stdout).contains("next:"));
    assert!(text(&created.stderr).starts_with("next: run `board check`"));
    assert_eq!(board_in(scenario.workdir(), &["check"]).code, 0);
    let fresh = Scenario::new();
    let created = board_in(fresh.workdir(), &["init", "--json"]);
    assert_eq!(json(&created)["data"]["created"], true);
    assert_eq!(
        text(&created.stderr),
        "",
        "the JSON document leaves the note out"
    );
    let again = board_in(scenario.workdir(), &["init"]);
    assert!(text(&again.stdout).ends_with(" is already a board\n"));
    assert_eq!(text(&again.stderr), "");
    let again = json(&board_in(scenario.workdir(), &["init", "--json"]));
    assert_eq!(again["schema"], "board/init-report/v2");
    assert_eq!(again["data"]["created"], false);
}

#[test]
fn init_outside_a_repository_says_to_run_git_init_and_inside_one_succeeds() {
    let outside = tempfile::tempdir().unwrap();
    let refused = board_in(outside.path(), &["init"]);
    assert_eq!(refused.code, 1);
    assert!(text(&refused.stderr).contains("run `git init` first"));
    assert_eq!(board_in(Scenario::new().workdir(), &["init"]).code, 0);
}

#[test]
fn check_outside_a_repository_hints_git_init_and_in_a_repository_without_a_board_hints_board_init()
{
    let outside = tempfile::tempdir().unwrap();
    let refused = json(&board_in(outside.path(), &["check", "--json"]));
    assert_eq!(refused["code"], "board::config::not_found");
    let hint = refused["recovery"]["text"].as_str().unwrap();
    assert!(hint.starts_with("run `git init` first"), "{hint}");
    assert!(!hint.contains("board init"), "{hint}");
    let repository = Scenario::new();
    let refused = json(&board_in(repository.workdir(), &["check", "--json"]));
    assert_eq!(refused["code"], "board::config::not_found");
    let hint = refused["recovery"]["text"].as_str().unwrap();
    assert!(hint.contains("`board init` makes it a board"), "{hint}");
}

#[test]
fn ls_lists_directories_and_entries() {
    let scenario = board();
    scenario.write("projects/sub/x.md", Item::File(""));
    let output = board_in(scenario.workdir(), &["ls", "/projects"]);
    assert_eq!(output.code, 0);
    assert_eq!(
        text(&output.stdout),
        "/projects:\nrecord ok /projects/p\ndirectory /projects/sub/\n"
    );
    let report = json(&board_in(
        scenario.workdir(),
        &["ls", "/projects", "--json"],
    ));
    assert_eq!(report["schema"], "board/ls-report/v2");
    assert_eq!(report["data"][1]["kind"], "directory");
}

#[test]
fn a_failure_is_an_error_line_or_an_error_document() {
    let scenario = board();
    let plain = board_in(scenario.workdir(), &["get", "/missing"]);
    assert_eq!(plain.code, 1);
    assert!(plain.stdout.is_empty());
    assert_eq!(
        text(&plain.stderr),
        "error: nothing is at /missing\n\
         \x20 key:   /missing (index 0)\n\
         \x20 help:  run `board ls /` to see what is there; paths leave out `.json`\n\
         \x20 code:  board::path::not_found\n"
    );
    let document = board_in(scenario.workdir(), &["get", "/missing", "--json"]);
    assert_eq!(document.code, 1);
    let error = get::got(&document);
    assert_eq!(error["key"], "/missing");
    assert_eq!(error["code"], "board::path::not_found");
    assert_eq!(
        error["recovery"]["text"],
        Code::PathNotFound.hint(&Fields {
            ancestor: Some(LogicalPath::root()),
            ..Fields::at(&"/missing".parse().unwrap())
        })
    );
}

/// Every entry `check --json` reports, as an exhibit, sorted by path.
fn exhibits(report: &Value) -> Vec<Exhibit> {
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let reported = report["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| Exhibit {
            logical: text(&entry["logical"]),
            path: text(&entry["path"]).unwrap(),
            code: text(&entry["code"]).map(|code| code_named(&code)),
        })
        .collect();
    by_path(reported)
}

fn by_path(mut exhibits: Vec<Exhibit>) -> Vec<Exhibit> {
    exhibits.sort_by(|a, b| (&a.logical, &a.path).cmp(&(&b.logical, &b.path)));
    exhibits
}

/// Every code but those the seed lists as never given by `check` has an
/// exhibit, so a new code fails here until it has one or a reason.
#[test]
fn the_seed_board_has_an_exhibit_for_every_code_check_can_give() {
    let seed = Seed::new();
    let exhibited: BTreeSet<&str> = seed
        .exhibits
        .iter()
        .filter_map(|exhibit| exhibit.code.map(Code::as_str))
        .collect();
    let checkable: BTreeSet<&str> = Code::ALL
        .iter()
        .filter(|code| !NOT_CHECKED.iter().any(|(never, _)| never == *code))
        .map(|code| code.as_str())
        .collect();
    assert_eq!(exhibited, checkable);
}

/// The seed board checks with exactly the codes it describes, from a cold
/// index, a warm one and one in memory.
#[test]
fn check_gives_each_seed_exhibit_its_code_from_a_warm_index_and_a_cold_one() {
    let seed = Seed::new();
    let cache = tempfile::tempdir().unwrap();
    let dir = seed.board.workdir();
    let arguments = ["check", "--json", "--all"];
    let cold = json(&board_cached(dir, Some(cache.path()), &arguments));
    // A warm check hashes, parses and writes nothing, and opens only the
    // schema it could not read.
    let rechecked = board_ops::Board::open(dir, None)
        .unwrap()
        .check(None, Some(cache.path()))
        .unwrap();
    assert_eq!(
        (
            rechecked.work.hashed,
            rechecked.work.parsed,
            rechecked.work.wrote
        ),
        (0, 0, false),
        "the cold run left the index warm"
    );
    let warm = json(&board_cached(dir, Some(cache.path()), &arguments));
    let memory = json(&board_cached(dir, None, &arguments));
    assert_eq!(warm["data"], cold["data"]);
    assert_eq!(memory["data"], cold["data"]);
    let reported = exhibits(&cold);
    let described = by_path(seed.exhibits);
    let degraded = |exhibits: &[Exhibit]| -> Vec<Exhibit> {
        exhibits
            .iter()
            .filter(|exhibit| exhibit.code.is_some())
            .cloned()
            .collect()
    };
    assert_eq!(degraded(&reported), degraded(&described));
    let unreported: Vec<&Exhibit> = described
        .iter()
        .filter(|exhibit| !reported.contains(exhibit))
        .collect();
    assert!(
        unreported.is_empty(),
        "described but not reported: {unreported:?}"
    );
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
fn ls_says_on_stderr_when_the_index_is_in_memory() {
    let scenario = board();
    let cache = tempfile::tempdir().unwrap();
    let kept = board_cached(scenario.workdir(), Some(cache.path()), &["ls"]);
    assert_eq!(kept.code, 0);
    assert_eq!(text(&kept.stderr), "");
    let memory = board_cached(scenario.workdir(), None, &["ls"]);
    assert_eq!(memory.stdout, kept.stdout);
    assert_eq!(
        text(&memory.stderr),
        "note: the index is in memory for this run: there is no cache directory\n"
    );
}

/// crumbkit's JSON document has no place for a note, so Board prints the
/// index's note on stderr after it, as it does in text.
#[test]
fn a_json_report_says_on_stderr_when_the_index_is_in_memory_and_nothing_when_it_is_kept() {
    let scenario = board();
    let cache = tempfile::tempdir().unwrap();
    let lines: [&[&str]; 4] = [
        &["check", "--json"],
        &["ls", "--json"],
        &["query", "SELECT 1", "--json"],
        &["refs", "/projects/p", "--json"],
    ];
    for words in lines {
        let kept = board_cached(scenario.workdir(), Some(cache.path()), words);
        assert_eq!(text(&kept.stderr), "", "{words:?}");
        let memory = board_cached(scenario.workdir(), None, words);
        assert_eq!(memory.stdout, kept.stdout, "{words:?}");
        assert_eq!(
            text(&memory.stderr),
            "note: the index is in memory for this run: there is no cache directory\n",
            "{words:?}"
        );
    }
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
    assert!(text(&output.stdout).contains("ok /vendor/lib\n"));
    let report = json(&board_in(scenario.workdir(), &["check", "--json"]));
    let entry = report["data"]
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
    assert_eq!(report["schema"], "board/query-report/v2");
    assert_eq!(report["data"].as_array().unwrap().len(), 1);
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
        &["query", "DELETE FROM main.blobs", "--json"],
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
    assert_eq!(text(&to.stdout), "to /tasks/a /project\n");
    let report = answer(&board_in(
        scenario.workdir(),
        &["refs", "/projects/p", "--json"],
    ));
    assert_eq!(report["to"][0]["pointers"][0], "/project");
}

#[test]
fn refs_json_is_one_report_whose_answers_match_their_golden_bytes() {
    let scenario = board();
    scenario.write(
        "tasks/a.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": "a", "project": "/projects/p"}"#),
    );
    let ran = board_in(
        scenario.workdir(),
        &["refs", "/tasks/a", "/tasks/zz", "--json"],
    );
    assert_eq!(
        (ran.code, ran.error.as_deref()),
        (1, Some("board::path::not_found"))
    );
    let root = std::fs::canonicalize(scenario.workdir()).unwrap();
    let printed = text(&ran.stdout).replace(&root.display().to_string(), "<root>");
    assert_eq!(printed, GOLDEN_REFS);
}

/// `board refs /tasks/a /tasks/zz --json`, with the board's root as `<root>`.
const GOLDEN_REFS: &str = r#"{"schema":"board/refs-report/v2","data":[{"from":[{"link":false,"location":{"display":"/projects/p","path":"projects/p.json","pointers":[],"repository":"<root>","source":"worktree"},"pointer":"/project","status":"ok","target":"/projects/p"}],"location":{"display":"/tasks/a","path":"tasks/a.json","pointers":[],"repository":"<root>","source":"worktree"},"missing":null,"to":[],"total":1,"truncated":false}],"failures":[{"key":"/tasks/zz","index":1,"code":"board::path::not_found","message":"nothing is at /tasks/zz","fields":{"ancestor":"/tasks","path":"/tasks/zz"},"recovery":{"kind":"hint","text":"run `board ls /tasks` to see what is there; paths leave out `.json`"}}]}
"#;

#[test]
fn refs_of_several_paths_answers_each_as_alone_and_one_that_names_nothing_fails_the_run() {
    let scenario = board();
    scenario.write(
        "tasks/a.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": "a", "project": "/projects/p"}"#),
    );
    let cache = tempfile::tempdir().unwrap();
    let run = |arguments: &[&str]| board_cached(scenario.workdir(), Some(cache.path()), arguments);
    let admitted = run(&["refs", "/tasks/a", "/projects/p"]);
    assert_eq!(admitted.code, 0);
    assert_eq!(
        text(&admitted.stdout),
        "==> /tasks/a <==\nfrom /project /projects/p ok\n\n==> /projects/p <==\nto /tasks/a /project\n"
    );
    assert_eq!(text(&admitted.stderr), "");
    let refused = run(&["refs", "/tasks/a", "/tasks/zz", "/projects/p"]);
    assert_eq!(refused.code, 1);
    assert_eq!(refused.error.as_deref(), Some("board::path::not_found"));
    assert_eq!(text(&refused.stdout), text(&admitted.stdout));
    let alone = text(&run(&["refs", "/tasks/zz"]).stderr);
    assert_eq!(
        text(&refused.stderr),
        alone.replacen("(index 0)", "(index 1)", 1),
        "the refusal is printed as it is alone, but for the index of its path"
    );
    assert!(
        alone.starts_with("error: nothing is at /tasks/zz\n"),
        "{alone}"
    );
    let ran = run(&[
        "refs",
        "/tasks/a",
        "/tasks/zz",
        "/projects/p",
        "--limit",
        "1",
        "--json",
    ]);
    assert_eq!(
        (ran.code, ran.error.as_deref()),
        (1, Some("board::path::not_found"))
    );
    assert_eq!(json(&ran)["schema"], "board/refs-report/v2");
    let alone: Vec<Value> = ["/tasks/a", "/tasks/zz", "/projects/p"]
        .iter()
        .map(|path| answer(&run(&["refs", path, "--limit", "1", "--json"])))
        .collect();
    let mut merged = answers(&ran);
    assert_eq!(merged[1]["code"], "board::path::not_found");
    // A failure names its key's index in the run, which alone is 0.
    merged[1]["index"] = 0.into();
    assert_eq!(merged, alone);
}

#[test]
fn refs_of_several_refused_paths_names_each_path_and_its_index_in_its_refusal() {
    let scenario = board();
    let cache = tempfile::tempdir().unwrap();
    let run = |arguments: &[&str]| board_cached(scenario.workdir(), Some(cache.path()), arguments);
    let mut expected = String::new();
    for (index, path) in ["/tasks/zz", "/projects/p.json"].iter().enumerate() {
        let alone = text(&run(&["refs", path]).stderr);
        assert!(
            alone.contains(&format!("\n  key:   {path} (index 0)\n")),
            "{alone}"
        );
        expected += &alone.replacen("(index 0)", &format!("(index {index})"), 1);
    }
    let refused = run(&["refs", "/tasks/zz", "/projects/p.json"]);
    assert_eq!(
        (refused.code, refused.error.as_deref()),
        (1, Some("board::path::not_found"))
    );
    assert_eq!(text(&refused.stderr), expected);
    let admitted = run(&["refs", "/projects/p", "/projects/p"]);
    assert_eq!((admitted.code, admitted.error.as_deref()), (0, None));
}

#[test]
fn refs_answers_at_most_max_limit_paths_and_refuses_one_more() {
    let scenario = board();
    scenario.write(
        "tasks/a.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": "a", "project": "/projects/p"}"#),
    );
    let cache = tempfile::tempdir().unwrap();
    let run = |count: usize| {
        let mut arguments = vec!["refs"];
        arguments.extend(std::iter::repeat_n("/tasks/a", count));
        board_cached(scenario.workdir(), Some(cache.path()), &arguments)
    };
    let admitted = run(board_ops::MAX_LIMIT);
    assert_eq!((admitted.code, admitted.error.as_deref()), (0, None));
    let headers = text(&admitted.stdout).matches("==> /tasks/a <==").count();
    assert_eq!(headers, board_ops::MAX_LIMIT);
    let refused = run(board_ops::MAX_LIMIT + 1);
    assert_eq!(
        (refused.code, refused.error.as_deref()),
        (2, Some("board::cli::usage"))
    );
    assert_eq!(text(&refused.stdout), "");
    assert!(
        text(&refused.stderr).starts_with(
            "error: refs was given more than 10000 paths, and one run answers at most 10000\n"
        ),
        "{}",
        text(&refused.stderr)
    );
    assert_eq!(
        hint_of(&refused),
        "split the paths over several runs of at most 10000 each"
    );
}

#[test]
fn refs_of_several_paths_names_each_path_in_its_note_and_limits_each_on_its_own() {
    let scenario = board();
    scenario.write(
        "tasks/a.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": "a", "project": "/projects/p"}"#),
    );
    scenario.write(
        "tasks/b.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": "b", "project": "/projects/p"}"#),
    );
    let cache = tempfile::tempdir().unwrap();
    let output = board_cached(
        scenario.workdir(),
        Some(cache.path()),
        &[
            "refs",
            "/projects/p",
            "/tasks/a",
            "--missing",
            "/tasks/new.md",
            "--limit",
            "1",
        ],
    );
    assert_eq!(output.code, 0);
    assert_eq!(
        text(&output.stdout),
        "==> /projects/p <==\nto /tasks/a /project\n\n==> /tasks/a <==\nfrom /project /projects/p ok\n\n\
         ==> /tasks/new.md <==\n"
    );
    assert_eq!(
        text(&output.stderr),
        "note: /projects/p: showing 1 of 2 references from and to the record; --limit <n> (at most 10000) or --all shows more\n\
         note: /tasks/new.md: nothing is there yet; a file there would be /tasks/new.md, at tasks/new.md\n"
    );
}

#[test]
fn refs_missing_prints_where_a_new_file_would_be_and_without_it_refuses() {
    let scenario = board();
    scenario.write(
        "links/l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects"}"#),
    );
    let refused = answer(&board_in(
        scenario.workdir(),
        &["refs", "/links/l/new.md", "--json"],
    ));
    assert_eq!(refused["code"], "board::path::not_found");
    let report = answer(&board_in(
        scenario.workdir(),
        &["refs", "--missing", "/links/l/new.md", "--json"],
    ));
    assert_eq!(report["location"]["display"], "/projects");
    assert_eq!(
        report["missing"],
        serde_json::json!({"segments": ["new.md"], "path": "projects/new.md", "display": "/projects/new.md"})
    );
    let shown = board_in(
        scenario.workdir(),
        &["refs", "--missing", "/links/l/new.md"],
    );
    assert_eq!(shown.code, 0);
    assert_eq!(
        text(&shown.stderr),
        "note: nothing is there yet; a file there would be /projects/new.md, at projects/new.md\n"
    );
    let present = answer(&board_in(
        scenario.workdir(),
        &["refs", "--missing", "/projects/p", "--json"],
    ));
    assert_eq!(present["missing"], Value::Null);
}

#[test]
fn refs_missing_names_the_mounted_board_a_new_file_would_be_in() {
    let library = Scenario::new();
    let commit = library.commit(&[("docs/a.md", "text")]);
    let scenario = board();
    scenario.write(
        "vendor/lib.json",
        Item::File(&format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            library.path()
        )),
    );
    let mounted = board_in(
        scenario.workdir(),
        &["refs", "--missing", "/vendor/lib/docs/new.md"],
    );
    assert_eq!(mounted.code, 0, "{}", text(&mounted.stderr));
    let repository = std::fs::canonicalize(library.path()).unwrap();
    assert_eq!(
        text(&mounted.stderr),
        format!(
            "note: nothing is there yet; a file there would be /vendor/lib/docs/new.md, at docs/new.md in the board mounted from {} at commit {commit}\n",
            repository.display()
        )
    );
    let own = board_in(scenario.workdir(), &["refs", "--missing", "/docs/new.md"]);
    assert_eq!(
        text(&own.stderr),
        "note: nothing is there yet; a file there would be /docs/new.md, at docs/new.md\n"
    );
}

#[test]
fn refs_missing_refuses_an_existing_record_named_by_its_file_with_a_hint_and_admits_a_new_one() {
    let scenario = board();
    let refused = answer(&board_in(
        scenario.workdir(),
        &["refs", "--missing", "/projects/p.json", "--json"],
    ));
    assert_eq!(refused["code"], "board::path::not_found");
    assert_eq!(
        refused["recovery"]["text"],
        "a logical path leaves out .json: try `board refs /projects/p`"
    );
    let admitted = answer(&board_in(
        scenario.workdir(),
        &["refs", "--missing", "/projects/new.json", "--json"],
    ));
    assert_eq!(admitted["missing"]["path"], "projects/new.json");
}

#[test]
fn put_writes_the_record_on_stdin_and_reports_where() {
    let scenario = board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
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
    assert_eq!(again["schema"], "board/put-report/v2");
    assert_eq!(again["data"]["writes"][0]["previous"], blob.to_string());
    assert_eq!(again["data"]["writes"][0]["display"], "/tasks/a");
    assert_eq!(again["data"]["commit"], Value::Null);
}

#[test]
fn put_notes_a_write_it_leaves_degraded_and_its_clean_twin_has_no_note() {
    let scenario = board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let run = |arguments: &[&str], project: &str| {
        let record = format!(r#"{{"$type": "lr/task/v1", "title": "t", "project": "{project}"}}"#);
        board_with(scenario.workdir(), None, arguments, record.as_bytes())
    };
    let clean = run(&["put", "/tasks/a"], "/projects/p");
    assert_eq!(clean.code, 0);
    assert_eq!(text(&clean.stderr), "");
    let degraded = run(&["put", "/tasks/b"], "/nowhere");
    assert_eq!(degraded.code, 0, "the record is written");
    assert_eq!(
        text(&degraded.stderr),
        "note: /tasks/b is written but degraded: board::ref::dangling /tasks/b holds a reference \
         at \"/project\": nothing is at /nowhere\n"
    );
    let report = json(&run(&["put", "/tasks/b", "--json"], "/nowhere"));
    assert_eq!(
        report["data"]["writes"][0]["degraded"],
        json!({
            "code": "board::ref::dangling",
            "pointer": "/project",
            "reason": r#"holds a reference at "/project": nothing is at /nowhere"#,
            "failures": 1,
        })
    );
    let report = json(&run(&["put", "/tasks/a", "--json"], "/projects/p"));
    assert_eq!(report["data"]["writes"][0]["degraded"], Value::Null);
}

#[test]
fn put_refuses_a_stale_expect_with_exit_one_and_a_malformed_one_with_exit_two() {
    let scenario = board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
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
    assert!(text(&stale.stderr).contains("\n  code:  board::put::stale\n"));
    assert_eq!(run("not-a-blob").code, 2);
    let current = scenario.blob(r#"{"$type": "lr/project/v1"}"#);
    assert_eq!(run(&current.to_string()).code, 0);
}

#[test]
fn a_batch_refused_for_several_reasons_prints_each_line_as_golden_text_and_its_fixed_twin_writes() {
    let scenario = board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let good = r#"{"path": "/tasks/a", "record": {"$type": "lr/task/v1", "title": "a"}}"#;
    let refused = board_with(
        scenario.workdir(),
        None,
        &["put", "--batch"],
        format!("{good}\n{{\n{good}\n{{\"path\": \"/tasks/c\", \"record\": {{\"$type\": \"lr/nope/v1\"}}}}\n")
            .as_bytes(),
    );
    assert_eq!(refused.code, 1);
    assert!(refused.stdout.is_empty());
    assert_eq!(
        text(&refused.stderr),
        "error: the batch is refused\n\
         \x20 help:  fix each line listed, then send the whole batch again; nothing was written\n\
         \x20 code:  board::put::invalid\n\
         \x20 line 2: board::json::parse: json does not parse: EOF while parsing an object at line 1 column 1\n\
         \x20 line 3: board::put::invalid: it writes /tasks/a, the same file as line 1's /tasks/a\n\
         \x20 line 4: board::record::unknown_type: the record for /tasks/c is refused: \
         no schema for lr/nope/v1 at .board/types/lr/nope/v1.schema.json\n"
    );
    let admitted = board_with(
        scenario.workdir(),
        None,
        &["put", "--batch"],
        format!("{good}\n").as_bytes(),
    );
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

#[test]
fn put_batch_writes_every_line_or_lists_the_refused_ones() {
    let scenario = board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let good = r#"{"path": "/tasks/a", "record": {"$type": "lr/task/v1", "title": "a"}}"#;
    let bad = r#"{"path": "/tasks/b", "record": {"$type": "lr/task/v1"}}"#;
    let refused = board_with(
        scenario.workdir(),
        None,
        &["put", "--batch"],
        format!("{good}\n{bad}\n").as_bytes(),
    );
    assert_eq!(refused.code, 1);
    assert_eq!(
        text(&refused.stderr),
        "error: the batch is refused\n\
         \x20 help:  fix each line listed, then send the whole batch again; nothing was written\n\
         \x20 code:  board::put::invalid\n\
         \x20 line 2: board::record::invalid: the record for /tasks/b \
         fails its schema at its root: \"title\" is a required property\n"
    );
    let document = json(&board_with(
        scenario.workdir(),
        None,
        &["put", "--batch", "--json"],
        format!("{good}\n{bad}\n").as_bytes(),
    ));
    assert_eq!(document["schema"], "board/failure/v1");
    assert_eq!(
        document["recovery"]["text"],
        "fix each line listed, then send the whole batch again; nothing was written"
    );
    assert_eq!(document["fields"]["refusals"][0]["line"], 2);
    assert_eq!(
        document["fields"]["refusals"][0]["code"],
        "board::record::invalid"
    );
    assert_eq!(
        document["fields"]["refusals"][0]["fields"]["path"],
        "/tasks/b"
    );
    assert!(!scenario.workdir().join("tasks/a.json").exists());
    let written = board_with(
        scenario.workdir(),
        None,
        &["put", "--batch", "--json"],
        format!("{good}\n").as_bytes(),
    );
    assert_eq!(written.code, 0, "{}", text(&written.stderr));
    assert_eq!(json(&written)["data"]["writes"][0]["display"], "/tasks/a");
}

#[test]
fn put_file_writes_stdin_as_given_and_a_batch_of_a_record_and_files_makes_one_commit() {
    let scenario = board();
    scenario
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        )
        .write(
            ".board/types/lr/task/v1.schema.json",
            Item::File(board_scenario::generated::TASK_SCHEMA),
        );
    let wrote = board_with(
        scenario.workdir(),
        None,
        &["put", "/notes/plan.md", "--file"],
        b"# Plan\n",
    );
    assert_eq!(wrote.code, 0, "{}", text(&wrote.stderr));
    let head = scenario.head().unwrap();
    assert_eq!(
        text(&wrote.stdout),
        format!(
            "wrote /notes/plan.md {}\ncommitted {}\n",
            scenario.blob("# Plan\n"),
            head.id
        )
    );
    assert_eq!(
        text(&board_in(scenario.workdir(), &["get", "/notes/plan.md"]).stdout),
        "# Plan\n"
    );
    let lines = concat!(
        r#"{"path": "/reports/r1", "record": {"$type": "lr/task/v1", "title": "r1", "see": "/reports/r1/notes.md"}}"#,
        "\n",
        r##"{"path": "/reports/r1/notes.md", "text": "# Notes\n"}"##,
        "\n",
        r#"{"path": "/reports/r1/data.csv", "text": "a,b\n"}"#,
        "\n",
        r#"{"path": "/reports/r1/log.txt", "text": "ok\n"}"#,
        "\n",
    );
    let batch = board_with(
        scenario.workdir(),
        None,
        &["put", "--batch"],
        lines.as_bytes(),
    );
    assert_eq!(batch.code, 0, "{}", text(&batch.stderr));
    let head = scenario.head().unwrap();
    assert_eq!(head.message, "board put --batch (1 records, 3 files)");
    assert_eq!(head.parents.len(), 1);
    for (path, bytes) in [
        ("/reports/r1/notes.md", "# Notes\n"),
        ("/reports/r1/data.csv", "a,b\n"),
        ("/reports/r1/log.txt", "ok\n"),
    ] {
        let got = board_in(scenario.workdir(), &["get", path]);
        assert_eq!(text(&got.stdout), bytes, "{path}");
    }
    let refs = board_in(scenario.workdir(), &["refs", "/reports/r1"]);
    assert_eq!(text(&refs.stdout), "from /see /reports/r1/notes.md ok\n");
    let both = board_with(scenario.workdir(), None, &["put", "--batch", "--file"], b"");
    assert_eq!(both.code, 2);
}

#[test]
fn put_needs_a_path_or_batch_and_not_both() {
    let scenario = board();
    for arguments in [
        &["put"][..],
        &["put", "/tasks/a", "--batch"],
        &["put", "--batch", "--expect", "none"],
    ] {
        let output = board_with(scenario.workdir(), None, arguments, b"");
        assert_eq!(output.code, 2, "{arguments:?}");
    }
    assert_eq!(
        board_with(scenario.workdir(), None, &["put", "--batch"], b"").code,
        0
    );
}

#[test]
fn mount_update_reports_the_move_and_then_that_it_is_there() {
    let library = Scenario::new();
    let old = library.commit(&[("a.md", "one")]);
    let new = library.commit(&[("a.md", "two")]);
    let scenario = board();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
        )
        .write("lib.json", Item::File(&mount_of(&library.path(), old)));
    let moved = board_in(scenario.workdir(), &["mount", "update", "/lib"]);
    assert_eq!(moved.code, 0, "{}", text(&moved.stderr));
    assert_eq!(
        text(&moved.stdout),
        format!("updated /lib {old} -> {new}\n")
    );
    let again = json(&board_in(
        scenario.workdir(),
        &["mount", "update", "/lib", "--json"],
    ));
    assert_eq!(again["schema"], "board/mount-update-report/v2");
    assert_eq!(again["data"]["old"], new.to_string());
    assert_eq!(again["data"]["new"], new.to_string());
    assert_eq!(again["data"]["wrote"], false);
}

#[test]
fn mount_update_of_a_missing_path_says_nothing_is_there_and_of_a_mount_runs() {
    let library = Scenario::new();
    let commit = library.commit(&[("a.md", "one")]);
    let scenario = board();
    scenario.write("lib.json", Item::File(&mount_of(&library.path(), commit)));
    let missing = board_in(scenario.workdir(), &["mount", "update", "/nope"]);
    assert_eq!(missing.code, 1);
    assert!(
        text(&missing.stderr).starts_with("error: /nope cannot be written: nothing is at /nope\n"),
        "{}",
        text(&missing.stderr)
    );
    let mounted = board_in(scenario.workdir(), &["mount", "update", "/lib"]);
    assert_eq!(mounted.code, 0, "{}", text(&mounted.stderr));
}

/// A board that commits each put as a test identity.
fn committing() -> Scenario {
    let scenario = board();
    scenario.write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    );
    scenario
}

#[test]
fn type_add_writes_and_commits_the_schema_then_a_record_of_the_type_puts() {
    let scenario = committing();
    let schema = br#"{"type": "object", "required": ["title"]}"#;
    let run =
        |arguments: &[&str], stdin: &[u8]| board_with(scenario.workdir(), None, arguments, stdin);
    let added = run(&["type", "add", "notes/note/v1"], schema);
    assert_eq!(added.code, 0, "{}", text(&added.stderr));
    let blob =
        scenario.blob("{\n  \"required\": [\n    \"title\"\n  ],\n  \"type\": \"object\"\n}\n");
    let head = scenario.head().unwrap();
    assert_eq!(
        text(&added.stdout),
        format!(
            "wrote /.board/types/notes/note/v1.schema {blob}\ncommitted {}\n",
            head.id
        )
    );
    let record = br#"{"$type": "notes/note/v1", "title": "a"}"#;
    let put = run(&["put", "/notes/a"], record);
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
    let again = run(&["type", "add", "notes/note/v1"], schema);
    assert_eq!(again.code, 0, "{}", text(&again.stderr));
    assert_eq!(text(&again.stdout), "");
    assert_eq!(
        text(&again.stderr),
        "note: the board already holds this schema for notes/note/v1, so nothing is written\n"
    );
    let batch = run(&["put", "--batch"], b"");
    assert_eq!(text(&batch.stderr), "", "{}", text(&batch.stdout));
    let json = json(&run(&["type", "add", "notes/note/v1", "--json"], schema));
    assert_eq!(json["schema"], "board/put-report/v2");
    assert_eq!(json["data"]["writes"], json!([]));
    assert_eq!(json["data"]["commit"], Value::Null);
}

#[test]
fn type_add_commits_an_uncommitted_equal_schema_without_saying_the_board_held_it() {
    let scenario = committing();
    let schema = "{\n  \"type\": \"object\"\n}\n";
    scenario.write(".board/types/notes/note/v1.schema.json", Item::File(schema));
    let added = board_with(
        scenario.workdir(),
        None,
        &["type", "add", "notes/note/v1"],
        schema.as_bytes(),
    );
    assert_eq!(added.code, 0, "{}", text(&added.stderr));
    let head = scenario.head().unwrap();
    assert_eq!(text(&added.stdout), format!("committed {}\n", head.id));
    assert_eq!(text(&added.stderr), "");
}

#[test]
fn type_add_reads_a_schema_of_the_record_limit_and_refuses_one_byte_more() {
    let scenario = committing();
    let sized = |bytes: usize| {
        let padding = bytes - r#"{"description": ""}"#.len();
        format!(r#"{{"description": "{}"}}"#, "a".repeat(padding))
    };
    let run = |record_type: &str, schema: &str| {
        board_with(
            scenario.workdir(),
            None,
            &["type", "add", record_type],
            schema.as_bytes(),
        )
    };
    let over = run("notes/note/v1", &sized(board_model::MAX_RECORD_BYTES + 1));
    assert_eq!(over.code, 1);
    assert!(
        text(&over.stderr).ends_with("\n  code:  board::json::too_large\n"),
        "{}",
        text(&over.stderr)
    );
    let at = run("notes/note/v1", &sized(board_model::MAX_RECORD_BYTES));
    assert_eq!(at.code, 0, "{}", text(&at.stderr));
}

#[test]
fn type_add_refuses_a_changed_schema_and_admits_it_as_the_next_version() {
    let scenario = committing();
    let changed = br#"{"type": "object", "required": ["body"]}"#;
    let run = |arguments: &[&str]| board_with(scenario.workdir(), None, arguments, changed);
    let refused = run(&["type", "add", "lr/task/v1"]);
    assert_eq!(refused.code, 1);
    assert_eq!(
        text(&refused.stderr),
        "error: the board already holds another schema for lr/task/v1 at \
         .board/types/lr/task/v1.schema.json\n  help:  a changed type is a new version: add this \
         schema as the next one, such as v2 after v1, and move its records there; a type not yet \
         released may instead be edited by hand\n  code:  \
         board::schema::differs\n"
    );
    let admitted = run(&["type", "add", "lr/task/v2"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

#[test]
fn type_add_refuses_the_board_namespace_and_a_malformed_type() {
    let scenario = committing();
    let schema = br#"{"type": "object"}"#;
    let run = |arguments: &[&str]| board_with(scenario.workdir(), None, arguments, schema);
    let reserved = run(&["type", "add", "board/note/v1"]);
    assert_eq!(reserved.code, 1);
    assert!(
        text(&reserved.stderr).contains("\n  code:  board::schema::invalid\n"),
        "{}",
        text(&reserved.stderr)
    );
    let malformed = run(&["type", "add", "Notes/note/v1"]);
    assert_eq!(malformed.code, 2, "{}", text(&malformed.stderr));
    let admitted = run(&["type", "add", "notes/note/v1"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

mod cli;
mod complete;
mod consistency;
mod directory;
mod flags;
mod get;
mod help;
mod hints;
mod known;
mod plugins;
mod query;
mod recoveries;
mod refs;
mod refused;
mod relative;
mod released;
mod sync;
