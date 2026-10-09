use board_validate::Schema;
use serde_json::json;

use super::*;

#[test]
fn output_its_schema_refuses_says_where_and_why_and_valid_output_is_admitted() {
    let schema = Schema::compile(json!({
        "type": "object",
        "properties": {
            "count": {"type": "integer"},
            "task": {"type": "string", "format": "board-ref"},
        },
    }))
    .unwrap();
    assert!(schema.validate(&json!({"count": 2, "task": "/a"})).is_ok());
    for (output, message) in [
        (
            json!({"count": "two"}),
            r#"its output fails its schema at "/count": "two" is not of type "integer""#,
        ),
        (
            json!({"task": "a//b"}),
            r#"its output holds a reference at "/task": "a//b" is not a logical path"#,
        ),
    ] {
        let reason = BadOutput::Output {
            source: schema.validate(&output).unwrap_err(),
        };
        assert_eq!(reason.to_string(), message, "{output}");
    }
}
