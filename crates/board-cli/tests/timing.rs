//! Every run of the `board` binary appends one record to the timing log in
//! its cache directory, unless `BOARD_TIMINGS` is `0`, and a log that cannot
//! be written changes nothing the run prints (docs/architecture/performance/timing.md).
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "only the binary's own main records timings, so the test runs it as a child process"
)]

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use board_scenario::{Item, Scenario};
use serde_json::Value;

/// SIGTERM's number on macOS and Linux.
const SIGTERM: i32 = 15;

fn board() -> Scenario {
    let scenario = Scenario::board();
    scenario
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
    for step in ["load", "walk", "sync", "facts"] {
        let name = format!("index.refresh.{step}");
        assert!(named.contains(&name.as_str()), "{name} in {named:?}");
    }
    assert!(
        named.contains(&"snapshot.head"),
        "snapshot.head in {named:?}"
    );
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

/// Calls `available` of a `chores` plugin that replies with an empty output,
/// with `arguments` after `call`, in a board that enables it, with `{}` on
/// stdin.
fn call_chores(home: &Path, arguments: &[&str]) {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "plugins": ["chores"]}"#),
    );
    let manifest = serde_json::json!({
        "$type": "board/plugin-manifest/v1",
        "name": "chores",
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
        "board/plugins/chores/plugin.json",
        Item::File(&manifest.to_string()),
    )
    .write(
        "board/plugins/chores/run",
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
    call_chores(home.path(), &["chores.available"]);
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

/// Runs `skills list`, which opens no index, with a timing log and no tag in
/// a cache root that is unwritable when `locked`, and gives its exit code
/// and stderr.
fn skills_under_cache_root(home: &Path, locked: bool) -> (Option<i32>, String) {
    let scenario = board();
    let root = home.join("cache/board");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("timings.jsonl"), b"").unwrap();
    let mode = |mode| std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(&root, mode(if locked { 0o500 } else { 0o700 })).unwrap();
    let output = run(&scenario, home, None, &["skills", "list"]);
    std::fs::set_permissions(&root, mode(0o700)).unwrap();
    assert_eq!(records(home).len(), 1);
    let stderr = String::from_utf8(output.stderr).unwrap();
    (output.status.code(), stderr)
}

#[test]
fn appending_the_timing_line_under_an_unwritable_cache_root_warns_that_it_is_unmarked() {
    let home = tempfile::tempdir().unwrap();
    let (code, stderr) = skills_under_cache_root(home.path(), true);
    assert_eq!(code, Some(0));
    let root = home.path().join("cache/board");
    // The error's text differs by platform: Linux names the temporary file.
    let (message, hint) = stderr.split_once("; ").unwrap();
    assert!(
        message.starts_with(&format!(
            "warning: board::cache::unmarked: the cache directory {} is not marked as a \
             cache, so backups may copy it: Permission denied (os error 13)",
            root.display()
        )),
        "{stderr}"
    );
    assert_eq!(
        hint,
        "Board marks the directory with a CACHEDIR.TAG file and, on macOS, a Time Machine \
         exclusion; fix what stopped it, such as the directory's permissions, and the next \
         run marks it\n"
    );
}

#[test]
fn appending_the_timing_line_under_a_writable_cache_root_warns_nothing() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(
        skills_under_cache_root(home.path(), false),
        (Some(0), String::new())
    );
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
    call_chores(home.path(), &["chores.available", "--repo", "secret"]);
    let record = &records(home.path())[0];
    assert_eq!(record["command"], "call", "{record}");
    assert_eq!(record["capability"], "chores.available", "{record}");
    let text = std::fs::read_to_string(log(home.path())).unwrap();
    assert!(!text.contains("secret"), "{text}");
}

#[test]
fn a_record_names_the_machines_load_and_cores_and_that_the_run_was_not_killed() {
    let scenario = board();
    let home = tempfile::tempdir().unwrap();
    run(&scenario, home.path(), None, &["get", "/notes/a"]);
    let record = &records(home.path())[0];
    assert_eq!(record["killed"], false);
    assert!(record["cpus"].as_u64().unwrap() >= 1, "{record}");
    assert!(record["load1"].as_f64().unwrap() >= 0.0, "{record}");
}

