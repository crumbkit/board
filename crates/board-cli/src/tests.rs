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
        environment,
        complete: None,
        cursor: None,
    };
    let arguments = std::iter::once(BINARY_NAME)
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

/// The hint line of a text error.
fn hint_of(output: &Output) -> String {
    let stderr = text(&output.stderr);
    stderr
        .lines()
        .find_map(|line| line.strip_prefix("hint: "))
        .unwrap_or_default()
        .to_owned()
}

fn code_named(name: &str) -> Code {
    Code::ALL
        .into_iter()
        .find(|code| code.as_str() == name)
        .unwrap_or_else(|| panic!("{name} is not a code"))
}

/// Every code in a JSON report: an error's, or each degraded entry's or
/// plugin's.
fn codes(output: &Output) -> Vec<Code> {
    let document = json(output);
    if let Some(code) = document.get("code").and_then(Value::as_str) {
        return vec![code_named(code)];
    }
    let listed = |key: &str| document[key].as_array().into_iter().flatten();
    listed("entries")
        .chain(listed("plugins"))
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
        report["entries"]
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
    let report = json(&board_in(scenario.workdir(), &["get", "/l", "--json"]));
    assert_eq!(report["$type"], "board/get-report/v1");
    assert_eq!(report["record"]["name"], "q");
    assert_eq!(report["location"]["display"], "/projects/q");
    assert_eq!(report["location"]["path"], "projects/q.json");
    assert_eq!(report["location"]["source"], "worktree");
    assert_eq!(report["location"]["pointers"][0]["path"], "l.json");
}

