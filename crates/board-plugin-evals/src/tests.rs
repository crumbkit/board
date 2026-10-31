//! The manifest parses as Board reads it, the shipped schema admits and
//! refuses what the Python plugin's did, and a capability the plugin does
//! not offer is refused.

use board_model::Code;
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

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/evals/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Evals.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("evals", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [EVAL_TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["about", "open"]);
    assert_eq!(manifest.description, skill_description());
}

#[test]
fn every_input_property_carries_a_sentence_and_about_takes_a_subject_or_a_mount() {
    let manifest = manifest();
    let [about, open] = [0, 1].map(|n| manifest["capabilities"][n]["inputSchema"].clone());
    for input in [&about, &open] {
        for (name, property) in input["properties"].as_object().unwrap() {
            let description = property["description"].as_str().unwrap();
            assert!(description.ends_with('.'), "{name}: {description}");
        }
        assert_eq!(input["additionalProperties"], false);
        let limit = &input["properties"]["limit"];
        assert_eq!(
            [&limit["minimum"], &limit["maximum"], &limit["default"]],
            [&json!(1), &json!(100), &json!(100)]
        );
        assert_eq!(
            limit.get("format"),
            None,
            "a Rust integer's format is left out"
        );
    }
    assert_eq!(
        about["oneOf"],
        json!([{"required": ["subject"]}, {"required": ["mount"]}])
    );
    for name in ["subject", "mount"] {
        let property = &about["properties"][name];
        assert_eq!(
            (&property["type"], &property["minLength"]),
            (&json!("string"), &json!(1))
        );
        assert_eq!(property.get("default"), None, "{name}");
    }
    let names: Vec<&str> = open["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(names, ["limit"]);
}

#[test]
fn the_output_schema_admits_an_answer_and_refuses_one_without_its_total() {
    let manifest = Evals.manifest();
    let output = &manifest.capabilities[0].output;
    let answer = json!({
        "evals": [{
            "path": "/evals/e",
            "question": "Is gix faster?",
            "subjects": ["gix"],
            "status": "concluded",
            "verdict": "supported",
            "decision": null,
        }],
        "total": 1,
        "truncated": false,
    });
    assert!(output.validate(&answer).is_ok());
    let mut partial = answer;
    partial.as_object_mut().unwrap().remove("total");
    assert!(output.validate(&partial).is_err());
}

/// A concluded eval that refers to a mount, a suite and a run.
fn concluded() -> Value {
    json!({
        "$type": "evals/eval/v1",
        "question": "Is gix faster?",
        "subjects": ["gix"],
        "status": "concluded",
        "verdict": "supported",
        "mounts": ["/repos/board"],
        "suites": ["/suites/s"],
        "runs": ["/suites/s/runs/1"],
        "written_by": "runs/test-run",
    })
}

#[test]
fn the_shipped_schema_finds_each_reference_with_its_type() {
    let [(name, schema)] = &Evals.schemas()[..] else {
        panic!("one schema")
    };
    assert_eq!(name.as_str(), EVAL_TYPE);
    let schema = board_validate::Schema::compile(schema.clone()).unwrap();
    let record = concluded();
    schema.validate(&record).unwrap();
    let found: Vec<(String, Option<Vec<String>>)> = (schema.references(&record).into_iter())
        .map(|reference| {
            (
                reference.pointer,
                reference
                    .accepts
                    .map(|types| types.iter().map(ToString::to_string).collect()),
            )
        })
        .collect();
    let typed = |name: &str| Some(vec![name.to_owned()]);
    assert_eq!(
        found,
        [
            ("/mounts/0".to_owned(), None),
            ("/runs/0".to_owned(), typed("runs/run/v1")),
            ("/suites/0".to_owned(), typed("runs/suite/v1")),
        ]
    );
}

#[test]
fn the_shipped_schema_refuses_a_concluded_eval_without_a_verdict_a_null_and_a_bad_writer_and_admits_their_twins()
 {
    let [(_, schema)] = &Evals.schemas()[..] else {
        panic!("one schema")
    };
    let schema = board_validate::Schema::compile(schema.clone()).unwrap();
    let with = |name: &str, value: Value| {
        let mut record = concluded();
        if value.is_null() && name == "verdict" {
            record.as_object_mut().unwrap().remove(name);
        } else {
            record[name] = value;
        }
        record
    };
    for (refused, admitted) in [
        (
            with("verdict", Value::Null),
            with("status", json!("running")),
        ),
        (
            with("decision", Value::Null),
            with("decision", json!("Pick.")),
        ),
        (
            with("written_by", json!("the hook")),
            with("written_by", json!("a/b")),
        ),
    ] {
        assert_eq!(
            schema.validate(&refused).unwrap_err().code(),
            Code::RecordInvalid,
            "{refused}"
        );
        schema.validate(&admitted).unwrap();
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_open_is_answered() {
    let other: CapabilityName = "runs".parse().unwrap();
    let refused = Evals.call(&empty(), &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "evals::call::unknown_capability");
    assert_eq!(
        refused.message,
        "evals offers no capability runs; it offers about and open"
    );
    let open: CapabilityName = "open".parse().unwrap();
    let answered = Evals.call(&empty(), &open, json!({})).unwrap();
    assert_eq!(
        answered,
        json!({"evals": [], "total": 0, "truncated": false})
    );
}

#[test]
fn input_that_does_not_read_is_refused_and_a_whole_number_written_as_a_decimal_is_read() {
    let open: CapabilityName = "open".parse().unwrap();
    let refused = Evals
        .call(&empty(), &open, json!({"limit": "x"}))
        .unwrap_err();
    assert_eq!(refused.code, "evals::call::bad_input");
    assert!(
        refused
            .message
            .contains("report this to Board's developers"),
        "{}",
        refused.message
    );
    assert!(Evals.call(&empty(), &open, json!({"limit": 5.0})).is_ok());
}
