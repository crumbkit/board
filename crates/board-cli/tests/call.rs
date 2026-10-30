//! Plugins calling plugins through the real `board` binary
//! (docs/plugins.md, Calling a capability).
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "a nested call needs the real board on the plugin's PATH"
)]

use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

/// Reads the request, keeps it, and leaves the board's root in `$root`.
const PRELUDE: &str = r#"#!/bin/sh
cat > request.json
root=$(sed -n 's/^    "root": "\(.*\)",\{0,1\}$/\1/p' request.json)
"#;

fn manifest(name: &str, capability: &str) -> String {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": name,
        "version": "1.0.0",
        "description": "",
        "run": ["./run"],
        "capabilities": [{
            "name": capability,
            "description": "",
            "inputSchema": {"type": "object"},
            "outputSchema": {"type": "object"}
        }]
    })
    .to_string()
}

/// A board enabling `enabled`, and a data directory to install plugins in.
fn fixture(enabled: &[&str]) -> (Scenario, Scenario) {
    let board = Scenario::new();
    board.write(
        ".board/board.json",
        Item::File(&json!({"$type": "board/config/v1", "plugins": enabled}).to_string()),
    );
    (board, Scenario::new())
}

fn install(data: &Scenario, name: &str, capability: &str, script: &str) {
    data.write(
        &format!("board/plugins/{name}/plugin.json"),
        Item::File(&manifest(name, capability)),
    )
    .write(
        &format!("board/plugins/{name}/run"),
        Item::Executable(script),
    );
}

/// Installs `name` like `install`, but runs its script through the system
/// `sh`. macOS scans a new executable the first time it runs, which can
/// outlast a test that waits on the plugin.
fn install_for_sh(data: &Scenario, name: &str, capability: &str, script: &str) {
    let mut manifest: Value = serde_json::from_str(&manifest(name, capability)).unwrap();
    manifest["run"] = json!(["sh", "./run"]);
    data.write(
        &format!("board/plugins/{name}/plugin.json"),
        Item::File(&manifest.to_string()),
    )
    .write(&format!("board/plugins/{name}/run"), Item::File(script));
}

/// Runs the real `board` in `board` with plugins from `data`.
fn board_call(board: &Scenario, data: &Scenario, arguments: &[&str], input: &str) -> Output {
    start(board, data, arguments, input)
        .wait_with_output()
        .unwrap()
}

