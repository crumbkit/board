//! The manifest parses as Board reads it, each schema file ships, and a
//! capability the plugin does not offer is refused.

use std::path::Path;

use board_ops::{Limit, QueryError, QueryReport, Settings};
use serde_json::json;

use super::*;

/// A host that answers every query with no rows.
struct Empty(Settings);

impl Host for Empty {
    fn query(&self, _sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        Ok(QueryReport {
            columns: vec!["logical".into(), "code".into(), "body".into()],
            rows: Vec::new(),
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn settings(&self) -> &Settings {
        &self.0
    }
}

fn empty() -> Empty {
    Empty(Settings {
        now: std::time::SystemTime::UNIX_EPOCH,
        day_start: None,
        caller: None,
    })
}

#[test]
fn the_manifest_parses_as_board_reads_a_built_in_one_and_names_no_program_to_run() {
    let manifest = Plans.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("plans", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(
        types,
        ["plans/question/v1", "plans/answer/v1", "plans/decision/v1"]
    );
    let [open] = &manifest.capabilities[..] else {
        panic!("{:?}", manifest.capabilities)
    };
    assert_eq!(open.name.as_str(), "open");
    assert!(
        manifest
            .description
            .ends_with("in Board's repository says where each record lives.")
    );
}

#[test]
fn every_input_property_carries_a_sentence_that_says_what_it_means() {
    let input = open::input_schema();
    let properties = input["properties"].as_object().unwrap();
    let names: Vec<&str> = properties.keys().map(String::as_str).collect();
    assert_eq!(names, ["limit", "path"]);
    for (name, property) in properties {
        let description = property["description"].as_str().unwrap();
        assert!(description.ends_with('.'), "{name}: {description}");
    }
    assert_eq!(input["additionalProperties"], false);
    assert_eq!(input["required"], Value::Null);
    let limit = &input["properties"]["limit"];
    assert_eq!(
        (
            &limit["type"],
            &limit["minimum"],
            &limit["maximum"],
            &limit["default"]
        ),
        (&json!("integer"), &json!(1), &json!(10_000), &json!(100))
    );
    assert_eq!(
        limit.get("format"),
        None,
        "a Rust integer's format is left out"
    );
    let path = &input["properties"]["path"];
    assert_eq!(
        (&path["format"], &path["pattern"], &path["default"]),
        (&json!("board-path"), &json!("^/"), &json!("/"))
    );
}

#[test]
fn the_output_schema_admits_an_answer_and_refuses_one_without_its_total() {
    let manifest = Plans.manifest();
    let output = &manifest.capabilities[0].output;
    let answer = json!({
        "questions": [{
            "path": "/projects/a/questions/q",
            "question": "Which way?",
            "asked": "2026-10-01T09:00:00Z",
            "session": "local_0a1b",
            "options": [{"id": "a", "text": "A"}],
            "recommendation": null,
            "blocks": "the release",
        }],
        "total": 1,
        "truncated": false,
        "unreadable": 0,
    });
    assert!(output.validate(&answer).is_ok());
    let mut partial = answer;
    partial.as_object_mut().unwrap().remove("total");
    assert!(output.validate(&partial).is_err());
}

#[test]
fn the_plugin_ships_each_committed_schema_file_as_it_is_on_disk() {
    let shipped = Plans.schemas();
    let types = Path::new(env!("CARGO_MANIFEST_DIR")).join("types");
    assert_eq!(shipped.len(), 3);
    for (record_type, schema) in shipped {
        let file = types.join(format!("{}.schema.json", record_type.as_str()));
        let text = std::fs::read(&file).unwrap();
        assert_eq!(schema, parse_json(&text).unwrap(), "{}", file.display());
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_open_is_answered() {
    let host = empty();
    let call = |name: &str| Plans.call(&host, &name.parse().unwrap(), json!({}));
    let refused = call("closed").unwrap_err();
    assert_eq!(refused.code, "plans::call::unknown_capability");
    assert_eq!(
        refused.message,
        "plans offers no capability closed; it offers open"
    );
    let answered = call("open").unwrap();
    assert_eq!(answered["total"], 0);
}

#[test]
fn input_that_does_not_read_is_refused_and_a_whole_number_written_as_a_decimal_is_read() {
    let host = empty();
    let call = |input| Plans.call(&host, &"open".parse().unwrap(), input);
    let refused = call(json!({"limit": "two"})).unwrap_err();
    assert_eq!(refused.code, "plans::call::bad_input");
    assert!(call(json!({"limit": 2.0})).is_ok());
}
