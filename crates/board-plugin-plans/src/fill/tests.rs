use board_ops::{Plugin, With};
use serde_json::Value;

use crate::Plans;

/// The schema `Plans` ships for `record_type`.
fn schema(record_type: &str) -> Value {
    (Plans.schemas().into_iter())
        .find(|(name, _)| name.as_str() == record_type)
        .map(|(_, schema)| schema)
        .unwrap()
}

#[test]
fn plans_fills_who_and_when_of_a_question_and_a_decision_and_a_questions_status() {
    let fills: Vec<_> = (Plans.fills().iter())
        .map(|fill| (fill.record_type, fill.field))
        .collect();
    assert_eq!(
        fills,
        [
            ("plans/question/v1", "by"),
            ("plans/question/v1", "asked"),
            ("plans/question/v1", "status"),
            ("plans/decision/v1", "by"),
            ("plans/decision/v1", "decided"),
        ]
    );
}

#[test]
fn each_field_plans_fills_is_one_its_schema_requires() {
    for fill in Plans.fills() {
        let required = &schema(fill.record_type)["required"];
        assert!(
            (required.as_array().unwrap()).contains(&Value::from(fill.field)),
            "{} {}",
            fill.record_type,
            fill.field
        );
    }
}

#[test]
fn a_questions_status_is_filled_with_a_status_its_schema_admits() {
    let status = Plans.fills().iter().find(|fill| fill.field == "status");
    let Some(With::Text(text)) = status.map(|fill| &fill.with) else {
        panic!("status is filled with text");
    };
    let admitted = schema("plans/question/v1")["properties"]["status"].to_string();
    assert!(admitted.contains(&format!("\"{text}\"")), "{admitted}");
}
