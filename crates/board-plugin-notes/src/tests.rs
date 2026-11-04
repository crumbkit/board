//! The manifest parses as Board reads it, the committed schema is what the
//! record struct generates, a note's frontmatter is read for `private` as
//! Reflect reads it, an inbox note's frontmatter and a note's clock are
//! written as the Python plugin wrote them, and a capability the plugin
//! does not offer is refused. These were the plugin's `test_manifest.py`
//! and `test_layout.py`; the slug cases are board-repos'.

use std::path::Path;
use std::time::{Duration, SystemTime};

use board_model::{LogicalPath, Refusal, canonical_json};
use board_ops::Elsewhere;
use serde_json::json;

use super::*;
use add::{clock, inbox};
use daily::private;

mod mount;

/// The committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_is_what_its_struct_generates() {
    for (record_type, schema) in Notes.schemas() {
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
    let skill = include_str!("../../../plugins/notes/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Notes.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("notes", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, ["notes/repo/v1"]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["add", "daily"]);
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
            "add.remote",
            "add.repo",
            "add.text",
            "add.topic",
            "daily.date",
            "daily.remote",
            "daily.repo"
        ]
    );
}

#[test]
fn every_output_property_is_required_so_a_null_is_never_left_out() {
    let manifest = super::manifest();
    let required =
        |index: usize| manifest["capabilities"][index]["outputSchema"]["required"].clone();
    assert_eq!(
        required(0),
        json!(["repo", "path", "commit", "candidates", "parked", "unpushed"])
    );
    assert_eq!(
        required(1),
        json!([
            "repo", "path", "exists", "private", "text", "commit", "unsynced"
        ])
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
        _: &[LogicalPath],
        _: board_ops::Absent,
        _: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("notes resolves no refs")
    }
    fn get_each(
        &self,
        _: &[LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("an unknown capability reads nothing")
    }
    fn put(&self, _: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("an unknown capability writes nothing")
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        panic!("notes calls no plugin")
    }
    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        panic!("an unknown capability opens no other board")
    }
    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<Refusal>> {
        panic!("notes asks the owner nothing")
    }
    fn sync(&self, _: bool, _: Duration) -> Result<board_ops::Synced, board_ops::SyncError> {
        panic!("an unknown capability syncs nothing")
    }
    fn mount_update(
        &self,
        _: &LogicalPath,
        _: Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        panic!("an unknown capability updates no mount")
    }
    fn settings(&self) -> &board_ops::Settings {
        panic!("an unknown capability reads no settings")
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_naming_what_it_offers() {
    let capability: CapabilityName = "find".parse().unwrap();
    let refused = Notes.call(&Nowhere, &capability, json!({})).unwrap_err();
    assert_eq!(refused.code, "notes::call::unknown_capability");
    assert_eq!(
        refused.message,
        "notes offers no capability find; it offers add and daily"
    );
}

#[test]
fn a_note_is_hidden_unless_every_private_key_in_its_frontmatter_is_plainly_false() {
    let cases = [
        ("private: true", true),
        ("private: on", true),
        ("private: \"yes\"", true),
        ("private: 1", true),
        ("private: maybe", true),
        ("private: !!bool true", true),
        ("\"private\": true", true),
        ("'private' : true", true),
        ("private : true", true),
        ("private: false\ntitle: x\nprivate: true", true),
        ("private: false", false),
        ("private: 'no'", false),
        ("private: off # shown", false),
        ("private: 0", false),
        ("PRIVATE: true", false),
        ("privately: true", false),
    ];
    for (frontmatter, hidden) in cases {
        let note = format!("---\nid: x\n{frontmatter}\n---\n# Note\n");
        assert_eq!(private(&note), hidden, "{frontmatter}");
    }
}

#[test]
fn a_byte_order_mark_before_the_frontmatter_does_not_show_a_private_note() {
    assert!(private("\u{feff}---\nprivate: true\n---\n"));
    assert!(!private("\u{feff}---\nprivate: false\n---\n"));
}

#[test]
fn a_private_line_in_the_body_after_the_frontmatter_does_not_hide_the_note() {
    assert!(!private("---\nid: x\n---\nprivate: true\n"));
    assert!(private("---\nprivate: true\n---\n"));
}

#[test]
fn lines_end_where_pythons_splitlines_ends_them() {
    assert!(private("---\r\nprivate: true\r\n---\r\n"));
    assert!(private("---\u{2028}private: true\u{2028}---\n"));
    assert!(!private("---\u{2028}private: off\u{2028}---\n"));
    assert!(!private("--- x\nprivate: true\n---\n"), "no frontmatter");
}

#[test]
fn an_inbox_notes_frontmatter_writes_each_value_as_json_and_leaves_out_what_is_absent() {
    let candidates = [
        "/repos/notes-home".to_owned(),
        "/repos/notes-work".to_owned(),
    ];
    let remote = "https://x-access-token:SECRET@github.com/crumbkit/board.git";
    assert_eq!(
        inbox(Some(remote), &candidates, Some("Board: \"names\" ü")),
        "---\nremote: \"crumbkit/board\"\ncandidates: [\"/repos/notes-home\", \"/repos/notes-work\"]\n\
         topic: \"Board: \\\"names\\\" ü\"\n---\n\n"
    );
    assert_eq!(inbox(None, &[], None), "---\ncandidates: []\n---\n\n");
}

#[test]
fn a_notes_clock_is_the_local_time_its_offset_gives_on_either_side_of_utc_midnight() {
    // 2026-11-04T23:30:15Z.
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_835_015);
    let shown = |offset| {
        let (date, hours, minutes, seconds) = clock(now, offset);
        format!("{date} {hours:02}:{minutes:02}:{seconds:02}")
    };
    assert_eq!(shown(0), "2026-11-04 23:30:15");
    assert_eq!(shown(3600), "2026-11-05 00:30:15");
    assert_eq!(shown(-12 * 3600), "2026-11-04 11:30:15");
    assert_eq!(shown(14 * 3600), "2026-11-05 13:30:15");
}