#[test]
fn get_json_refuses_bytes_that_do_not_parse_with_a_parse_error_and_admits_its_parsed_twin() {
    let scenario = board();
    scenario
        .write("torn.json", Item::File(r#"{"$type": "lr/project/v1", "#))
        .write("whole.json", Item::File(r#"{"$type": "lr/project/v1"}"#));
    let refused = board_in(scenario.workdir(), &["get", "/torn", "--json"]);
    assert_eq!(refused.code, 1);
    let error = json(&refused);
    assert_eq!(error["$type"], "board/error/v1");
    assert_eq!(error["code"], "board::json::parse");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );
    assert!(error["hint"].is_string());
    assert_eq!(error["fields"]["path"], "/torn");
    assert!(error["fields"]["blob"].is_string());
    let admitted = board_in(scenario.workdir(), &["get", "/whole", "--json"]);
    assert_eq!(admitted.code, 0);
    assert_eq!(json(&admitted)["$type"], "board/get-report/v1");
}

#[test]
fn get_prints_a_files_bytes_and_json_refuses_it_with_why_it_is_not_a_record() {
    let scenario = board();
    scenario.write("notes.md", Item::File("# n\n"));
    let plain = board_in(scenario.workdir(), &["get", "/notes.md"]);
    assert_eq!((plain.code, text(&plain.stdout)), (0, "# n\n".to_owned()));
    let document = board_in(scenario.workdir(), &["get", "/notes.md", "--json"]);
    assert_eq!(document.code, 1);
    assert_eq!(json(&document)["code"], "board::record::not_a_record");
    scenario.write("bad.json", Item::File(r#"{"$type": "task"}"#));
    let broken = board_in(scenario.workdir(), &["get", "/bad"]);
    assert_eq!(broken.code, 0);
    let broken = board_in(scenario.workdir(), &["get", "/bad", "--json"]);
    assert_eq!(json(&broken)["code"], "board::record::bad_type");
    let record = board_in(scenario.workdir(), &["get", "/projects/p", "--json"]);
    assert_eq!(record.code, 0);
    assert_eq!(json(&record)["$type"], "board/get-report/v1");
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
    assert_eq!(codes(&get), [Code::PathShadowed]);
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
    assert!(text(&created.stdout).contains("next: run `board check`"));
    assert_eq!(board_in(scenario.workdir(), &["check"]).code, 0);
    let again = json(&board_in(scenario.workdir(), &["init", "--json"]));
    assert_eq!(again["$type"], "board/init-report/v1");
    assert_eq!(again["created"], false);
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
    let hint = refused["hint"].as_str().unwrap();
    assert!(hint.starts_with("run `git init` first"), "{hint}");
    assert!(!hint.contains("board init"), "{hint}");
    let repository = Scenario::new();
    let refused = json(&board_in(repository.workdir(), &["check", "--json"]));
    assert_eq!(refused["code"], "board::config::not_found");
    let hint = refused["hint"].as_str().unwrap();
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
        "error: board::path::not_found: nothing is at /missing\n\
         hint: run `board ls /` to see what is there; paths leave out `.json`\n"
    );
    let document = board_in(scenario.workdir(), &["get", "/missing", "--json"]);
    assert_eq!(document.code, 1);
    let error = json(&document);
    assert_eq!(error["$type"], "board/error/v1");
    assert_eq!(error["code"], "board::path::not_found");
    assert_eq!(
        error["hint"],
        Code::PathNotFound.hint(&Fields {
            ancestor: Some(LogicalPath::root()),
            ..Fields::at(&"/missing".parse().unwrap())
        })
    );
}

/// Every entry `check --json` reports, as an exhibit, sorted by path.
fn exhibits(report: &Value) -> Vec<Exhibit> {
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let reported = report["entries"]
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
    assert_eq!(warm["entries"], cold["entries"]);
    assert_eq!(memory["entries"], cold["entries"]);
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
    let report = json(&board_in(
        scenario.workdir(),
        &["refs", "/projects/p", "--json"],
    ));
    assert_eq!(report["$type"], "board/refs-report/v1");
    assert_eq!(report["to"][0]["pointers"][0], "/project");
}

#[test]
fn refs_missing_prints_where_a_new_file_would_be_and_without_it_refuses() {
    let scenario = board();
    scenario.write(
        "links/l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects"}"#),
    );
    let refused = json(&board_in(
        scenario.workdir(),
        &["refs", "/links/l/new.md", "--json"],
    ));
    assert_eq!(refused["code"], "board::path::not_found");
    let report = json(&board_in(
        scenario.workdir(),
        &["refs", "--missing", "/links/l/new.md", "--json"],
    ));
    assert_eq!(report["$type"], "board/refs-report/v1");
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
    let present = json(&board_in(
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
    let refused = json(&board_in(
        scenario.workdir(),
        &["refs", "--missing", "/projects/p.json", "--json"],
    ));
    assert_eq!(refused["code"], "board::path::not_found");
    assert_eq!(
        refused["hint"],
        "a logical path leaves out .json: try `board refs /projects/p`"
    );
    let admitted = json(&board_in(
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
    assert_eq!(again["$type"], "board/put-report/v1");
    assert_eq!(again["writes"][0]["previous"], blob.to_string());
    assert_eq!(again["writes"][0]["display"], "/tasks/a");
    assert_eq!(again["commit"], Value::Null);
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
        report["writes"][0]["degraded"],
        json!({
            "code": "board::ref::dangling",
            "pointer": "/project",
            "reason": r#"holds a reference at "/project": nothing is at /nowhere"#,
            "failures": 1,
        })
    );
    let report = json(&run(&["put", "/tasks/a", "--json"], "/projects/p"));
    assert_eq!(report["writes"][0]["degraded"], Value::Null);
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
    assert!(text(&stale.stderr).starts_with("error: board::put::stale"));
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
        "error: board::put::invalid: the batch is refused\n\
         line 2: board::json::parse: json does not parse: EOF while parsing an object at line 1 column 1\n\
         line 3: board::put::invalid: it writes tasks/a.json as line 1 writes tasks/a.json\n\
         line 4: board::record::unknown_type: the record for /tasks/c is refused: \
         no schema for lr/nope/v1 at .board/types/lr/nope/v1.schema.json\n\
         hint: fix each line listed, then send the whole batch again; nothing was written\n"
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
        "error: board::put::invalid: the batch is refused\n\
         line 2: board::record::invalid: the record for /tasks/b \
         fails its schema at its root: \"title\" is a required property\n\
         hint: fix each line listed, then send the whole batch again; nothing was written\n"
    );
    let document = json(&board_with(
        scenario.workdir(),
        None,
        &["put", "--batch", "--json"],
        format!("{good}\n{bad}\n").as_bytes(),
    ));
    assert_eq!(document["$type"], "board/error/v1");
    assert_eq!(
        document["hint"],
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
    assert_eq!(json(&written)["writes"][0]["display"], "/tasks/a");
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
    assert_eq!(again["$type"], "board/mount-update-report/v1");
    assert_eq!(again["old"], new.to_string());
    assert_eq!(again["new"], new.to_string());
    assert_eq!(again["wrote"], false);
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
        text(&missing.stderr).starts_with(
            "error: board::path::not_found: /nope cannot be written: nothing is at /nope\n"
        ),
        "{}",
        text(&missing.stderr)
    );
    let mounted = board_in(scenario.workdir(), &["mount", "update", "/lib"]);
    assert_eq!(mounted.code, 0, "{}", text(&mounted.stderr));
}

mod cli;
mod complete;
mod consistency;
mod dead_ends;
mod directory;
mod hints;
mod plugins;
mod refused;
