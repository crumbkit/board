//! The manifest parses and carries the skill's description, the committed
//! schema is what its struct generates, carries the share and local
//! keywords, and admits a record while refusing its malformed twins. The
//! tests through the binary, in `board-cli`, cover the capabilities.

use std::path::Path;

use board_model::canonical_json;
use serde_json::json;

use super::*;

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/priorities/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_names_the_type_and_capabilities_and_has_the_skills_description() {
    let manifest = Priorities.manifest();
    assert_eq!(manifest.name.as_str(), "priorities");
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, ["priorities/list/v1"]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["list", "set", "move", "drop", "confirm"]);
    assert_eq!(manifest.description, skill_description());
}

/// The committed schema file is the schema the struct generates.
/// `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_is_what_its_struct_generates() {
    let types = Path::new(env!("CARGO_MANIFEST_DIR")).join("types");
    let overwrite = std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite");
    for (record_type, schema) in Priorities.schemas() {
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

fn schema() -> Value {
    Priorities.schemas().remove(0).1
}

#[test]
fn the_schema_refuses_unknown_fields_keeps_out_of_shares_and_holds_covers_local() {
    let schema = schema();
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(schema["x-board-goes-with-share"], json!(false));
    let entry = &schema["$defs"]["Entry"];
    assert_eq!(entry["additionalProperties"], json!(false));
    let covered = &entry["properties"]["covers"]["items"];
    assert_eq!(
        (&covered["x-board-local"], &covered["x-board-type"]),
        (
            &json!(true),
            &json!(["changes/change/v2", "tasks/task/v2", "tasks/task/v3"])
        )
    );
}

/// An entry needs an id, a one-line title and covers; a horizon holds at
/// most seven entries and an entry covers at most twenty records.
#[test]
fn a_record_with_a_malformed_field_is_refused_and_its_twin_is_admitted() {
    let compiled = board_validate::Schema::compile(schema()).unwrap();
    let entry = json!({
        "id": "board-release",
        "title": "Board 0.1.0 release with the signed dmg",
        "covers": ["/tasks/release.md"],
        "confirmed": "2026-11-23",
    });
    let record = json!({"$type": "priorities/list/v1", "short": [entry], "long": []});
    compiled.validate(&record).unwrap();
    let mut seven = record.clone();
    seven["long"] = json!(vec![entry.clone(); 7]);
    compiled.validate(&seven).unwrap();
    for (key, value) in [
        ("id", json!("Board Release")),
        ("title", json!("two\nlines")),
        ("title", json!("word ".repeat(26).trim())),
        ("covers", json!(vec!["/tasks/a.md"; 21])),
        ("covers", json!(["tasks/a.md"])),
        ("confirmed", json!("23 November")),
        ("rank", json!(1)),
    ] {
        let mut refused = record.clone();
        refused["short"][0][key] = value;
        assert!(compiled.validate(&refused).is_err(), "{key}");
    }
    let mut eight = record.clone();
    eight["long"] = json!(vec![entry; 8]);
    assert!(compiled.validate(&eight).is_err());
    let mut unknown = record;
    unknown["owner"] = json!("ana");
    assert!(compiled.validate(&unknown).is_err());
}

/// An entry confirmed `days` before 2026-11-24 with `commits` since,
/// against the default 300 commits.
fn stale(days: i64, commits: Option<u32>) -> bool {
    let today = board_repos::Date::parse("2026-11-24").unwrap();
    list::is_stale(Some(today.plus(-days)), today, commits, 300.0)
}

#[test]
fn an_entry_is_stale_from_seven_days_after_its_confirmation() {
    assert!(!stale(6, Some(0)));
    assert!(stale(7, Some(0)));
}

#[test]
fn an_entry_is_stale_once_it_has_seen_more_commits_than_the_value() {
    assert!(stale(0, Some(301)));
    assert!(!stale(0, Some(300)));
}

#[test]
fn when_git_cannot_count_commits_the_days_alone_decide() {
    assert!(stale(8, None));
    assert!(!stale(1, None));
}

#[test]
fn an_entry_confirmed_after_today_is_not_stale() {
    assert!(!stale(-3, Some(0)));
}

#[test]
fn when_the_confirmed_day_does_not_parse_the_commits_alone_decide() {
    let today = board_repos::Date::parse("2026-11-24").unwrap();
    assert!(!list::is_stale(None, today, Some(300), 300.0));
    assert!(list::is_stale(None, today, Some(301), 300.0));
}
