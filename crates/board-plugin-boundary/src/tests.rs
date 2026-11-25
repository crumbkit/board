//! The manifest parses as Board reads it, each committed schema is what its
//! struct generates, and each refuses a bad record whose twin it admits.

use std::path::Path;

use board_model::{Code, canonical_json};
use board_plugin::VariableName;
use serde_json::json;

use super::*;

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/boundary/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Boundary.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("boundary", "0.1.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [BoundaryRecord::TYPE, CrossingRecord::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["check", "covers", "verify", "record"]);
    let check = &manifest.capabilities[0];
    assert_eq!(
        check.timeout,
        Some(std::time::Duration::from_secs(CHECK_TIMEOUT))
    );
    let env: Vec<&str> = check.env.iter().map(VariableName::as_str).collect();
    assert_eq!(env, AGENT_VARIABLES);
    assert_eq!(manifest.description, skill_description());
}

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites the
/// files.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Boundary.schemas() {
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

/// `record` with each of `fields` set, or removed when null.
fn with(mut record: Value, fields: &Value) -> Value {
    for (name, value) in fields.as_object().unwrap() {
        if value.is_null() {
            record.as_object_mut().unwrap().remove(name);
        } else {
            record[name] = value.clone();
        }
    }
    record
}

fn compiled(record_type: &str) -> board_validate::Schema {
    let (_, schema) = (Boundary.schemas().into_iter())
        .find(|(name, _)| name.as_str() == record_type)
        .unwrap();
    board_validate::Schema::compile(schema).unwrap()
}

#[test]
fn the_boundary_schema_refuses_a_boundary_without_checks_or_with_a_bad_field_and_admits_its_twin() {
    let schema = compiled(BoundaryRecord::TYPE);
    let check = json!({"rule": "/rules/names", "parts": ["*"]});
    let boundary = json!({
        "$type": "boundary/boundary/v1",
        "to": "github:octo/*",
        "actions": ["github.*"],
        "checks": [check],
    });
    for (refused, admitted) in [
        (json!({"checks": []}), json!({})),
        (json!({"actions": []}), json!({"actions": ["*"]})),
        (
            json!({"action": ["github.*"]}),
            json!({"from": ["/repos/board"]}),
        ),
        (json!({"to": ""}), json!({"to": "*"})),
        (
            json!({"checks": [{"rule": "/rules/names", "parts": []}]}),
            json!({"checks": [{"rule": "/rules/names", "parts": ["pr.*"]}]}),
        ),
        (json!({"to": null}), json!({"from": null})),
        // A boundary written before the rename names its rule `control`.
        (
            json!({"checks": [{"rule": "/rules/names", "control": "/controls/names", "parts": ["*"]}]}),
            json!({"checks": [{"control": "/controls/names", "parts": ["*"]}]}),
        ),
    ] {
        let (refused, admitted) = (
            with(boundary.clone(), &refused),
            with(boundary.clone(), &admitted),
        );
        assert_eq!(
            schema.validate(&refused).unwrap_err().code(),
            Code::RecordInvalid,
            "{refused}"
        );
        schema.validate(&admitted).unwrap();
    }
}

#[test]
fn the_crossing_schema_refuses_a_bad_digest_null_or_unknown_field_and_admits_its_twin() {
    let schema = compiled(CrossingRecord::TYPE);
    let digest = format!("sha256:{}", "ab".repeat(32));
    let crossing = json!({
        "$type": "boundary/crossing/v1",
        "digest": digest,
        "action": "github.pr-create",
        "to": "github:octo/board",
        "boundaries": ["/boundaries/octo"],
        "verdicts": [{
            "rule": "/rules/names",
            "part": "pr.body",
            "checker": "pattern",
            "status": "failed",
            "findings": [{"text": "acme-123", "line": 1}],
        }],
    });
    for (refused, admitted) in [
        (json!({"digest": "ab".repeat(32)}), json!({})),
        (
            json!({"digest": format!("sha256:{}", "AB".repeat(32))}),
            json!({}),
        ),
        (json!({"override": "x"}), json!({"override": digest})),
        (
            json!({"verdicts": [{"rule": "/rules/names", "part": "p", "status": "late", "findings": []}]}),
            json!({"verdicts": [{"rule": "/rules/names", "part": "p", "status": "stated", "findings": []}]}),
        ),
        (
            json!({"from": null, "via": "/repos/board"}),
            json!({"from": "/repos/board"}),
        ),
        (json!({"passed": true}), json!({})),
        (
            json!({"verdicts": [{"rule": "/rules/names", "part": "p", "checker": "rubric", "status": "passed", "findings": []}]}),
            json!({"verdicts": [{"rule": "/rules/names", "part": "p", "checker": "version", "status": "passed", "findings": []}]}),
        ),
        // A crossing written before the rename names the rule `control` and
        // its checker `rule`, and mixes neither name into the other form.
        (
            json!({"verdicts": [{"control": "/controls/names", "part": "p", "rule": "rubric", "status": "passed", "findings": []}]}),
            json!({"verdicts": [{"control": "/controls/names", "part": "p", "rule": "version", "status": "passed", "findings": []}]}),
        ),
        (
            json!({"verdicts": [{"control": "/controls/names", "part": "p", "checker": "version", "status": "passed", "findings": []}]}),
            json!({"verdicts": [{"control": "/controls/names", "part": "p", "status": "passed", "findings": []}]}),
        ),
    ] {
        let (refused, admitted) = (
            with(crossing.clone(), &refused),
            with(crossing.clone(), &admitted),
        );
        assert_eq!(
            schema.validate(&refused).unwrap_err().code(),
            Code::RecordInvalid,
            "{refused}"
        );
        schema.validate(&admitted).unwrap();
    }
}

