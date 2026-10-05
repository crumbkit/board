use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use board_model::Code;
use board_plugin::{CallReport, Environment, Limits, Target};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::tests::open;
use crate::{CallFailed, Plugins};

/// Takes a number and answers it.
const INPUT_SCHEMA: &str =
    r#"{"type": "object", "required": ["n"], "properties": {"n": {"type": "integer"}}}"#;
/// Answers a count.
const OUTPUT_SCHEMA: &str =
    r#"{"type": "object", "required": ["count"], "properties": {"count": {"type": "integer"}}}"#;

/// A script that records its request, environment and directory, then
/// answers a count.
const RECORDING: &str = r#"#!/bin/sh
cat > request.json
env > environment.txt
pwd -P > directory.txt
printf '%s' '{"$type": "board/call-result/v1", "output": {"count": 2}}'
"#;

/// A board, a plugins directory and a cache.
struct Fixture {
    board: Scenario,
    data: Scenario,
    cache: tempfile::TempDir,
}

impl Fixture {
    /// A board enabling `enabled`.
    fn new(enabled: &[&str]) -> Self {
        let board = Scenario::new();
        board.write(
            ".board/board.json",
            Item::File(&json!({"$type": "board/config/v1", "plugins": enabled}).to_string()),
        );
        Self {
            board,
            data: Scenario::new(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    /// Installs `name`, offering `capabilities`, whose `run` script is
    /// `script`, run by the system `sh`. macOS scans a new file the first
    /// time it is executed, which took up to 4 s with 40 starting at once,
    /// longer than the time-limit tests allow.
    fn install(&self, name: &str, capabilities: &[&str], script: &str) -> &Self {
        let capabilities: Vec<Value> = capabilities
            .iter()
            .map(|capability| {
                json!({
                    "name": capability,
                    "description": "",
                    "inputSchema": serde_json::from_str::<Value>(INPUT_SCHEMA).unwrap(),
                    "outputSchema": serde_json::from_str::<Value>(OUTPUT_SCHEMA).unwrap(),
                })
            })
            .collect();
        let manifest = json!({
            "$type": "board/plugin-manifest/v1",
            "name": name,
            "version": "0.1.0",
            "description": "",
            "run": ["sh", "./run"],
            "capabilities": capabilities,
        });
        self.data
            .write(
                &format!("plugins/{name}/plugin.json"),
                Item::File(&manifest.to_string()),
            )
            .write(&format!("plugins/{name}/run"), Item::File(script));
        self
    }

    /// Installs `tasks` offering `due`, answering with `script`.
    fn tasks(script: &str) -> Self {
        let fixture = Self::new(&["tasks"]);
        fixture.install("tasks", &["due"], script);
        fixture
    }

    fn plugin(&self, name: &str) -> PathBuf {
        self.data.workdir().join("plugins").join(name)
    }

    fn call_with(
        &self,
        target: &str,
        input: &str,
        environment: &Environment,
        limits: Limits,
    ) -> Result<CallReport, CallFailed> {
        let target = match target.strip_prefix("--capability ") {
            Some(capability) => Target::Capability(capability.parse().unwrap()),
            None => target.parse().unwrap(),
        };
        let directory = self.data.workdir().join("plugins");
        let plugins = Plugins {
            directory: Some(&directory),
            environment: environment.clone(),
            limits,
        };
        open(&self.board)
            .call(&target, input.as_bytes(), &plugins, Some(self.cache.path()))
            .map(|called| called.report)
    }

    fn call(&self, target: &str, input: &str) -> Result<CallReport, CallFailed> {
        self.call_with(target, input, &environment(), Limits::default())
    }

    /// The code of a refused call.
    fn refused(&self, target: &str, input: &str) -> Code {
        self.call(target, input).unwrap_err().code().unwrap()
    }
}

fn environment() -> Environment {
    Environment {
        path: Some(OsString::from("/usr/bin:/bin")),
        home: Some(OsString::from("/home/nobody")),
        ..Environment::default()
    }
}

/// A script that ignores its request and writes `stdout`, then exits
/// `exit`.
fn answering(stdout: &str, exit: u8) -> String {
    format!("#!/bin/sh\ncat > /dev/null\nprintf '%s' '{stdout}'\nexit {exit}\n")
}

const RESULT: &str = r#"{"$type": "board/call-result/v1", "output": {"count": 2}}"#;

#[test]
fn a_call_sends_one_request_and_returns_the_checked_output() {
    let fixture = Fixture::tasks(RECORDING);
    let report = fixture.call("tasks.due", r#"{"n": 1}"#).unwrap();
    assert_eq!(report.plugin.as_str(), "tasks");
    assert_eq!(report.capability.as_str(), "due");
    assert_eq!(report.output, json!({"count": 2}));

    let directory = fixture.plugin("tasks");
    let request: Value =
        serde_json::from_slice(&std::fs::read(directory.join("request.json")).unwrap()).unwrap();
    assert_eq!(request["$type"], "board/call/v1");
    assert_eq!(request["capability"], "due");
    assert_eq!(request["input"], json!({"n": 1}));
    // Board sends the canonical root; on macOS a temporary directory under
    // /var is reached through the symlink /private/var.
    let root = fixture.board.workdir().canonicalize().unwrap();
    assert_eq!(request["board"]["root"], root.to_str().unwrap());
    let database = PathBuf::from(request["board"]["database"].as_str().unwrap());
    assert!(database.starts_with(fixture.cache.path()), "{database:?}");
    assert!(
        database.ends_with("plugins/tasks@0.1.0.sqlite"),
        "{database:?}"
    );
    assert!(database.parent().unwrap().is_dir());

    let ran_in = std::fs::read_to_string(directory.join("directory.txt")).unwrap();
    assert_eq!(
        PathBuf::from(ran_in.trim()),
        directory.canonicalize().unwrap()
    );
}

#[test]
fn a_plugin_sees_only_the_variables_board_passes_and_one_more_depth() {
    let fixture = Fixture::tasks(RECORDING);
    let mut environment = environment();
    environment.cache = Some(OsString::from("/cache"));
    environment.depth = Some(OsString::from("3"));
    fixture
        .call_with("tasks.due", r#"{"n": 1}"#, &environment, Limits::default())
        .unwrap();
    let seen = std::fs::read_to_string(fixture.plugin("tasks").join("environment.txt")).unwrap();
    let mut keys: Vec<&str> = seen
        .lines()
        .filter_map(|line| line.split_once('=').map(|(key, _)| key))
        // The shell sets these itself.
        .filter(|key| !["PWD", "SHLVL", "_", "OLDPWD"].contains(key))
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["BOARD_CALL_DEPTH", "HOME", "PATH", "XDG_CACHE_HOME"]);
    assert!(seen.contains("BOARD_CALL_DEPTH=4\n"), "{seen}");
    assert!(seen.contains("XDG_CACHE_HOME=/cache\n"), "{seen}");
}

#[test]
fn a_bare_program_name_is_found_through_path() {
    let fixture = Fixture::tasks(RECORDING);
    assert!(
        fixture.call("tasks.due", r#"{"n": 1}"#).is_ok(),
        "sh is found"
    );
    let manifest = fixture.plugin("tasks").join("plugin.json");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let missing = text.replace(r#"["sh","./run"]"#, r#"["no-such-program-anywhere"]"#);
    assert_ne!(missing, text);
    fixture
        .data
        .write("plugins/tasks/plugin.json", Item::File(&missing));
    assert_eq!(
        fixture.refused("tasks.due", r#"{"n": 1}"#),
        Code::PluginFailed
    );
}

#[test]
fn a_call_by_capability_runs_the_one_enabled_plugin_that_offers_it() {
    let fixture = Fixture::new(&["tasks"]);
    fixture
        .install("tasks", &["due"], RECORDING)
        .install("notes", &["due"], RECORDING);
    let report = fixture.call("--capability due", r#"{"n": 1}"#).unwrap();
    assert_eq!(report.plugin.as_str(), "tasks");
}

#[test]
fn a_capability_two_enabled_plugins_offer_is_ambiguous() {
    let fixture = Fixture::new(&["tasks", "notes"]);
    fixture
        .install("tasks", &["due"], RECORDING)
        .install("notes", &["due"], RECORDING);
    let error = fixture.call("--capability due", r#"{"n": 1}"#).unwrap_err();
    assert_eq!(error.code(), Some(Code::PluginAmbiguous));
    assert_eq!(error.fields()["plugins"], json!(["notes", "tasks"]));
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn a_plugin_the_board_does_not_enable_is_not_called() {
    let fixture = Fixture::new(&[]);
    fixture.install("tasks", &["due"], RECORDING);
    assert_eq!(
        fixture.refused("tasks.due", r#"{"n": 1}"#),
        Code::PluginNotEnabled
    );
    assert_eq!(
        fixture.refused("--capability due", r#"{"n": 1}"#),
        Code::PluginNotEnabled
    );
    let fixture = Fixture::tasks(RECORDING);
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
    assert!(fixture.call("--capability due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn a_plugin_or_capability_that_is_not_installed_is_refused() {
    let fixture = Fixture::tasks(RECORDING);
    for target in ["notes.due", "tasks.late", "--capability late"] {
        assert_eq!(
            fixture.refused(target, r#"{"n": 1}"#),
            Code::PluginNotInstalled,
            "{target}"
        );
    }
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn a_refusal_names_the_missing_plugin_or_capability_and_what_is_offered() {
    let fixture = Fixture::tasks(RECORDING);
    fixture.install("notes", &["due", "list"], RECORDING);
    for (target, message) in [
        (
            "notes.due",
            "plugin notes is installed but this board does not enable it",
        ),
        ("other.due", "plugin other is not installed"),
        (
            "tasks.dew",
            "plugin tasks has no capability dew; it offers due",
        ),
        (
            "notes.late",
            "plugin notes has no capability late; it offers due, list",
        ),
        (
            "--capability dew",
            "no installed plugin offers capability dew; the nearest is due",
        ),
        (
            "--capability overdue",
            "no installed plugin offers capability overdue",
        ),
    ] {
        let error = fixture.call(target, r#"{"n": 1}"#).unwrap_err();
        assert_eq!(error.to_string(), message, "{target}");
    }
    let error = fixture.call("tasks.dew", r#"{"n": 1}"#).unwrap_err();
    assert_eq!(error.code(), Some(Code::PluginNotInstalled));
    assert_eq!(
        Value::Object(error.fields()),
        json!({"plugin": "tasks", "capability": "dew", "offered": ["due"]})
    );
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn input_the_schema_refuses_is_reported_at_its_pointer() {
    let fixture = Fixture::tasks(RECORDING);
    let error = fixture.call("tasks.due", r#"{"n": "one"}"#).unwrap_err();
    assert_eq!(error.code(), Some(Code::PluginBadInput));
    assert_eq!(
        error.to_string(),
        r#"the input fails the capability's input schema at "/n": "one" is not of type "integer""#
    );
    assert_eq!(error.fields()["pointer"], "/n");
    let error = fixture.call("tasks.due", "{}").unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"the input fails the capability's input schema at its root: "n" is a required property"#
    );
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn a_named_plugin_with_a_bad_manifest_is_refused_and_skipped_by_capability() {
    let fixture = Fixture::new(&["tasks", "broken"]);
    fixture.install("tasks", &["due"], RECORDING);
    fixture
        .data
        .write("plugins/broken/plugin.json", Item::File("{}"));
    assert_eq!(
        fixture.refused("broken.due", r#"{"n": 1}"#),
        Code::PluginBadManifest
    );
    assert_eq!(
        fixture
            .call("--capability due", r#"{"n": 1}"#)
            .unwrap()
            .plugin
            .as_str(),
        "tasks"
    );
}

#[test]
fn a_plugin_shipping_an_unusable_schema_is_a_bad_manifest_and_a_usable_one_is_called() {
    let fixture = Fixture::tasks(RECORDING);
    let manifest = fixture.plugin("tasks").join("plugin.json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    value["types"] = json!(["tasks/note/v1"]);
    fixture
        .data
        .write("plugins/tasks/plugin.json", Item::File(&value.to_string()))
        .write(
            "plugins/tasks/types/tasks/note/v1.schema.json",
            Item::File(r#"{"type": 3}"#),
        );
    assert_eq!(
        fixture.refused("tasks.due", r#"{"n": 1}"#),
        Code::PluginBadManifest
    );
    fixture.data.write(
        "plugins/tasks/types/tasks/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn input_that_is_not_strict_json_or_fails_the_schema_is_refused_before_the_plugin_runs() {
    let fixture = Fixture::tasks(RECORDING);
    for input in ["{", r#"{"n": 1, "n": 2}"#, r#"{"n": "one"}"#, "{}"] {
        assert_eq!(
            fixture.refused("tasks.due", input),
            Code::PluginBadInput,
            "{input}"
        );
    }
    assert!(!fixture.plugin("tasks").join("request.json").exists());
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn a_plugins_own_error_is_reported_with_its_code_and_fields() {
    let error = r#"{"$type": "board/error/v1", "code": "tasks::date::bad", "message": "the date 2026-13-01 is not a date", "fields": {"date": "2026-13-01"}}"#;
    let fixture = Fixture::tasks(&answering(error, 1));
    let refused = fixture.call("tasks.due", r#"{"n": 1}"#).unwrap_err();
    assert_eq!(refused.code(), None);
    assert_eq!(refused.code_text(), "tasks::date::bad");
    assert_eq!(refused.to_string(), "the date 2026-13-01 is not a date");
    assert_eq!(
        Value::Object(refused.fields()),
        json!({"date": "2026-13-01", "plugin": "tasks"})
    );
}

#[test]
fn an_answer_board_cannot_accept_is_bad_output() {
    let foreign = r#"{"$type": "board/error/v1", "code": "notes::date::bad", "message": "m"}"#;
    let own = r#"{"$type": "board/error/v1", "code": "tasks::date::bad", "message": "m"}"#;
    for (stdout, exit) in [
        (
            r#"{"$type": "board/call-result/v1", "output": {"count": "two"}}"#,
            0,
        ),
        (
            r#"{"$type": "board/call-result/v1", "output": {"count": 2}, "x": 1}"#,
            0,
        ),
        (
            r#"{"$type": "board/call-result/v2", "output": {"count": 2}}"#,
            0,
        ),
        ("not json", 0),
        ("", 0),
        (own, 0),
        (RESULT, 1),
        (foreign, 1),
        ("not json", 1),
    ] {
        let fixture = Fixture::tasks(&answering(stdout, exit));
        assert_eq!(
            fixture.refused("tasks.due", r#"{"n": 1}"#),
            Code::PluginBadOutput,
            "{stdout} exit {exit}"
        );
    }
    let fixture = Fixture::tasks(&answering(RESULT, 0));
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn a_plugin_that_exits_non_zero_without_an_error_document_failed() {
    for (stdout, exit) in [("", 1), (RESULT, 2), ("", 3)] {
        let fixture = Fixture::tasks(&answering(stdout, exit));
        assert_eq!(
            fixture.refused("tasks.due", r#"{"n": 1}"#),
            Code::PluginFailed,
            "{stdout} exit {exit}"
        );
    }
    let killed = "#!/bin/sh\nkill -9 $$\n";
    assert_eq!(
        Fixture::tasks(killed).refused("tasks.due", r#"{"n": 1}"#),
        Code::PluginFailed
    );
}

#[test]
fn board_keeps_the_end_of_a_failed_plugins_stderr() {
    let script = "#!/bin/sh\ncat > /dev/null\nprintf 'early lines\\nthe last line' >&2\nexit 1\n";
    let fixture = Fixture::tasks(script);
    let limits = Limits {
        stderr: 13,
        ..Limits::default()
    };
    let error = fixture
        .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
        .unwrap_err();
    assert_eq!(error.stderr(), Some(&b"the last line"[..]));
    assert_eq!(error.fields()["stderr"], "the last line");
}

#[test]
fn board_keeps_the_last_64_kib_of_stderr_by_default() {
    let limit = Limits::default().stderr;
    assert_eq!(limit, 64 * 1024);
    // An `x`, then `a`s: one byte over the limit loses the `x`, one under keeps it.
    for total in [limit + 1, limit - 1] {
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\nprintf x >&2\nhead -c {} /dev/zero | tr '\\0' a >&2\nexit 1\n",
            total - 1
        );
        let written: Vec<u8> = std::iter::once(b'x')
            .chain(std::iter::repeat_n(b'a', total - 1))
            .collect();
        let error = Fixture::tasks(&script)
            .call("tasks.due", r#"{"n": 1}"#)
            .unwrap_err();
        assert_eq!(
            error.stderr(),
            Some(&written[total.saturating_sub(limit)..]),
            "{total} bytes written"
        );
    }
}

#[test]
fn a_plugin_past_its_time_limit_is_stopped() {
    let limits = Limits {
        time: Duration::from_millis(500),
        ..Limits::default()
    };
    let slow = "#!/bin/sh\ncat > /dev/null\nsleep 10\n";
    let started = Instant::now();
    let error = Fixture::tasks(slow)
        .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
        .unwrap_err();
    assert_eq!(error.code(), Some(Code::PluginTimeout));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(
        Fixture::tasks(&answering(RESULT, 0))
            .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
            .is_ok()
    );
}

#[test]
fn a_grandchild_holding_the_pipes_cannot_keep_board_waiting_past_the_limit() {
    let limits = Limits {
        time: Duration::from_millis(500),
        ..Limits::default()
    };
    let script = format!("#!/bin/sh\ncat > /dev/null\nsleep 30 &\nprintf '%s' '{RESULT}'\n");
    let started = Instant::now();
    let error = Fixture::tasks(&script)
        .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
        .unwrap_err();
    assert_eq!(error.code(), Some(Code::PluginTimeout));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_plugin_past_its_time_limit_is_asked_to_stop_what_it_started() {
    let limits = Limits {
        time: Duration::from_millis(500),
        ..Limits::default()
    };
    let script = "#!/bin/sh\ncat > /dev/null\nsh -c 'echo $$ > grandchild.pid; exec sleep 30' &\ntrap 'kill $!; exit 1' TERM\nwait\n";
    let fixture = Fixture::tasks(script);
    let error = fixture
        .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
        .unwrap_err();
    assert_eq!(error.code(), Some(Code::PluginTimeout));
    let pid = std::fs::read_to_string(fixture.plugin("tasks").join("grandchild.pid")).unwrap();
    assert!(ends(pid.trim()));
}

#[test]
fn a_plugin_that_ignores_sigterm_is_killed_after_the_grace_period() {
    let limits = Limits {
        time: Duration::from_millis(500),
        ..Limits::default()
    };
    let script = "#!/bin/sh\ncat > /dev/null\ntrap '' TERM\nexec sleep 30\n";
    let started = Instant::now();
    let error = Fixture::tasks(script)
        .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
        .unwrap_err();
    assert_eq!(error.code(), Some(Code::PluginTimeout));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_wrapper_that_execs_its_program_is_stopped_and_one_that_does_not_leaves_it() {
    let limits = Limits {
        time: Duration::from_millis(500),
        ..Limits::default()
    };
    for (exec, stopped) in [("exec ", true), ("", false)] {
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\n{exec}sh -c 'echo $$ > child.pid; exec sleep 30'\n"
        );
        let fixture = Fixture::tasks(&script);
        let error = fixture
            .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
            .unwrap_err();
        assert_eq!(error.code(), Some(Code::PluginTimeout));
        let pid = std::fs::read_to_string(fixture.plugin("tasks").join("child.pid")).unwrap();
        if stopped {
            assert!(ends(pid.trim()), "the child of a wrapper that execs it");
        } else {
            assert!(running(pid.trim()), "the child of a wrapper without exec");
            tool(&["kill", "-KILL", pid.trim()]);
        }
    }
}

/// Whether the process `pid` has ended within a bounded number of checks.
fn ends(pid: &str) -> bool {
    (0..500).any(|_| {
        let ended = !running(pid);
        if !ended {
            std::thread::sleep(Duration::from_millis(10));
        }
        ended
    })
}

/// Whether the process `pid` is running. A zombie is not: it holds nothing,
/// and only waits for init to reap it.
fn running(pid: &str) -> bool {
    let state = tool(&["ps", "-o", "stat=", "-p", pid]);
    !(state.trim().is_empty() || state.trim_start().starts_with('Z'))
}

/// The stdout of `command`, which asks about or stops a process a plugin
/// started.
#[expect(
    clippy::disallowed_methods,
    reason = "the test inspects processes a plugin started"
)]
fn tool(command: &[&str]) -> String {
    let output = std::process::Command::new(command[0])
        .args(&command[1..])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_plugin_writing_more_than_the_stdout_limit_is_stopped() {
    let limits = Limits {
        stdout: 1024,
        ..Limits::default()
    };
    let large = "#!/bin/sh\ncat > /dev/null\nhead -c 4096 /dev/zero\n";
    let error = Fixture::tasks(large)
        .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
        .unwrap_err();
    assert_eq!(error.code(), Some(Code::PluginTooLarge));
    assert!(
        Fixture::tasks(&answering(RESULT, 0))
            .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
            .is_ok()
    );
}

#[test]
fn a_call_nested_eight_deep_is_refused() {
    let fixture = Fixture::tasks(RECORDING);
    let at = |depth: &str| {
        let mut environment = environment();
        environment.depth = Some(OsString::from(depth));
        fixture.call_with("tasks.due", r#"{"n": 1}"#, &environment, Limits::default())
    };
    for refused in ["8", "9", "x", "-1", ""] {
        assert_eq!(
            at(refused).unwrap_err().code(),
            Some(Code::PluginTooDeep),
            "{refused:?}"
        );
    }
    assert!(at("7").is_ok());
    assert!(at("0").is_ok());
}

#[test]
fn without_a_cache_the_database_is_a_temporary_file_for_the_run() {
    let fixture = Fixture::tasks(RECORDING);
    let target: Target = "tasks.due".parse().unwrap();
    let directory = fixture.data.workdir().join("plugins");
    let plugins = Plugins {
        directory: Some(&directory),
        environment: environment(),
        limits: Limits::default(),
    };
    open(&fixture.board)
        .call(&target, br#"{"n": 1}"#, &plugins, None)
        .unwrap();
    let request: Value = serde_json::from_slice(
        &std::fs::read(fixture.plugin("tasks").join("request.json")).unwrap(),
    )
    .unwrap();
    let database = PathBuf::from(request["board"]["database"].as_str().unwrap());
    assert!(database.ends_with("tasks@0.1.0.sqlite"));
    assert!(!database.parent().unwrap().exists(), "{database:?}");
}
