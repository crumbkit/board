//! How a refusal is printed, from a hand-built value.

use board_model::{Code, Fields, Refusal};
use serde_json::{Map, json};

use crate::render::{self, Format};

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

#[test]
fn a_refusal_prints_its_code_message_hint_and_stderr_as_text_and_its_fields_as_json() {
    let refusal = Refusal {
        stderr: b"Traceback: boom".to_vec(),
        ..Refusal::new(
            Code::PluginFailed,
            "plugin tasks exited 1",
            &Fields {
                plugin: Some("tasks".to_owned()),
                ..Fields::default()
            },
        )
    };
    let plain = render::refused(Format::Text, &refusal);
    assert_eq!(plain.code, 1);
    assert!(plain.stdout.is_empty());
    assert_eq!(
        text(&plain.stderr),
        "error: board::plugin::failed: plugin tasks exited 1\n\
         hint: the plugin exited without an answer; the end of its stderr follows this error\n\
         Traceback: boom\n"
    );
    let document = render::refused(Format::Json, &refusal);
    assert_eq!(document.code, 1);
    assert!(document.stderr.is_empty());
    assert_eq!(
        text(&document.stdout),
        "{\n  \"$type\": \"board/error/v1\",\n  \"code\": \"board::plugin::failed\",\n  \
         \"fields\": {\n    \"plugin\": \"tasks\"\n  },\n  \"hint\": \"the plugin exited \
         without an answer; the end of its stderr follows this error\",\n  \
         \"message\": \"plugin tasks exited 1\"\n}\n"
    );
}

#[test]
fn a_plugins_refusal_prints_no_hint_and_a_refused_command_line_exits_2() {
    let plugin = Refusal::plugin(
        "tasks::due::past".to_owned(),
        "the date is past".to_owned(),
        Map::new(),
    );
    let plain = render::refused(Format::Text, &plugin);
    assert_eq!(
        (text(&plain.stderr), plain.code),
        ("error: tasks::due::past: the date is past\n", 1)
    );
    let document = render::refused(Format::Json, &plugin);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&document.stdout).unwrap(),
        json!({
            "$type": "board/error/v1",
            "code": "tasks::due::past",
            "message": "the date is past",
        })
    );
    let usage = Refusal::new(Code::UsageInvalid, "no such command", &Fields::default());
    assert_eq!(render::refused(Format::Text, &usage).code, 2);
    assert_eq!(render::refused(Format::Json, &usage).code, 2);
}
