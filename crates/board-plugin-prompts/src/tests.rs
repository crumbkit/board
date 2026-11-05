//! The manifest parses as Board reads it, the committed schemas are what
//! the record structs generate, rendering is pure and refuses each bad
//! value, and a capability the plugin does not offer is refused. These
//! were the plugin's `test_manifest.py` and `test_render.py`.

use std::path::Path;

use board_model::canonical_json;
use serde_json::json;

use super::*;
use records::RunsIn;
use render::{Values, body, checked, placed, render, repr, unused};

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites them.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Prompts.schemas() {
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

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/prompts/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Prompts.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("prompts", "0.1.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [PromptRecord::TYPE, ReposRecord::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["get", "list"]);
    assert_eq!(super::manifest()["description"], skill_description());
}

#[test]
fn every_input_property_carries_a_sentence_that_says_what_it_means() {
    let manifest = super::manifest();
    let mut described = Vec::new();
    for capability in manifest["capabilities"].as_array().unwrap() {
        let input = &capability["inputSchema"];
        assert_eq!(input["additionalProperties"], false);
        for (name, property) in input["properties"].as_object().unwrap() {
            let sentence = property["description"].as_str().unwrap();
            assert!(sentence.ends_with('.'), "{name}: {sentence}");
            described.push(format!("{}.{name}", capability["name"].as_str().unwrap()));
        }
    }
    assert_eq!(
        described,
        [
            "get.inputs",
            "get.path",
            "get.runs_in",
            "list.remote",
            "list.repo"
        ]
    );
}

#[test]
fn every_output_property_is_required_so_origin_unread_is_null_and_never_left_out() {
    let manifest = super::manifest();
    let list = &manifest["capabilities"][1]["outputSchema"];
    assert_eq!(
        list["required"],
        json!(["prompts", "total", "degraded", "repos", "origin_unread"])
    );
}

/// A host that reaches nothing of the board: the unknown capability is
/// refused before any read.
struct Nowhere;

impl Host for Nowhere {
    fn query(
        &self,
        _: &str,
        _: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("an unknown capability queries nothing")
    }
    fn refs(
        &self,
        _: &[board_model::LogicalPath],
        _: board_ops::Absent,
        _: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("prompts resolves no refs")
    }
    fn get_each(
        &self,
        _: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("an unknown capability reads nothing")
    }
    fn put(&self, _: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("prompts writes nothing")
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("prompts calls no plugin")
    }
    fn board(
        &self,
        _: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("prompts opens no other board")
    }
    fn decide(
        &self,
        _: &str,
        _: &board_model::LogicalPath,
        _: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("prompts asks the owner nothing")
    }
    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("this host syncs nothing")
    }
    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("this host updates no mount")
    }
    fn settings(&self) -> &board_ops::Settings {
        panic!("an unknown capability reads no settings")
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_naming_what_it_offers() {
    let capability: CapabilityName = "none".parse().unwrap();
    let refused = Prompts.call(&Nowhere, &capability, json!({})).unwrap_err();
    assert_eq!(refused.code, "prompts::call::unknown_capability");
    assert_eq!(
        refused.message,
        "prompts offers no capability none; it offers get and list"
    );
}

fn declared() -> Values {
    values(&[("repo", "The repository."), ("pr", "The pull request.")])
}

fn values(pairs: &[(&str, &str)]) -> Values {
    (pairs.iter())
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn the_body_is_the_text_after_the_line_closing_the_frontmatter() {
    assert_eq!(
        body("---\ntitle: x\n---\nYou are {pr}.\n"),
        "You are {pr}.\n"
    );
    assert_eq!(
        body("\u{feff}---\r\ntitle: x\r\n...\r\nBody\r\n"),
        "Body\r\n"
    );
    assert_eq!(body("---\ntitle: x\n---\n"), "");
    assert_eq!(body("---\ntitle: x\n"), "");
}

#[test]
fn each_declared_input_is_replaced_and_every_other_brace_is_left_as_written() {
    let text = r#"Review #{pr} in {repo}; keep {prr}, {"a": 1} and {Pr}."#;
    assert_eq!(
        render(
            text,
            &values(&[("repo", "example-org/widgets"), ("pr", "2140")])
        ),
        r#"Review #2140 in example-org/widgets; keep {prr}, {"a": 1} and {Pr}."#
    );
}

#[test]
fn rendering_is_one_pass_so_a_value_naming_another_input_stays_literal() {
    let given = values(&[("repo", "{pr}"), ("pr", "2140")]);
    assert_eq!(render("{repo} then {pr}", &given), "{pr} then 2140");
}

#[test]
fn unused_lists_each_declared_input_the_body_never_names() {
    assert_eq!(unused(&declared(), "Review #{pr}."), ["repo"]);
    assert_eq!(
        unused(&declared(), "Review #{pr} in {repo}."),
        Vec::<String>::new()
    );
}

fn refused(given: &[(&str, &str)]) -> (String, Value) {
    let error = checked(&declared(), &values(given)).unwrap_err();
    (error.code, Value::Object(error.fields))
}

#[test]
fn every_declared_input_needs_a_value_and_each_missing_one_is_named() {
    assert_eq!(
        refused(&[]),
        (
            "prompts::input::missing".to_owned(),
            json!({"inputs": ["pr", "repo"]})
        )
    );
    let both = values(&[("repo", "a/b"), ("pr", "1")]);
    assert_eq!(checked(&declared(), &both).unwrap(), both);
}

#[test]
fn a_value_naming_no_declared_input_is_refused_by_name() {
    let error = checked(
        &declared(),
        &values(&[("repo", "a/b"), ("pr", "1"), ("focus", "x")]),
    )
    .unwrap_err();
    assert_eq!(error.code, "prompts::input::unknown");
    assert_eq!(Value::Object(error.fields), json!({"inputs": ["focus"]}));
    assert_eq!(
        error.message,
        "no input is named focus; the prompt takes pr, repo"
    );
}

#[test]
fn a_value_over_200_characters_spanning_lines_or_holding_a_control_character_is_refused() {
    let long = "x".repeat(201);
    for value in [
        long.as_str(),
        "two\nlines",
        "carriage\rreturn",
        "tab\there",
        "bell\u{7}",
        "sep\u{2028}",
    ] {
        assert_eq!(
            refused(&[("repo", "a/b"), ("pr", value)]),
            (
                "prompts::input::bad_value".to_owned(),
                json!({"inputs": ["pr"]})
            ),
            "{value:?}"
        );
    }
}

#[test]
fn a_value_of_200_characters_on_one_line_is_admitted() {
    let value = "\u{e9}".repeat(200);
    let given = values(&[("repo", "a/b"), ("pr", &value)]);
    assert_eq!(checked(&declared(), &given).unwrap()["pr"], value);
}

#[test]
fn a_format_character_such_as_a_direction_override_or_zero_width_space_is_a_bad_value() {
    for value in ["21\u{202e}40", "21\u{200b}40", "\u{feff}2140"] {
        assert_eq!(
            refused(&[("repo", "a/b"), ("pr", value)]).0,
            "prompts::input::bad_value"
        );
    }
    let given = values(&[("repo", "a/b"), ("pr", "2140 \u{e9}")]);
    assert_eq!(checked(&declared(), &given).unwrap(), given);
}

/// The place `key` names at `value`, as a record writes it.
fn place(key: &str, value: &str) -> RunsIn {
    serde_json::from_value(json!({key: value})).unwrap()
}

#[test]
fn a_runs_in_naming_dot_or_dot_dot_or_holding_a_control_or_format_character_is_refused() {
    let none = Values::new();
    for (key, value) in [
        ("remote", "example-org/.."),
        ("remote", "example-org/."),
        ("directory", "/srv/\u{202e}exe.txt"),
        ("directory", "/a\r/b"),
        ("directory", "/a\tb"),
    ] {
        let error = placed(&place(key, value), &none).unwrap_err();
        assert_eq!(error.code, "prompts::runs_in::invalid", "{value:?}");
    }
}

#[test]
fn a_runs_in_with_dots_inside_a_name_or_path_is_admitted() {
    let none = Values::new();
    for (key, value) in [
        ("remote", "example-org/.github"),
        ("remote", "example-org/a..b"),
        ("directory", "/srv/a..b/.config"),
    ] {
        assert_eq!(
            placed(&place(key, value), &none).unwrap().place(),
            (key, value)
        );
    }
}

#[test]
fn a_refused_runs_in_names_its_rendered_value_as_python_wrote_it_and_the_inputs_it_used() {
    let given = values(&[("base", "relative")]);
    let error = placed(&place("directory", "{base}/work"), &given).unwrap_err();
    assert_eq!(
        error.message,
        "runs_in.directory renders as 'relative/work', which is not an absolute path or one \
         under ~/ without a .. segment; check the values of the inputs: base"
    );
    assert_eq!(
        Value::Object(error.fields),
        json!({"runs_in": {"directory": "relative/work"}, "inputs": ["base"]})
    );
    assert_eq!(repr("it's"), r#""it's""#);
    assert_eq!(repr("a\tb\u{202e}"), "'a\\tb\\u202e'");
    assert_eq!(repr("caf\u{e9}"), "'caf\u{e9}'");
}