/// A verdict that names its rule `control` and its checker `rule`, as a
/// crossing written before the rename and `rules.evaluate` do, reads as the
/// one with the new names, and a boundary's `control` reads as `rule`.
#[test]
fn a_verdict_and_a_check_with_the_former_names_read_as_their_twins_with_the_new() {
    let fields = |rule: &str, checker: &str| json!([{rule: "/rules/names", "part": "p", checker: "pattern", "status": "failed", "findings": [{"text": "x"}]}]);
    let before = records::verdicts(&fields("control", "rule")).unwrap();
    let now = records::verdicts(&fields("rule", "checker")).unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(&now).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&now).unwrap(),
        fields("rule", "checker")
    );
    let check = |name: &str| {
        let record: BoundaryRecord = serde_json::from_value(json!({
            "$type": "boundary/boundary/v1",
            "to": "*",
            "actions": ["*"],
            "checks": [{name: "/rules/names", "parts": ["*"]}],
        }))
        .unwrap();
        record.checks[0].rule.clone()
    };
    assert_eq!(check("control"), check("rule"));
}

/// `rules.evaluate` names a result's rule `rule` and its checker `checker`.
/// The boundary reads its output in those names alone, while a stored
/// crossing written before the rename still reads in either.
#[test]
fn the_results_of_rules_evaluate_read_in_the_new_names_and_are_refused_in_the_former() {
    let result = |rule: &str, checker: &str| json!([{rule: "/rules/names", "part": "p", checker: "pattern", "status": "failed", "findings": [{"text": "x"}]}]);
    let now = records::evaluated(&result("rule", "checker")).unwrap();
    assert_eq!(
        serde_json::to_value(&now).unwrap(),
        result("rule", "checker")
    );
    assert!(records::evaluated(&result("control", "rule")).is_err());
    assert!(records::evaluated(&result("control", "checker")).is_err());
    assert!(records::verdicts(&result("control", "rule")).is_ok());
}

#[test]
fn every_input_property_carries_a_sentence() {
    let manifest = manifest();
    for capability in manifest["capabilities"].as_array().unwrap() {
        let input = &capability["inputSchema"];
        for (name, property) in input["properties"].as_object().unwrap() {
            let description = property["description"].as_str().unwrap();
            assert!(description.ends_with('.'), "{name}: {description}");
        }
        assert_eq!(input["additionalProperties"], false);
    }
}

/// The blob another writer's crossing gets.
const THEIRS: &str = "1111111111111111111111111111111111111111";

/// A host whose board holds no valid crossing, which answers a query for
/// the blob at a path from what it holds, and which applies each put
/// line's expect as Board does under its put lock: a line whose expect the
/// held blob does not match refuses the whole batch as stale.
#[derive(Default)]
struct Racing {
    /// The path another writer stores just before the next put.
    first: std::cell::RefCell<Option<String>>,
    /// Each path and the blob it holds.
    held: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
    landed: std::cell::RefCell<Vec<Vec<Value>>>,
}