/// The line a run prints to stderr once its signal handler is installed,
/// when `BOARD_TIMING_READY` is `1`.
const READY: &str = "board: timing handler ready";

#[test]
fn a_run_says_its_handler_is_ready_only_when_board_timing_ready_is_1() {
    let scenario = board();
    let home = tempfile::tempdir().unwrap();
    for (value, says) in [(None, false), (Some("0"), false), (Some("1"), true)] {
        let mut command = command(&scenario, home.path(), &["get", "/notes/a"]);
        if let Some(value) = value {
            command.env("BOARD_TIMING_READY", value);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(
            stderr.lines().filter(|line| *line == READY).count(),
            usize::from(says),
            "{value:?}: {stderr}"
        );
    }
}

/// Reads `stderr` on a thread until it prints [`READY`], and fails the
/// test when that takes over 30 seconds or the run closes stderr first.
fn await_ready(stderr: std::process::ChildStderr) {
    let (said, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead as _;
        let ready = std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
            .any(|line| line == READY);
        let _ = said.send(ready);
    });
    let ready = heard
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("the put did not print {READY:?} within 30 seconds"));
    assert!(ready, "the put closed stderr without printing {READY:?}");
}

#[test]
fn a_run_sent_sigterm_writes_one_line_that_says_it_was_killed() {
    let scenario = board();
    let home = tempfile::tempdir().unwrap();
    // A put waits for its record on stdin. The test holds the stdin it took
    // out of the child until the put has exited, so the put is still
    // waiting when the signal lands.
    let mut child = command(&scenario, home.path(), &["put", "/notes/c"])
        .env("BOARD_TIMING_READY", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    await_ready(child.stderr.take().unwrap());
    // The handler is installed. The wait gives the run a known least time.
    std::thread::sleep(std::time::Duration::from_millis(200));
    let sent = Command::new("kill")
        .args(["-s", "TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    // The loop bounds the wait: a put the signal never ends fails the test
    // after 30 seconds instead of holding it until CI's time limit.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            panic!("the put was still running 30 seconds after SIGTERM");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    drop(stdin);
    assert_eq!(status.signal(), Some(SIGTERM), "{status}");
    assert!(log(home.path()).exists(), "the run wrote no timing line");
    let records = records(home.path());
    assert_eq!(records.len(), 1, "{records:?}");
    let killed = &records[0];
    assert_eq!(
        (&killed["command"], &killed["killed"], &killed["exit"]),
        (&"put".into(), &true.into(), &Value::Null)
    );
    assert!(killed["total_us"].as_u64().unwrap() >= 200_000, "{killed}");
}

/// A personal board that holds the schemas of the runs the check writes,
/// and a config directory under `home` that names it.
fn personal(home: &Path) -> Scenario {
    let scenario = Scenario::board();
    for (path, schema) in [
        (
            "runs/run/v1",
            include_str!("../../board-plugin-runs/types/runs/run/v1.schema.json"),
        ),
        (
            "runs/suite/v1",
            include_str!("../../board-plugin-runs/types/runs/suite/v1.schema.json"),
        ),
    ] {
        scenario.write(
            &format!(".board/types/{path}.schema.json"),
            Item::File(schema),
        );
    }
    scenario.identified();
    let known = serde_json::json!({"personal": scenario.path()}).to_string();
    std::fs::create_dir_all(home.join("config/board")).unwrap();
    std::fs::write(home.join("config/board/config.json"), known).unwrap();
    scenario
}

fn check(scenario: &Scenario, home: &Path, arguments: &[&str]) -> Value {
    let ran = command(scenario, home, arguments)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .output()
        .unwrap();
    assert!(ran.status.success(), "{ran:?}");
    serde_json::from_slice::<Value>(&ran.stdout).unwrap()["data"].clone()
}

#[test]
fn a_recorded_check_writes_the_suite_and_a_run_once_an_hour_and_a_check_without_record_writes_none()
{
    let home = tempfile::tempdir().unwrap();
    let scenario = personal(home.path());
    for _ in 0..5 {
        run(&scenario, home.path(), None, &["get", "/missing"]);
    }
    let printed = check(&scenario, home.path(), &["--json", "timing", "check"]);
    assert_eq!(printed["recorded"], Value::Null);
    assert_eq!(printed["live"], serde_json::json!([]));
    assert_eq!(printed["commands"][0]["mode"], "silent");
    let get = &printed["commands"][0];
    assert_eq!(
        (&get["command"], &get["budget_ms"]),
        (&"get".into(), &25.into())
    );
    assert!(!scenario.workdir().join("suites").exists());

    let recorded = check(
        &scenario,
        home.path(),
        &["--json", "timing", "check", "--record"],
    );
    let run = recorded["recorded"].as_str().unwrap();
    assert!(run.starts_with("/suites/board-speed-local/runs/"), "{run}");
    let suite: Value = serde_json::from_slice(
        &std::fs::read(scenario.workdir().join("suites/board-speed-local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(suite["kind"], "benchmark");
    let written: Value = serde_json::from_slice(
        &std::fs::read(scenario.workdir().join(format!("{}.json", &run[1..]))).unwrap(),
    )
    .unwrap();
    assert_eq!(written["results"].as_array().unwrap().len(), 14);
    assert!(written["measures"]["over_budget"].is_u64(), "{written}");
    let setup = written["setup"].as_str().unwrap();
    assert!(
        setup.starts_with("board timing check --record on the machine "),
        "{setup}"
    );

    let again = check(
        &scenario,
        home.path(),
        &["--json", "timing", "check", "--record"],
    );
    assert_eq!(again["recorded"], Value::Null);
    let runs = std::fs::read_dir(scenario.workdir().join("suites/board-speed-local/runs"));
    assert_eq!(runs.unwrap().count(), 1);
}

#[test]
fn a_check_refuses_a_session_that_is_not_a_local_id_and_a_session_without_record() {
    let home = tempfile::tempdir().unwrap();
    let scenario = personal(home.path());
    for arguments in [
        &["timing", "check", "--record", "--session", "local_0a; rm"][..],
        &["timing", "check", "--session", "local_0a1b"][..],
    ] {
        let ran = command(&scenario, home.path(), arguments)
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .output()
            .unwrap();
        assert_eq!(ran.status.code(), Some(2), "{ran:?}");
    }
}

/// The line the rules mod runs under /bin/sh at a session's start, from
/// `SPEED_CHECK` in plugins/rules/claude-code/hooks/rules.ts.
fn speed_check_line() -> String {
    let source = include_str!("../../../plugins/rules/claude-code/hooks/rules.ts");
    let declared = source.find("SPEED_CHECK =").unwrap();
    let start = declared + source[declared..].find('\'').unwrap() + 1;
    let length = source[start..].find('\'').unwrap();
    source[start..start + length].to_owned()
}

#[test]
fn the_speed_check_the_rules_mod_starts_outlives_its_shell_and_the_hooks_process_group() {
    let home = tempfile::tempdir().unwrap();
    let scenario = personal(home.path());
    for _ in 0..5 {
        run(&scenario, home.path(), None, &["get", "/missing"]);
    }
    // A `board` on PATH that waits until the hook's group is signalled,
    // then runs the real check.
    let bin = tempfile::tempdir().unwrap();
    let script = format!(
        "#!/bin/sh\nsleep 2\nexec '{}' \"$@\"\n",
        env!("CARGO_BIN_EXE_board")
    );
    std::fs::write(bin.path().join("board"), script).unwrap();
    std::fs::set_permissions(
        bin.path().join("board"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.path().display());
    let mut shell = Command::new("/bin/sh");
    shell
        .args(["-c", &speed_check_line()])
        .current_dir(scenario.workdir())
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("PATH", path);
    std::os::unix::process::CommandExt::process_group(&mut shell, 0);
    let mut child = shell.spawn().unwrap();
    let group = child.id();
    assert!(child.wait().unwrap().success());
    // The hook's process group ends as a host may end it.
    for signal in ["HUP", "TERM"] {
        Command::new("kill")
            .args(["-s", signal, "--", &format!("-{group}")])
            .status()
            .unwrap();
    }
    let runs = scenario.workdir().join("suites/board-speed-local/runs");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !runs.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(std::fs::read_dir(runs).map_or(0, Iterator::count), 1);
}
