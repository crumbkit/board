use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use board_model::RefusalCode::Board;
use board_model::{Code, RefusalCode};
use board_plugin::{CallReport, Environment, Limits, Target};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::tests::open;
use crate::{CallFailed, Phase, Plugins, Trace};

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
        self.traced(target, input, environment, limits).0
    }

    /// A call like `call_with`, and what the board traced while making it.
    fn traced(
        &self,
        target: &str,
        input: &str,
        environment: &Environment,
        limits: Limits,
    ) -> (Result<CallReport, CallFailed>, Trace) {
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
        let board = open(&self.board);
        let called = board
            .call(&target, input.as_bytes(), &plugins, Some(self.cache.path()))
            .map(|called| called.report);
        (called, board.take_trace())
    }

    fn call(&self, target: &str, input: &str) -> Result<CallReport, CallFailed> {
        self.call_with(target, input, &environment(), Limits::default())
    }

    /// The code of a refused call.
    fn refused(&self, target: &str, input: &str) -> RefusalCode {
        self.call(target, input).unwrap_err().refusal().code
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
        Board(Code::PluginNotStarted)
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
    assert_eq!(error.refusal().code, Board(Code::PluginAmbiguous));
    assert_eq!(error.refusal().fields["plugins"], json!(["notes", "tasks"]));
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn a_plugin_the_board_does_not_enable_is_not_called() {
    let fixture = Fixture::new(&[]);
    fixture.install("tasks", &["due"], RECORDING);
    assert_eq!(
        fixture.refused("tasks.due", r#"{"n": 1}"#),
        Board(Code::PluginNotEnabled)
    );
    assert_eq!(
        fixture.refused("--capability due", r#"{"n": 1}"#),
        Board(Code::PluginNotEnabled)
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
            Board(Code::PluginNotInstalled),
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
    assert_eq!(error.refusal().code, Board(Code::PluginNotInstalled));
    assert_eq!(
        Value::Object(error.refusal().fields),
        json!({"plugin": "tasks", "capability": "dew", "offered": ["due"]})
    );
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn input_the_schema_refuses_is_reported_at_its_pointer() {
    let fixture = Fixture::tasks(RECORDING);
    let error = fixture.call("tasks.due", r#"{"n": "one"}"#).unwrap_err();
    assert_eq!(error.refusal().code, Board(Code::PluginBadInput));
    assert_eq!(
        error.to_string(),
        r#"the input to capability due fails its schema at "/n": "one" is not of type "integer""#
    );
    assert_eq!(error.refusal().fields["pointer"], "/n");
    let error = fixture.call("tasks.due", "{}").unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"the input to capability due fails its schema at its root: "n" is a required property"#
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
        Board(Code::PluginBadManifest)
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
        Board(Code::PluginBadManifest)
    );
    fixture.data.write(
        "plugins/tasks/types/tasks/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    fixture.board.write(
        ".board/types/tasks/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

/// `tasks`, shipping `tasks/note/v1` as `{"type": "object"}`.
fn shipping_a_schema() -> Fixture {
    let fixture = Fixture::tasks(RECORDING);
    let manifest = fixture.plugin("tasks").join("plugin.json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    value["types"] = json!(["tasks/note/v1"]);
    fixture
        .data
        .write("plugins/tasks/plugin.json", Item::File(&value.to_string()))
        .write(
            "plugins/tasks/types/tasks/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        );
    fixture
}

#[test]
fn a_plugin_whose_schema_the_board_lacks_is_not_started_and_one_the_board_holds_is_called() {
    let fixture = shipping_a_schema();
    let refused = fixture
        .call("tasks.due", r#"{"n": 1}"#)
        .unwrap_err()
        .refusal();
    assert_eq!(refused.code, Board(Code::PluginSchemaMissing));
    assert_eq!(refused.fields["plugin"], "tasks");
    assert_eq!(refused.fields["type"], "tasks/note/v1");
    assert_eq!(
        refused.fields["schema"],
        ".board/types/tasks/note/v1.schema.json"
    );
    assert!(!fixture.plugin("tasks").join("request.json").exists());
    fixture.board.write(
        ".board/types/tasks/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    assert!(fixture.call("tasks.due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn a_plugin_whose_schema_the_board_holds_differently_is_not_started_and_one_holding_equal_json_is_called()
 {
    let fixture = shipping_a_schema();
    fixture.board.write(
        ".board/types/tasks/note/v1.schema.json",
        Item::File(r#"{"type": "object", "additionalProperties": true}"#),
    );
    assert_eq!(
        fixture.refused("--capability due", r#"{"n": 1}"#),
        Board(Code::PluginSchemaDiffers)
    );
    assert!(!fixture.plugin("tasks").join("request.json").exists());
    fixture.board.write(
        ".board/types/tasks/note/v1.schema.json",
        Item::File("{ \"type\" : \"object\" }\n"),
    );
    assert!(fixture.call("--capability due", r#"{"n": 1}"#).is_ok());
}

#[test]
fn input_that_is_not_strict_json_or_fails_the_schema_is_refused_before_the_plugin_runs() {
    let fixture = Fixture::tasks(RECORDING);
    for input in ["{", r#"{"n": 1, "n": 2}"#, r#"{"n": "one"}"#, "{}"] {
        assert_eq!(
            fixture.refused("tasks.due", input),
            Board(Code::PluginBadInput),
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
    assert_eq!(
        refused.refusal().code,
        RefusalCode::Plugin("tasks::date::bad".to_owned())
    );
    assert_eq!(refused.to_string(), "the date 2026-13-01 is not a date");
    assert_eq!(
        Value::Object(refused.refusal().fields),
        json!({"date": "2026-13-01", "plugin": "tasks"})
    );
}

#[test]
fn an_answer_board_cannot_accept_is_bad_output() {
    let foreign = r#"{"$type": "board/error/v1", "code": "notes::date::bad", "message": "m"}"#;
    let own = r#"{"$type": "board/error/v1", "code": "tasks::date::bad", "message": "m"}"#;
    let schema_failure = r#"plugin tasks gave a bad answer: its output fails its schema at "/count": "two" is not of type "integer""#;
    for (stdout, exit, message) in [
        (
            r#"{"$type": "board/call-result/v1", "output": {"count": "two"}}"#,
            0,
            Some(schema_failure),
        ),
        (
            r#"{"$type": "board/call-result/v1", "output": {"count": 2}, "x": 1}"#,
            0,
            None,
        ),
        (
            r#"{"$type": "board/call-result/v2", "output": {"count": 2}}"#,
            0,
            None,
        ),
        ("not json", 0, None),
        ("", 0, None),
        (own, 0, None),
        (RESULT, 1, None),
        (foreign, 1, None),
        ("not json", 1, None),
    ] {
        let fixture = Fixture::tasks(&answering(stdout, exit));
        let error = fixture.call("tasks.due", r#"{"n": 1}"#).unwrap_err();
        assert_eq!(
            error.refusal().code,
            Board(Code::PluginBadOutput),
            "{stdout} exit {exit}"
        );
        if let Some(message) = message {
            assert_eq!(error.to_string(), message);
        }
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
            Board(Code::PluginFailed),
            "{stdout} exit {exit}"
        );
    }
    let killed = "#!/bin/sh\nkill -9 $$\n";
    assert_eq!(
        Fixture::tasks(killed).refused("tasks.due", r#"{"n": 1}"#),
        Board(Code::PluginFailed)
    );
}

fn phase<'a>(trace: &'a Trace, name: &str) -> Option<&'a Phase> {
    trace.phases.iter().find(|phase| phase.name == name)
}

/// The work count `started` of `trace` and how often it entered the phase
/// `plugin.reply`.
fn starts(trace: &Trace) -> (Option<usize>, Option<usize>) {
    (
        trace.work.get("started").copied(),
        phase(trace, "plugin.reply").map(|phase| phase.entered),
    )
}

#[test]
fn a_call_counts_the_plugin_it_started_and_times_its_reply_only_when_one_came() {
    let call = |fixture: Fixture| {
        let (called, trace) = fixture.traced(
            "tasks.due",
            r#"{"n": 1}"#,
            &environment(),
            Limits::default(),
        );
        let code = called.err().map(|error| error.refusal().code);
        (code, starts(&trace), trace)
    };
    let (code, counted, trace) = call(Fixture::tasks(&answering(RESULT, 0)));
    assert_eq!((code, counted), (None, (Some(1), Some(1))));
    let (reply, run) = (
        phase(&trace, "plugin.reply").unwrap(),
        phase(&trace, "plugin.call").unwrap(),
    );
    assert!(run.started <= reply.started, "{trace:?}");
    assert!(
        reply.started + reply.took <= run.started + run.took,
        "the reply came within the call: {trace:?}"
    );
    let (code, counted, _) = call(Fixture::tasks(&answering("", 1)));
    assert_eq!(
        (code, counted),
        (Some(Board(Code::PluginFailed)), (Some(1), None))
    );
    // A program that cannot be spawned is not counted.
    let fixture = Fixture::tasks(&answering(RESULT, 0));
    let manifest = fixture.plugin("tasks").join("plugin.json");
    let missing = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace(r#"["sh","./run"]"#, r#"["no-such-program-anywhere"]"#);
    fixture
        .data
        .write("plugins/tasks/plugin.json", Item::File(&missing));
    let (code, counted, _) = call(fixture);
    assert_eq!(
        (code, counted),
        (Some(Board(Code::PluginNotStarted)), (Some(0), None))
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
    assert_eq!(error.refusal().stderr, b"the last line");
    assert_eq!(error.refusal().fields["stderr"], "the last line");
}

#[test]
fn board_keeps_the_stderr_of_a_plugin_whose_output_fails_the_output_schema() {
    let logging = |output: &str| {
        format!(
            "#!/bin/sh\ncat > /dev/null\nprintf 'a log line' >&2\nprintf '%s' '{{\"$type\": \"board/call-result/v1\", \"output\": {output}}}'\n"
        )
    };
    let refused = Fixture::tasks(&logging(r#"{"count": "two"}"#))
        .call("tasks.due", r#"{"n": 1}"#)
        .unwrap_err()
        .refusal();
    assert_eq!(refused.code, Board(Code::PluginBadOutput));
    assert_eq!(refused.stderr, b"a log line");
    assert_eq!(refused.fields["stderr"], "a log line");
    let admitted = Fixture::tasks(&logging(r#"{"count": 2}"#)).call("tasks.due", r#"{"n": 1}"#);
    assert_eq!(admitted.unwrap().output, json!({"count": 2}));
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
            error.refusal().stderr,
            written[total.saturating_sub(limit)..],
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
    // It writes finished.txt only if it is left to run to its end.
    let slow = "#!/bin/sh\ncat > /dev/null\necho $$ > plugin.pid\nsleep 10\ntouch finished.txt\n";
    let fixture = Fixture::tasks(slow);
    let error = fixture
        .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
        .unwrap_err();
    assert_eq!(error.refusal().code, Board(Code::PluginTimeout));
    assert!(ends(&written_pid(&fixture, "plugin.pid")));
    assert!(!fixture.plugin("tasks").join("finished.txt").exists());
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
    // The grandchild holds the pipes, and writes finished.txt only if it is
    // left to run to its end.
    let script = format!(
        "#!/bin/sh\ncat > /dev/null\nsh -c 'echo $$ > grandchild.pid; sleep 30; touch finished.txt' &\nprintf '%s' '{RESULT}'\n"
    );
    let fixture = Fixture::tasks(&script);
    let error = fixture
        .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
        .unwrap_err();
    assert_eq!(error.refusal().code, Board(Code::PluginTimeout));
    assert!(ends(&written_pid(&fixture, "grandchild.pid")));
    assert!(!fixture.plugin("tasks").join("finished.txt").exists());
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
    assert_eq!(error.refusal().code, Board(Code::PluginTimeout));
    assert!(ends(&written_pid(&fixture, "grandchild.pid")));
}

#[test]
fn a_plugin_that_traps_sigterm_has_the_grace_period_to_finish_and_one_that_ignores_it_is_killed() {
    let limits = Limits {
        time: Duration::from_secs(2),
        ..Limits::default()
    };
    let finish = "echo stopped > stopped.txt; exit 1";
    for (action, finished) in [(finish, true), ("", false)] {
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\necho $$ > plugin.pid\ntrap '{action}' TERM\nsleep 30 &\nwait\n"
        );
        let fixture = Fixture::tasks(&script);
        let error = fixture
            .call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits)
            .unwrap_err();
        assert_eq!(error.refusal().code, Board(Code::PluginTimeout));
        assert!(ends(&written_pid(&fixture, "plugin.pid")));
        let stopped = fixture.plugin("tasks").join("stopped.txt");
        assert_eq!(stopped.exists(), finished, "with the trap {action:?}");
    }
}

#[test]
fn a_wrapper_has_its_program_stopped_whether_or_not_it_execs_it() {
    let limits = Limits {
        time: Duration::from_secs(2),
        ..Limits::default()
    };
    for exec in ["exec ", ""] {
        // A program that outlives the limit, then one that answers within it.
        for (seconds, timeout) in [(60, true), (0, false)] {
            let script = format!(
                "#!/bin/sh\ncat > /dev/null\n{exec}sh -c 'echo $$ > child.pid; sleep {seconds}; printf %s \"$0\"' '{RESULT}'\n"
            );
            let fixture = Fixture::tasks(&script);
            let called = fixture.call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits);
            let wrapper = format!("a wrapper with {exec:?} of a program sleeping {seconds} s");
            if timeout {
                assert_eq!(
                    called.unwrap_err().refusal().code,
                    Board(Code::PluginTimeout),
                    "{wrapper}"
                );
                assert!(ends(&written_pid(&fixture, "child.pid")), "{wrapper}");
            } else {
                assert_eq!(called.unwrap().output, json!({"count": 2}), "{wrapper}");
            }
        }
    }
}

#[test]
fn a_plugin_past_its_time_limit_is_stopped_with_the_grandchild_it_started() {
    let limits = Limits {
        time: Duration::from_secs(2),
        ..Limits::default()
    };
    // A plugin that waits for its grandchild past the limit, then one that
    // answers without waiting. Board signals the group only past a limit, so
    // the second grandchild is left running.
    for (wait, timeout) in [("wait\n", true), ("", false)] {
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\nsh -c 'echo $$ > grandchild.pid; exec sleep 60' > /dev/null 2>&1 &\n{wait}printf '%s' '{RESULT}'\n"
        );
        let fixture = Fixture::tasks(&script);
        let called = fixture.call_with("tasks.due", r#"{"n": 1}"#, &environment(), limits);
        let pid = written_pid(&fixture, "grandchild.pid");
        if timeout {
            assert_eq!(
                called.unwrap_err().refusal().code,
                Board(Code::PluginTimeout)
            );
            assert!(
                ends(&pid),
                "the grandchild of a plugin stopped past its limit"
            );
        } else {
            assert_eq!(called.unwrap().output, json!({"count": 2}));
            assert!(
                running(&pid),
                "the grandchild of a plugin that answered in time"
            );
            tool(&["kill", "-KILL", &pid]);
        }
    }
}

#[test]
fn a_plugin_leads_its_own_process_group_unless_the_call_is_nested() {
    let script = format!(
        "#!/bin/sh\ncat > /dev/null\necho $$ > plugin.pid\nps -o pgid= -p $$ > group.txt\nprintf '%s' '{RESULT}'\n"
    );
    let ours = tool(&["ps", "-o", "pgid=", "-p", &std::process::id().to_string()]);
    for (depth, own) in [(None, true), (Some("1"), false)] {
        let fixture = Fixture::tasks(&script);
        let environment = Environment {
            depth: depth.map(OsString::from),
            ..environment()
        };
        fixture
            .call_with("tasks.due", r#"{"n": 1}"#, &environment, Limits::default())
            .unwrap();
        let read = |name| std::fs::read_to_string(fixture.plugin("tasks").join(name)).unwrap();
        let group = read("group.txt");
        let expected = if own {
            read("plugin.pid")
        } else {
            ours.clone()
        };
        assert_eq!(group.trim(), expected.trim(), "at depth {depth:?}");
    }
}

/// The pid a fixture script wrote to `name` in the plugin's directory, once
/// the whole line is there.
fn written_pid(fixture: &Fixture, name: &str) -> String {
    let path = fixture.plugin("tasks").join(name);
    for _ in 0..500 {
        if let Ok(text) = std::fs::read_to_string(&path)
            && text.ends_with('\n')
        {
            return text.trim().to_owned();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the plugin never wrote a pid to {}", path.display());
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
    assert_eq!(error.refusal().code, Board(Code::PluginTooLarge));
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
            at(refused).unwrap_err().refusal().code,
            Board(Code::PluginTooDeep),
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
