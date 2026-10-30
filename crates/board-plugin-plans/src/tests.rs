//! The manifest parses as Board reads it, each schema file is what its
//! struct generates, and a capability the plugin does not offer is refused.

use std::path::Path;

use board_model::canonical_json;
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
    let input = schema_of::<open::Input>(SchemaSettings::draft2020_12());
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

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites the files.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn each_committed_schema_file_is_what_its_record_struct_generates() {
    let shipped = Plans.schemas();
    let types = Path::new(env!("CARGO_MANIFEST_DIR")).join("types");
    let overwrite = std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite");
    assert_eq!(shipped.len(), 3);
    for (record_type, schema) in shipped {
        let file = types.join(format!("{}.schema.json", record_type.as_str()));
        let generated = canonical_json(&schema);
        if overwrite {
            std::fs::write(&file, generated.as_bytes()).unwrap();
        }
        let committed = std::fs::read(&file).unwrap();
        assert!(
            committed == generated.as_bytes(),
            "{} differs from what its struct generates; run this test with SNAPSHOTS=overwrite:\n{}",
            file.display(),
            String::from_utf8_lossy(generated.as_bytes())
        );
    }
}

/// A sample of each record validates, and Board finds exactly the
/// references the structs declare, with the types they accept.
#[test]
fn sample_records_validate_and_their_references_are_the_ones_the_structs_declare() {
    let schemas: std::collections::BTreeMap<String, Value> = (Plans.schemas().into_iter())
        .map(|(name, schema)| (name.as_str().to_owned(), schema))
        .collect();
    let compile = |name: &str| board_validate::Schema::compile(schemas[name].clone()).unwrap();
    let question = json!({
        "$type": "plans/question/v1",
        "question": "Which way?",
        "session": "local_0a1b",
        "asked": "2026-10-01T09:00:00Z",
        "status": "open",
        "options": [{"id": "a", "text": "A"}],
        "context_path": "/projects/a/notes/background",
    });
    let answer = json!({
        "$type": "plans/answer/v1",
        "question": "/projects/a/questions/q",
        "choice": "a",
        "words": "A, please.",
        "answered": "2026-10-01T10:00:00Z",
        "evidence": {"session": "s", "entry": "e", "quote": "A, please."},
    });
    let decision = json!({
        "$type": "plans/decision/v1",
        "decision": "A",
        "why": "It is simpler.",
        "session": "s",
        "decided": "2026-10-01T10:00:00Z",
        "refs": ["https://example.com/pull/1"],
    });
    let mut found = Vec::new();
    for (name, record) in [
        ("plans/question/v1", &question),
        ("plans/answer/v1", &answer),
        ("plans/decision/v1", &decision),
    ] {
        let schema = compile(name);
        schema.validate(record).unwrap();
        for reference in schema.references(record) {
            let accepts = reference.accepts.map(|types| {
                types
                    .iter()
                    .map(|kind| kind.as_str().to_owned())
                    .collect::<Vec<_>>()
            });
            found.push((name, reference.pointer, accepts));
        }
    }
    assert_eq!(
        found,
        [
            ("plans/question/v1", "/context_path".to_owned(), None),
            (
                "plans/answer/v1",
                "/question".to_owned(),
                Some(vec!["plans/question/v1".to_owned()])
            ),
        ]
    );
    // An optional field holds no null; the same record without it is admitted.
    let mut nulled = question.clone();
    nulled["recommendation"] = Value::Null;
    let refused = compile("plans/question/v1").validate(&nulled).unwrap_err();
    assert_eq!(refused.code(), board_model::Code::RecordInvalid);
    // A decision an agent wrote needs a reason; a hook's stub may leave it empty.
    let mut empty = decision.clone();
    empty["why"] = json!("");
    let refused = compile("plans/decision/v1").validate(&empty).unwrap_err();
    assert_eq!(refused.code(), board_model::Code::RecordInvalid);
    empty["written_by"] = json!("runs/pr-create");
    assert!(compile("plans/decision/v1").validate(&empty).is_ok());
}

/// The question schema declares every field `open` returns, and `open`'s
/// query names the type `QuestionRecord::TYPE` gives.
#[test]
fn open_reads_fields_the_question_struct_declares() {
    let question = &Plans.schemas()[0].1;
    for field in [
        "question",
        "session",
        "asked",
        "options",
        "recommendation",
        "blocks",
        "status",
    ] {
        assert!(question["properties"].get(field).is_some(), "{field}");
    }
    assert!(open::QUESTIONS.contains(&format!("type = '{}'", QuestionRecord::TYPE)));
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
