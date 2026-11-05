//! The manifest parses as Board reads it and carries the skill's description,
//! the record comparison, the refusal of index codes and the attachment
//! scheme read as the Python plugin read them, and a capability the plugin
//! does not offer is refused.

use serde_json::json;

use super::*;
use to::{Line, refused, same, scheme};

/// A host that does nothing; each call here is refused before the plugin
/// reaches the board.
struct Nothing;

impl Host for Nothing {
    fn query(
        &self,
        _sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("a refused call runs no query")
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("a refused call resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("a refused call reads no record")
    }

    fn put(&self, _lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("a refused call writes no record")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("share calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("share opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("share asks the owner nothing")
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
        panic!("a refused call reads no settings")
    }
}

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/share/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Share.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("share", "0.1.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    assert_eq!(manifest.types, Vec::new());
    assert!(Share.schemas().is_empty(), "the plugin ships no types");
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["to"]);
    assert_eq!(super::manifest()["description"], skill_description());
}

#[test]
fn every_input_property_carries_a_sentence_that_says_what_it_means() {
    let manifest = super::manifest();
    let input = &manifest["capabilities"][0]["inputSchema"];
    assert_eq!(input["additionalProperties"], false);
    assert_eq!(input["required"], json!(["path", "to"]));
    let mut described = Vec::new();
    for (name, property) in input["properties"].as_object().unwrap() {
        let sentence = property["description"].as_str().unwrap();
        assert!(sentence.ends_with('.'), "{name}: {sentence}");
        assert!(
            !sentence.contains('\n'),
            "{name} reads as one line, as before: {sentence}"
        );
        described.push(name.as_str());
    }
    assert_eq!(described, ["dry_run", "local_attachments", "path", "to"]);
    for flag in ["dry_run", "local_attachments"] {
        assert_eq!(input["properties"][flag]["default"], false, "{flag}");
    }
    for path in ["path", "to"] {
        assert_eq!(input["properties"][path]["format"], "board-path", "{path}");
        assert_eq!(input["properties"][path]["minLength"], 1, "{path}");
    }
}

#[test]
fn a_record_held_with_true_where_the_source_has_1_is_different_and_one_in_another_key_order_is_the_same()
 {
    let line = Line::Record(json!({"a": 1, "b": "c"}));
    assert!(!same(&line, br#"{"a": true, "b": "c"}"#));
    assert!(same(&line, br#"{"b": "c", "a": 1}"#));
    assert!(!same(&line, b"not json"));
    let text = Line::Text("# A\n".to_owned());
    assert!(same(&text, b"# A\n"));
    assert!(!same(&text, b"# A"));
}

#[test]
fn a_record_held_with_minus_zero_where_the_source_has_zero_is_different_and_one_with_zero_is_the_same()
 {
    let line = Line::Record(json!({"x": 0.0}));
    assert!(!same(&line, br#"{"x": -0.0}"#));
    assert!(same(&line, br#"{"x": 0.0}"#));
}

#[test]
fn a_held_integer_board_cannot_hold_exactly_differs_from_the_float_it_reads_as_and_that_float_is_the_same()
 {
    let line = Line::Record(json!({"x": 1e20}));
    assert!(!same(&line, br#"{"x": 100000000000000000000}"#));
    assert!(same(&line, br#"{"x": 1e20}"#));
}

#[test]
fn an_index_code_other_than_a_broken_reference_is_refused_and_a_broken_reference_is_admitted() {
    assert!(refused("record", Some("board::record::invalid")));
    assert!(refused("file", Some("board::frontmatter::invalid")));
    assert!(!refused("record", Some("board::ref::dangling")));
    assert!(!refused("record", None));
    assert!(!refused("record", Some("")));
    assert!(!refused("file", Some("board::record::invalid")));
}

#[test]
fn an_attachment_location_has_a_scheme_only_when_letters_start_it_before_a_colon() {
    for (location, expected) in [
        ("s3://objects/9f86", "s3"),
        ("HTTPS://example.com/a", "https"),
        ("file:///Users/me/a", "file"),
        ("svn+ssh://host/a", "svn+ssh"),
        ("/Users/me/objects/9f86", ""),
        ("objects/9f86", ""),
        ("1s3://objects", ""),
        ("None", ""),
    ] {
        assert_eq!(scheme(location), expected, "{location}");
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_to_reaches_its_own_check() {
    let other: CapabilityName = "from".parse().unwrap();
    let refused = Share.call(&Nothing, &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "share::call::unknown_capability");
    assert_eq!(
        refused.message,
        "share offers no capability from; it offers to"
    );
    let known: CapabilityName = "to".parse().unwrap();
    let refused = Share
        .call(&Nothing, &known, json!({"path": "/evals/x", "to": "team"}))
        .unwrap_err();
    assert_eq!(refused.code, "share::mount::not_found");
}
