//! The manifest parses as Board reads it, the committed schemas are what
//! the record structs generate, the shipped heuristics are records those
//! schemas admit, `usable` says why a reader falls back beside the twin it
//! gives a reader, and `stale` counts calendar days. The tests through the
//! binary, in `board-cli`, cover `params.get` and `params.due` themselves.

use std::path::Path;
use std::time::Duration;

use board_model::{Code, LogicalPath, Refusal, canonical_json};
use board_ops::Elsewhere;
use serde::Deserialize;

use super::*;
use crate::due::stale;
use crate::records::Bounds;
use crate::stored::{Check, Fallback, Heuristic, Stored, is_name, shipped, usable};

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/params/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Params.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("params", "0.1.0")
    );
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, ["params/heuristic/v1", "params/value/v1"]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["get", "due"]);
    assert_eq!(manifest.description, skill_description());
}

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites them.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Params.schemas() {
        let file = (Path::new(env!("CARGO_MANIFEST_DIR")).join("types"))
            .join(format!("{}.schema.json", record_type.as_str()));
        let generated = canonical_json(&schema);
        if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
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

/// The compiled schema of `record_type`.
fn schema(record_type: &str) -> board_validate::Schema {
    let (_, schema) = (Params.schemas().into_iter())
        .find(|(name, _)| name.as_str() == record_type)
        .unwrap();
    board_validate::Schema::compile(schema).unwrap()
}

#[test]
fn the_shipped_heuristics_are_admitted_with_the_start_default_bounds_and_check_the_plan_set() {
    let heuristic = schema(HeuristicRecord::TYPE);
    let found: Vec<(&str, [f64; 5])> = (SHIPPED.iter())
        .map(|(name, text)| {
            let record: Value = serde_json::from_str(text).unwrap();
            heuristic.validate(&record).unwrap();
            assert_eq!(
                (&record["unit"], &record["consumer"]),
                (&json!("changed-lines"), &json!("controls")),
                "{name}"
            );
            let method = record["method"].as_str().unwrap();
            assert!(
                method.contains("top-level `value`") && method.contains("lenient default"),
                "{name}: {method}"
            );
            let number = |pointer: &str| record.pointer(pointer).unwrap().as_f64().unwrap();
            let numbers = [
                "/start",
                "/default",
                "/bounds/min",
                "/bounds/max",
                "/check/fails_above",
            ];
            (*name, numbers.map(number))
        })
        .collect();
    assert_eq!(
        found,
        [
            ("overflow-agent-grace", [100.0, 200.0, 25.0, 2000.0, 0.3]),
            ("overflow-owner-grace", [400.0, 800.0, 100.0, 8000.0, 0.5]),
        ]
    );
}

/// A value of `overflow-agent-grace` that found enough data, as its schema
/// admits it.
fn value() -> Value {
    json!({
        "$type": "params/value/v1",
        "heuristic": "overflow-agent-grace",
        "value": 120,
        "scopes": {"/repos/board": 240},
        "data": "enough",
        "evidence": "/suites/params-overflow-agent-grace/runs/20261108T000000Z",
        "derived": "2026-11-08T00:00:00Z",
        "by": {"program": "claude-code"},
    })
}

#[test]
fn the_value_schema_refuses_a_bad_data_scope_or_time_and_admits_their_twins() {
    let schema = schema(ValueRecord::TYPE);
    let with = |name: &str, field: Value| {
        let mut record = value();
        record[name] = field;
        record
    };
    for (refused, admitted) in [
        (
            with("data", json!("some")),
            with("data", json!("too-little")),
        ),
        (
            with(
                "heuristic",
                json!("/params/heuristics/overflow-agent-grace"),
            ),
            with("heuristic", json!("overflow-agent-grace")),
        ),
        (
            with("scopes", json!({"repos/board": 1})),
            with("scopes", json!({"/repos/board": 1})),
        ),
        (
            with("scopes", json!({"/repos/{repo}": 1})),
            with("scopes", json!({"/repos/repo": 1})),
        ),
        (
            with("scopes", json!({"/repos/board": "1"})),
            with("scopes", json!({"/repos/board": 1.5})),
        ),
        (
            with("derived", json!("2026-11-08T01:00:00+01:00")),
            with("derived", json!("2026-11-08T00:00:00Z")),
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
fn the_value_schema_finds_the_evidence_as_its_only_reference() {
    let schema = schema(ValueRecord::TYPE);
    let found: Vec<(String, Option<Vec<String>>)> = (schema.references(&value()).into_iter())
        .map(|reference| {
            let types =
                (reference.accepts).map(|types| types.iter().map(ToString::to_string).collect());
            (reference.pointer, types)
        })
        .collect();
    assert_eq!(
        found,
        [("/evidence".to_owned(), Some(vec!["runs/run/v1".to_owned()]))]
    );
}

#[test]
fn the_heuristic_schema_refuses_no_inputs_and_a_recheck_of_zero_and_admits_their_twins() {
    let schema = schema(HeuristicRecord::TYPE);
    let shipped: Value = serde_json::from_str(SHIPPED[0].1).unwrap();
    let with = |name: &str, field: Value| {
        let mut record = shipped.clone();
        record[name] = field;
        record
    };
    let input = shipped["inputs"][0].clone();
    for (refused, admitted) in [
        (with("inputs", json!([])), with("inputs", json!([input]))),
        (
            with("recheck_after", json!(0)),
            with("recheck_after", json!(1)),
        ),
        (
            with("unit", json!("Changed lines")),
            with("unit", json!("changed-lines")),
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

/// A host that reaches nothing of the board: an unknown capability and an
/// input that does not read are refused before any read.
struct Nowhere;

impl Host for Nowhere {
    fn query(
        &self,
        _: &str,
        _: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("a refused call queries nothing")
    }
    fn refs(
        &self,
        _: &[LogicalPath],
        _: board_ops::Absent,
        _: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("params resolves no refs")
    }
    fn get_each(
        &self,
        _: &[LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("a refused call reads nothing")
    }
    fn put(&self, _: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("params writes nothing")
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        panic!("params calls no plugin")
    }
    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        panic!("params opens no other board")
    }
    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<Refusal>> {
        panic!("params asks the owner nothing")
    }
    fn sync(&self, _: bool, _: Duration) -> Result<board_ops::Synced, board_ops::SyncError> {
        panic!("params syncs nothing")
    }
    fn mount_update(
        &self,
        _: &LogicalPath,
        _: Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        panic!("params updates no mount")
    }
    fn settings(&self) -> &board_ops::Settings {
        panic!("a refused call reads no settings")
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_a_known_one_reaches_its_own_check() {
    let call = |name: &str| Params.call(&Nowhere, &name.parse().unwrap(), json!({"extra": 1}));
    let refused = call("set").unwrap_err();
    assert_eq!(refused.code, "params::call::unknown_capability");
    assert_eq!(
        refused.message,
        "params offers no capability set; it offers get and due"
    );
    assert_eq!(call("due").unwrap_err().code, "params::call::bad_input");
}

/// `is_name` keeps a record out of the output exactly when `Slug` would
/// refuse its name there.
#[test]
fn is_name_agrees_with_the_slug_shape_on_refused_names_and_their_admitted_twins() {
    let schema = schema_of::<board_plugin::shape::Slug>(SchemaSettings::draft2020_12());
    let slug = board_validate::Schema::compile(schema).unwrap();
    for (refused, admitted) in [
        ("My_Value", "my-value"),
        ("nested/one", "nested-one"),
        ("-half", "half"),
        ("half-", "half-life"),
        ("half--life", "half-1"),
        ("", "1"),
    ] {
        assert!(!is_name(refused), "{refused}");
        assert!(slug.validate(&json!(refused)).is_err(), "{refused}");
        assert!(is_name(admitted), "{admitted}");
        slug.validate(&json!(admitted)).unwrap();
    }
}

#[test]
fn the_shipped_heuristics_are_read_with_the_default_and_bounds_the_plan_set() {
    let found: Vec<(String, f64, Bounds)> = (shipped().into_iter())
        .map(|(name, heuristic)| (name, heuristic.default, heuristic.bounds))
        .collect();
    let bounds = |min, max| Bounds { min, max };
    assert_eq!(
        found,
        [
            (
                "overflow-agent-grace".to_owned(),
                200.0,
                bounds(25.0, 2000.0)
            ),
            (
                "overflow-owner-grace".to_owned(),
                800.0,
                bounds(100.0, 8000.0)
            ),
        ]
    );
}

/// `value` as the plugin reads it, with `number` as its value and
/// `heuristic` as the heuristic it names.
fn stored(number: f64, heuristic: &str) -> Stored {
    let mut record = value();
    record["value"] = json!(number);
    record["heuristic"] = json!(heuristic);
    record.as_object_mut().unwrap().remove("scopes");
    Stored::deserialize(&record).unwrap()
}

/// A heuristic with bounds `min` to `max`, as the plugin reads it.
fn bounded(min: f64, max: f64) -> Heuristic {
    Heuristic {
        default: 14.0,
        bounds: Bounds { min, max },
        check: Check { fails_above: 0.2 },
        recheck_after: 30,
    }
}

const OWN: &str = "half-life";

#[test]
fn a_value_outside_its_bounds_falls_back_and_one_at_either_bound_is_used() {
    let heuristic = bounded(3.0, 28.0);
    assert_eq!(
        usable("half-life", &heuristic, Some(&stored(2.0, OWN))).unwrap_err(),
        Fallback::OutOfBounds {
            number: 2.0,
            scope: None
        }
    );
    assert_eq!(
        usable("half-life", &heuristic, Some(&stored(29.0, OWN))).unwrap_err(),
        Fallback::OutOfBounds {
            number: 29.0,
            scope: None
        }
    );
    for number in [3.0, 28.0] {
        let value = stored(number, OWN);
        assert_eq!(
            usable("half-life", &heuristic, Some(&value)).unwrap().value,
            number
        );
    }
}

#[test]
fn a_scope_outside_the_bounds_makes_the_whole_record_fall_back_and_its_twin_inside_them_is_used() {
    let heuristic = bounded(25.0, 2000.0);
    let scoped = |number: f64| {
        let mut record = value();
        record["scopes"] = json!({"/repos/board": number});
        Stored::deserialize(&record).unwrap()
    };
    let outside = scoped(2001.0);
    assert_eq!(
        usable("overflow-agent-grace", &heuristic, Some(&outside)).unwrap_err(),
        Fallback::OutOfBounds {
            number: 2001.0,
            scope: Some("/repos/board".to_owned())
        }
    );
    let inside = scoped(2000.0);
    assert!(usable("overflow-agent-grace", &heuristic, Some(&inside)).is_ok());
}

#[test]
fn a_value_naming_another_heuristic_falls_back_and_its_twin_naming_its_own_is_used() {
    let heuristic = bounded(3.0, 28.0);
    let other = "archive-after";
    assert_eq!(
        usable("half-life", &heuristic, Some(&stored(7.0, other))).unwrap_err(),
        Fallback::WrongHeuristic {
            named: other.to_owned()
        }
    );
    assert!(usable("half-life", &heuristic, Some(&stored(7.0, OWN))).is_ok());
}

#[test]
fn no_record_is_missing_and_too_little_data_is_judged_after_the_heuristic_and_before_the_bounds() {
    let heuristic = bounded(3.0, 28.0);
    assert_eq!(
        usable("half-life", &heuristic, None).unwrap_err(),
        Fallback::Missing
    );
    let little = |number, named| {
        let mut record = stored(number, named);
        record.data = records::Data::TooLittle;
        record
    };
    assert_eq!(
        usable("half-life", &heuristic, Some(&little(2.0, OWN))).unwrap_err(),
        Fallback::TooLittle
    );
    let other = "archive-after";
    assert_eq!(
        usable("half-life", &heuristic, Some(&little(7.0, other))).unwrap_err(),
        Fallback::WrongHeuristic {
            named: other.to_owned()
        }
    );
}

#[test]
fn a_value_is_stale_one_second_past_its_days_and_fresh_at_exactly_its_days() {
    let derived: jiff::Timestamp = "2026-11-01T00:00:00Z".parse().unwrap();
    let at_limit: jiff::Timestamp = "2026-12-01T00:00:00Z".parse().unwrap();
    let past: jiff::Timestamp = "2026-12-01T00:00:01Z".parse().unwrap();
    assert!(!stale(at_limit, Some(derived), 30));
    assert!(stale(past, Some(derived), 30));
    assert!(stale(past, None, 30));
}

#[test]
fn the_get_output_lists_its_sources_the_reasons_a_default_applies_and_its_warning_codes() {
    let output = schema_of::<Got>(SchemaSettings::draft2020_12().for_serialize());
    assert_eq!(
        output["properties"]["source"]["enum"],
        json!(["value", "default"])
    );
    assert_eq!(
        output["properties"]["reason"]["enum"],
        json!(["missing", "too-little", "out-of-bounds", "wrong-heuristic"])
    );
    assert_eq!(
        output["$defs"]["Warning"]["properties"]["code"]["enum"],
        json!([
            "params::value::missing",
            "params::value::out_of_bounds",
            "params::value::wrong_heuristic"
        ])
    );
}