/// Starts the real `board` in `board` with plugins from `data`, and writes
/// `input` to its stdin. Board leads a new process group, as it does when a
/// shell starts it as a job, so Ctrl-C can be sent to that group.
fn start(board: &Scenario, data: &Scenario, arguments: &[&str], input: &str) -> Child {
    let binary = Path::new(env!("CARGO_BIN_EXE_board"));
    let path = format!("{}:/usr/bin:/bin", binary.parent().unwrap().display());
    let mut child = Command::new(binary)
        .process_group(0)
        .args(arguments)
        .current_dir(board.workdir())
        .env_clear()
        .env("PATH", path)
        .env("XDG_DATA_HOME", data.workdir())
        .env("XDG_CACHE_HOME", data.workdir().join("cache"))
        .env("XDG_CONFIG_HOME", data.workdir().join("config"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child
}

#[test]
fn ctrl_c_on_a_call_stops_every_process_in_the_plugins_group() {
    // A program the plugin waits for, interrupted; then one that ends at
    // once, left alone. Ctrl-C sends SIGINT to the terminal's foreground
    // group, which is Board's group here.
    for (seconds, interrupted) in [(60, true), (0, false)] {
        let (board, data) = fixture(&["sleeper"]);
        install_for_sh(
            &data,
            "sleeper",
            "go",
            &format!(
                "#!/bin/sh\ncat > /dev/null\nsh -c 'echo $$ > program.pid; exec sleep {seconds}'\nprintf '%s' '{{\"$type\": \"board/call-result/v1\", \"output\": {{}}}}'\n"
            ),
        );
        let mut child = start(&board, &data, &["call", "sleeper.go"], "{}");
        let pid = written_pid(
            &data.workdir().join("board/plugins/sleeper/program.pid"),
            &mut child,
        );
        if interrupted {
            tool(&["kill", "-s", "INT", "--", &format!("-{}", child.id())]);
        }
        let output = child.wait_with_output().unwrap();
        if interrupted {
            assert_eq!(output.status.signal(), Some(2), "board stops on SIGINT");
            assert!(ends(&pid), "the program the plugin was waiting for");
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(printed, json!({}));
        }
    }
}

/// The pid a fixture script wrote to `path`, once the whole line is there.
/// It waits as long as `board` runs, because a loaded machine can take many
/// seconds to start the first `board` of a test run, and fails at once if
/// `board` exits first.
fn written_pid(path: &Path, board: &mut Child) -> String {
    loop {
        // Board's exit is checked first: once it has exited, the plugin has
        // written all it ever will.
        let exited = board.try_wait().unwrap();
        if let Ok(text) = std::fs::read_to_string(path)
            && text.ends_with('\n')
        {
            return text.trim().to_owned();
        }
        if let Some(status) = exited {
            let mut stderr = String::new();
            board
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .unwrap();
            panic!("board exited with {status} before the plugin wrote a pid: {stderr}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether the process `pid` has ended within a bounded number of checks.
fn ends(pid: &str) -> bool {
    (0..500).any(|_| {
        let state = tool(&["ps", "-o", "stat=", "-p", pid]);
        // A zombie has ended; it only waits for init to reap it.
        let ended = state.trim().is_empty() || state.trim_start().starts_with('Z');
        if !ended {
            std::thread::sleep(Duration::from_millis(10));
        }
        ended
    })
}

/// The stdout of `command`, which asks about or signals a process.
fn tool(command: &[&str]) -> String {
    let output = Command::new(command[0])
        .args(&command[1..])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_plugin_calls_another_by_capability_through_board_call() {
    let (board, data) = fixture(&["tasks", "planner"]);
    install(
        &data,
        "tasks",
        "due",
        &format!(
            "{PRELUDE}printf '%s' '{{\"$type\": \"board/call-result/v1\", \"output\": {{\"count\": 2}}}}'\n"
        ),
    );
    install(
        &data,
        "planner",
        "plan",
        &format!(
            r#"{PRELUDE}due=$(cd "$root" && echo '{{}}' | board call --capability due) || exit 2
printf '{{"$type": "board/call-result/v1", "output": {{"due": %s}}}}' "$due"
"#
        ),
    );
    let output = board_call(&board, &data, &["call", "planner.plan", "--json"], "{}");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["data"]["output"], json!({"due": {"count": 2}}));
}

#[test]
fn a_plugin_calling_itself_is_refused_once_calls_nest_eight_deep() {
    let (board, data) = fixture(&["loop"]);
    install(
        &data,
        "loop",
        "go",
        &format!(
            r#"{PRELUDE}echo "$BOARD_CALL_DEPTH" >> depths.txt
(cd "$root" && echo '{{}}' | board call loop.go --json) > "nested-$BOARD_CALL_DEPTH.json"
exit 3
"#
        ),
    );
    let output = board_call(&board, &data, &["call", "loop.go", "--json"], "{}");
    assert_eq!(output.status.code(), Some(1));
    let plugin = data.workdir().join("board/plugins/loop");
    let depths = std::fs::read_to_string(plugin.join("depths.txt")).unwrap();
    assert_eq!(depths, "1\n2\n3\n4\n5\n6\n7\n8\n");
    let deepest: Value =
        serde_json::from_slice(&std::fs::read(plugin.join("nested-8.json")).unwrap()).unwrap();
    assert_eq!(deepest["code"], "board::plugin::too_deep");
    let shallower: Value =
        serde_json::from_slice(&std::fs::read(plugin.join("nested-7.json")).unwrap()).unwrap();
    assert_eq!(shallower["code"], "board::plugin::failed");
}

/// Installs `tasks` offering `due`, whose input has the properties
/// `properties` and no others. It keeps the request Board sent it in
/// `request.json`.
fn install_due(data: &Scenario, properties: &Value) {
    let mut manifest: Value = serde_json::from_str(&manifest("tasks", "due")).unwrap();
    manifest["capabilities"][0]["inputSchema"] =
        json!({"type": "object", "properties": properties, "additionalProperties": false});
    manifest["run"] = json!(["sh", "./run"]);
    let script = format!(
        "{PRELUDE}printf '%s' '{{\"$type\": \"board/call-result/v1\", \"output\": {{}}}}'\n"
    );
    data.write(
        "board/plugins/tasks/plugin.json",
        Item::File(&manifest.to_string()),
    )
    .write("board/plugins/tasks/run", Item::File(&script));
}

/// The input Board sent `tasks`.
fn sent(data: &Scenario) -> Value {
    let request = std::fs::read(data.workdir().join("board/plugins/tasks/request.json")).unwrap();
    serde_json::from_slice::<Value>(&request).unwrap()["input"].clone()
}

/// What `child` printed, once it exits within `limit`. It is killed, and
/// the test fails, when it does not.
fn exited_within(mut child: Child, limit: Duration) -> Output {
    for _ in 0..limit.as_millis() / 10 {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    panic!("board did not answer within {limit:?}");
}

#[test]
fn a_capability_whose_inputs_are_all_flags_answers_with_stdin_left_open() {
    let (board, data) = fixture(&["tasks"]);
    install_due(&data, &json!({"limit": {"type": "integer"}}));
    let binary = Path::new(env!("CARGO_BIN_EXE_board"));
    let mut child = Command::new(binary)
        .args(["call", "tasks.due"])
        .current_dir(board.workdir())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_DATA_HOME", data.workdir())
        .env("XDG_CACHE_HOME", data.workdir().join("cache"))
        .env("XDG_CONFIG_HOME", data.workdir().join("config"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A pipe nobody writes to or closes, as an agent's shell can leave stdin.
    let open = child.stdin.take();
    let output = exited_within(child, Duration::from_secs(10));
    drop(open);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(sent(&data), json!({}));
}

#[test]
fn a_capability_with_an_input_that_cannot_be_a_flag_reads_stdin() {
    let (board, data) = fixture(&["tasks"]);
    let properties = json!({"limit": {"type": "integer"}, "tags": {"type": "array"}});
    install_due(&data, &properties);
    let given = board_call(&board, &data, &["call", "tasks.due"], r#"{"tags": ["a"]}"#);
    assert!(
        given.status.success(),
        "{}",
        String::from_utf8_lossy(&given.stderr)
    );
    assert_eq!(sent(&data), json!({"tags": ["a"]}));
    let torn = board_call(&board, &data, &["--json", "call", "tasks.due"], "{");
    let refused: Value = serde_json::from_slice(&torn.stdout).unwrap();
    assert_eq!(refused["code"], "board::plugin::bad_input");
}
