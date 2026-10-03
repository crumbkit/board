//! Plugins calling plugins through the real `board` binary
//! (docs/plugins.md, Calling a capability).
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "a nested call needs the real board on the plugin's PATH"
)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

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
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"}
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

/// Runs the real `board` in `board` with plugins from `data`.
fn board_call(board: &Scenario, data: &Scenario, arguments: &[&str], input: &str) -> Output {
    let binary = Path::new(env!("CARGO_BIN_EXE_board"));
    let path = format!("{}:/usr/bin:/bin", binary.parent().unwrap().display());
    let mut child = Command::new(binary)
        .args(arguments)
        .current_dir(board.workdir())
        .env_clear()
        .env("PATH", path)
        .env("XDG_DATA_HOME", data.workdir())
        .env("XDG_CACHE_HOME", data.workdir().join("cache"))
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
    child.wait_with_output().unwrap()
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
    assert_eq!(report["output"], json!({"due": {"count": 2}}));
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