impl Host for Racing {
    fn query(
        &self,
        sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        let path = (sql.rsplit_once("logical = '"))
            .and_then(|(_, rest)| rest.strip_suffix('\''))
            .unwrap_or_default();
        let rows = match self.held.borrow().get(path) {
            Some(blob) if sql.starts_with("SELECT blob FROM entries") => vec![vec![json!(blob)]],
            _ => Vec::new(),
        };
        Ok(board_ops::QueryReport {
            columns: vec!["blob".into()],
            rows,
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        unreachable!("a crossing's write resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        unreachable!("a crossing's write reads no files")
    }

    fn put(&self, lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        if let Some(path) = self.first.borrow_mut().take() {
            self.held.borrow_mut().insert(path, THEIRS.to_owned());
        }
        for line in lines {
            let path = line["path"].as_str().unwrap();
            let held = self.held.borrow().get(path).cloned();
            let matches = match line["expect"].as_str() {
                None => true,
                Some("none") => held.is_none(),
                Some(blob) => held.as_deref() == Some(blob),
            };
            if !matches {
                return Err(board_ops::PutError::Stale {
                    path: path.parse().unwrap(),
                    found: held.as_deref().and_then(board_model::ObjectId::from_hex),
                });
            }
        }
        for line in lines {
            let path = line["path"].as_str().unwrap().to_owned();
            let blob = "2222222222222222222222222222222222222222".to_owned();
            self.held.borrow_mut().insert(path, blob);
        }
        self.landed.borrow_mut().push(lines.to_vec());
        Ok(board_ops::PutReport {
            writes: Vec::new(),
            removed: Vec::new(),
            absent: Vec::new(),
            commit: None,
        })
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        unreachable!("a crossing's write calls no plugin")
    }

    fn board(&self, _elsewhere: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        unreachable!("a crossing's write opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<Refusal>> {
        unreachable!("a crossing's write asks the owner nothing")
    }

    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("a crossing's write syncs nothing")
    }

    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("a crossing's write updates no mount")
    }

    fn settings(&self) -> &board_ops::Settings {
        unreachable!("a crossing's write reads no settings")
    }
}

/// A crossing that passed, of a digest `crossing::find` finds none of.
fn new_crossing() -> CrossingRecord {
    serde_json::from_value(json!({
        "$type": "boundary/crossing/v1",
        "digest": format!("sha256:{}", "ab".repeat(32)),
        "action": "github.pr-create",
        "to": "github:octo/board",
        "boundaries": [],
        "verdicts": [],
    }))
    .unwrap()
}

#[test]
fn a_new_crossing_is_written_expecting_nothing_where_find_found_none() {
    let host = Racing::default();
    let crossing = new_crossing();
    assert!(crossing::find(&host, &crossing.digest).unwrap().is_none());
    crossing::write(&host, &crossing, None).unwrap();
    let landed = host.landed.borrow();
    assert_eq!(landed[0][0]["path"], crossing::path(&crossing.digest));
    assert_eq!(landed[0][0]["expect"], "none", "{}", landed[0][0]);
}

#[test]
fn a_crossing_another_check_writes_first_makes_the_write_stale_and_keeps_theirs() {
    let host = Racing::default();
    let crossing = new_crossing();
    let at = crossing::path(&crossing.digest);
    *host.first.borrow_mut() = Some(at.clone());
    let refused = crossing::write(&host, &crossing, None).unwrap_err();
    assert_eq!(
        refused.fields["code"], "board::put::stale",
        "{}",
        refused.message
    );
    assert!(host.landed.borrow().is_empty(), "nothing is written");
    assert_eq!(
        host.held.borrow().get(&at).map(String::as_str),
        Some(THEIRS)
    );
}

/// The blob of a degraded record at the crossing's path, which
/// `crossing::find` does not find.
const DEGRADED: &str = "3333333333333333333333333333333333333333";

#[test]
fn a_degraded_record_at_the_path_is_overwritten_and_refused_as_stale_when_another_check_writes_first()
 {
    for racing in [false, true] {
        let host = Racing::default();
        let crossing = new_crossing();
        let at = crossing::path(&crossing.digest);
        host.held
            .borrow_mut()
            .insert(at.clone(), DEGRADED.to_owned());
        assert!(crossing::find(&host, &crossing.digest).unwrap().is_none());
        if racing {
            *host.first.borrow_mut() = Some(at.clone());
        }
        let written = crossing::write(&host, &crossing, None);
        if racing {
            let refused = written.unwrap_err();
            assert_eq!(
                refused.fields["code"], "board::put::stale",
                "{}",
                refused.message
            );
            assert!(host.landed.borrow().is_empty(), "nothing is written");
        } else {
            written.unwrap();
            let landed = host.landed.borrow();
            assert_eq!(landed[0][0]["expect"], DEGRADED, "{}", landed[0][0]);
        }
    }
}
