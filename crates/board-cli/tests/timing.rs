//! Every run of the `board` binary appends one record to the timing log in
//! its cache directory, unless `BOARD_TIMINGS` is `0`, and a log that cannot
//! be written changes nothing the run prints (docs/architecture/performance/timing.md).
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "only the binary's own main records timings, so the test runs it as a child process"
)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use board_scenario::{Item, Scenario};
use serde_json::Value;

fn board() -> Scenario {
    let scenario = Scenario::new();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(
            ".board/types/lr/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        )
        .write("notes/a.json", Item::File(r#"{"$type": "lr/note/v1"}"#));
    scenario
}

/// Runs `board` in `scenario` with `home` as its home and its cache
/// directory under it, so no record reaches the developer's own log.
fn run(scenario: &Scenario, home: &Path, timings: Option<&str>, arguments: &[&str]) -> Output {
    let mut command = command(scenario, home, arguments);
    if let Some(timings) = timings {
        command.env("BOARD_TIMINGS", timings);
    }
    command.output().unwrap()
}

fn command(scenario: &Scenario, home: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board"));
    command
        .args(arguments)
        .current_dir(scenario.workdir())
        .env_clear()
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"));
    command
}

fn log(home: &Path) -> PathBuf {
    home.join("cache/board/timings.jsonl")
}

fn records(home: &Path) -> Vec<Value> {
    std::fs::read_to_string(log(home))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_run_appends_a_record_of_its_command_and_phases_and_a_failed_run_its_error_code() {
    let scenario = board();
    let home = tempfile::tempdir().unwrap();
    assert!(
        run(&scenario, home.path(), None, &["get", "/notes/a"])
            .status
            .success()
    );
    let failed = run(&scenario, home.path(), None, &["get", "/notes/b"]);
    assert_eq!(failed.status.code(), Some(1));

    let records = records(home.path());
    assert_eq!(records.len(), 2);
    let (found, missing) = (&records[0], &records[1]);
    assert_eq!(found["$type"], "board/timing/v1");
    assert_eq!(found["command"], "get");
    assert_eq!(found["depth"], 0);
    let root = std::fs::canonicalize(scenario.workdir()).unwrap();
    assert_eq!(found["root"], root.display().to_string());
    assert_eq!((&found["exit"], &found["error"]), (&0.into(), &Value::Null));
    assert!(found["total_us"].as_u64().unwrap() > 0);
    assert!(found["at_ms"].as_u64().unwrap() > 0);
    let version = found["version"].as_str().unwrap();
    let build = version.strip_prefix(env!("CARGO_PKG_VERSION")).unwrap();
    // A build outside a git checkout has no suffix.
    if let Some(build) = build.strip_prefix('+') {
        let (commit, changes) = build.split_once(".dirty.").unwrap_or((build, "0"));
        assert_eq!(commit.len(), 12, "{version}");
        let hex = |text: &str| text.bytes().all(|byte| byte.is_ascii_hexdigit());
        assert!(hex(commit) && hex(changes), "{version}");
    } else {
        assert_eq!(build, "", "{version}");
    }
    assert_eq!(found["capability"], Value::Null);
    let phases: Vec<&str> = found["phases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|phase| phase["name"].as_str().unwrap())
        .collect();
    assert_eq!(phases, ["parse", "open", "resolve", "emit"]);
    assert_eq!(missing["exit"], 1);
    assert_eq!(missing["error"], "board::path::not_found");

    let mode = std::fs::metadata(log(home.path()))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "only its owner may read the log");
}

#[test]
fn a_check_records_the_work_it_did_and_the_entries_it_decided() {
    let scenario = board();
    let home = tempfile::tempdir().unwrap();
    run(&scenario, home.path(), None, &["check"]);
    run(&scenario, home.path(), None, &["check"]);
    let records = records(home.path());
    let (cold, warm) = (&records[0]["work"], &records[1]["work"]);
    assert_eq!(cold["entries"], 3);
    assert_eq!(cold["parsed"], 3);
    assert_eq!(warm["entries"], 3);
    assert_eq!(warm["parsed"], 0, "a warm check parses nothing again");
}

#[test]
fn a_check_records_each_step_of_its_refresh() {
    let scenario = board();
    let home = tempfile::tempdir().unwrap();
    run(&scenario, home.path(), None, &["check"]);
    let records = records(home.path());
    let named: Vec<&str> = (records[0]["phases"].as_array().unwrap().iter())
        .map(|phase| phase["name"].as_str().unwrap())
        .collect();
    for step in ["load", "walk", "sync", "facts", "head"] {
        let name = format!("index.refresh.{step}");
        assert!(named.contains(&name.as_str()), "{name} in {named:?}");
    }
}

/// When the phase `name` of `record` started and ended, in microseconds
/// after the run started.
fn span(record: &Value, name: &str) -> (u64, u64) {
    let phases = record["phases"].as_array().unwrap();
    let Some(phase) = phases.iter().find(|phase| phase["name"] == name) else {
        panic!("no {name} phase in {record}");
    };
    let at = phase["at_us"].as_u64().unwrap();
    (at, at + phase["us"].as_u64().unwrap())
}

#[test]
fn a_scoped_check_resolves_within_its_snapshot_and_refs_resolves_before_opening_the_index() {
    let scenario = board();
    let home = tempfile::tempdir().unwrap();
    run(&scenario, home.path(), None, &["check", "/notes"]);
    run(&scenario, home.path(), None, &["refs", "/notes/a"]);
    let records = records(home.path());
    let (resolved, snapshot) = (span(&records[0], "resolve"), span(&records[0], "snapshot"));
    // Each start and length is cut to whole microseconds, so a computed end
    // can be one microsecond short.
    assert!(
        snapshot.0 <= resolved.0 && resolved.1 <= snapshot.1 + 1,
        "{}",
        records[0]
    );
    let (resolved, opened) = (
        span(&records[1], "resolve"),
        span(&records[1], "index.open"),
    );
    assert!(resolved.1 <= opened.0, "{}", records[1]);
}

/// Calls `available` of a `tasks` plugin that replies with an empty output,
/// with `arguments` after `call`, in a board that enables it, with `{}` on
/// stdin.
fn call_tasks(home: &Path, arguments: &[&str]) {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": ["tasks"]}"#),
    );
    let manifest = serde_json::json!({
        "$type": "board/plugin-manifest/v1",
        "name": "tasks",
        "version": "1.0.0",
        "description": "",
        "run": ["sh", "./run"],
        "capabilities": [{
            "name": "available",
            "description": "",
            "inputSchema": {"type": "object", "properties": {"repo": {"type": "string"}}},
            "outputSchema": {"type": "object"}
        }]
    });
    let data = Scenario::new();
    data.write(
        "board/plugins/tasks/plugin.json",
        Item::File(&manifest.to_string()),
    )
    .write(
        "board/plugins/tasks/run",
        Item::File(
            "cat > /dev/null\nprintf '%s' '{\"$type\": \"board/call-result/v1\", \"output\": {}}'\n",
        ),
    )
    .write("input.json", Item::File("{}"));
    let called = command(&scenario, home, &[&["call"][..], arguments].concat())
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_DATA_HOME", data.workdir())
        .stdin(std::fs::File::open(data.workdir().join("input.json")).unwrap())
        .output()
        .unwrap();
    assert!(called.status.success(), "{called:?}");
}

#[test]
fn a_call_records_the_plugin_process_it_started_and_the_time_to_its_reply() {
    let home = tempfile::tempdir().unwrap();
    call_tasks(home.path(), &["tasks.available"]);
    let record = &records(home.path())[0];
    assert_eq!(record["work"]["started"], 1, "{record}");
    let (reply, call) = (span(record, "plugin.reply"), span(record, "plugin.call"));
    // As in the scoped check above, a computed end can be one microsecond
    // short.
    assert!(call.0 <= reply.0 && reply.1 <= call.1 + 1, "{record}");
}

#[test]
fn a_record_keeps_the_subcommands_run_and_none_of_the_values() {
    let scenario = board();
    let home = tempfile::tempdir().unwrap();
    run(&scenario, home.path(), None, &["get", "/notes/secret"]);
    run(
        &scenario,
        home.path(),
        None,
        &["mount", "update", "/secret"],
    );
    let records = records(home.path());
    assert_eq!(records[0]["command"], "get");
    assert_eq!(records[1]["command"], "mount update");
    let text = std::fs::read_to_string(log(home.path())).unwrap();
    assert!(!text.contains("secret"), "{text}");
}

#[test]
fn help_the_version_and_a_refused_command_line_record_nothing_and_a_parsed_one_records() {
    let scenario = board();
    let home = tempfile::tempdir().unwrap();
    for arguments in [
        &["check", "--help"][..],
        &["help", "check"],
        &["--version"],
        &["check", "--bogus"],
    ] {
        run(&scenario, home.path(), None, arguments);
        assert!(!log(home.path()).exists(), "{arguments:?}");
    }
    run(&scenario, home.path(), None, &["check"]);
    assert_eq!(records(home.path()).len(), 1);
}

#[test]
fn board_timings_0_records_nothing_and_any_other_value_records_as_unset_does() {
    let scenario = board();
    let off = tempfile::tempdir().unwrap();
    assert!(
        run(&scenario, off.path(), Some("0"), &["get", "/notes/a"])
            .status
            .success()
    );
    assert!(!log(off.path()).exists());

    let on = tempfile::tempdir().unwrap();
    assert!(
        run(&scenario, on.path(), Some("1"), &["get", "/notes/a"])
            .status
            .success()
    );
    assert_eq!(records(on.path()).len(), 1);
}

#[test]
fn a_log_that_cannot_be_written_leaves_output_and_exit_code_as_a_writable_log_does() {
    let scenario = board();
    let writable = tempfile::tempdir().unwrap();
    let blocked = tempfile::tempdir().unwrap();
    // A directory where the log belongs: the cache directory is usable,
    // the log is not.
    std::fs::create_dir_all(log(blocked.path())).unwrap();
    for arguments in [
        &["get", "/notes/a"][..],
        &["get", "/notes/b", "--json"],
        &["check"],
    ] {
        let written = run(&scenario, writable.path(), None, arguments);
        let dropped = run(&scenario, blocked.path(), None, arguments);
        assert_eq!(
            dropped.status.code(),
            written.status.code(),
            "{arguments:?}"
        );
        assert_eq!(dropped.stdout, written.stdout, "{arguments:?}");
        assert_eq!(dropped.stderr, written.stderr, "{arguments:?}");
    }
    assert_eq!(records(writable.path()).len(), 3);
    assert!(log(blocked.path()).is_dir());
}

#[test]
fn a_call_records_the_capability_it_ran_and_none_of_its_argument_values() {
    let home = tempfile::tempdir().unwrap();
    call_tasks(home.path(), &["tasks.available", "--repo", "secret"]);
    let record = &records(home.path())[0];
    assert_eq!(record["command"], "call", "{record}");
    assert_eq!(record["capability"], "tasks.available", "{record}");
    let text = std::fs::read_to_string(log(home.path())).unwrap();
    assert!(!text.contains("secret"), "{text}");
}
