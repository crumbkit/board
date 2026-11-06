//! The manifest parses as Board reads it, describes the plugin with
//! SKILL.md's description and each input with a sentence, and each
//! committed schema is what its struct generates.

use std::path::Path;

use board_model::canonical_json;
use board_plugin::VariableName;

use super::*;

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/changes/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_with_each_capabilitys_variables_and_time() {
    let manifest = Changes.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("changes", "0.1.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    assert_eq!(manifest.description, skill_description());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(
        types,
        [
            "changes/change/v1",
            "changes/change/v2",
            "changes/flow/v1",
            "changes/plan/v1",
            "changes/prototype/v1",
            "changes/run/v1",
            "changes/stop/v1",
            "changes/understanding/v1"
        ]
    );
    let found: Vec<(&str, Vec<&str>, Option<u64>)> = (manifest.capabilities.iter())
        .map(|capability| {
            (
                capability.name.as_str(),
                capability.env.iter().map(VariableName::as_str).collect(),
                capability.timeout.map(|timeout| timeout.as_secs()),
            )
        })
        .collect();
    let days = vec!["BOARD_CHANGES_ACTIVE_DAYS"];
    assert_eq!(
        found,
        [
            ("start", vec![], Some(120)),
            ("next", vec![], None),
            ("attach", vec![], None),
            ("advance", days.clone(), None),
            ("open", vec![], None),
            ("pr", vec![], None),
            ("plan", vec![], None),
            ("brief", vec![], None),
            ("prototype", days.clone(), Some(120)),
            ("verdict", vec![], Some(120)),
            ("pause", vec![], None),
            ("resume", days, None),
            ("reflow", vec![], None),
            (
                "sync",
                vec!["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"],
                Some(600)
            ),
            ("drop", vec![], None),
            ("migrate", vec![], Some(600)),
            (
                "run",
                vec![
                    "BOARD_AGENT",
                    "ANTHROPIC_API_KEY",
                    "CLAUDE_CODE_OAUTH_TOKEN",
                    "BOARD_CHANGES_ACTIVE_DAYS",
                    "TMPDIR"
                ],
                Some(3600)
            ),
            ("stop", vec![], None),
        ]
    );
}

#[test]
fn every_input_property_carries_a_sentence_that_says_what_it_means() {
    let mut described = Vec::new();
    for capability in manifest()["capabilities"].as_array().unwrap() {
        let properties = capability["inputSchema"]["properties"].as_object().unwrap();
        for (name, field) in properties {
            let description = field["description"].as_str().unwrap_or_default();
            assert!(description.ends_with('.'), "{name}: {description}");
            described.push(format!("{}.{name}", capability["name"].as_str().unwrap()));
        }
    }
    described.sort();
    let mut expected = vec![
        "start.mount",
        "start.title",
        "start.name",
        "start.handle",
        "start.task",
        "start.task_repo",
        "start.flow",
        "start.suite",
        "start.over_limit",
        "next.path",
        "attach.path",
        "attach.ref",
        "attach.ticket",
        "attach.pull",
        "attach.task",
        "attach.route",
        "attach.head",
        "attach.suite",
        "advance.path",
        "advance.over_limit",
        "open.kind",
        "open.limit",
        "pr.path",
        "plan.path",
        "plan.text",
        "plan.uses",
        "plan.paths",
        "brief.path",
        "prototype.mount",
        "prototype.question",
        "prototype.parent",
        "prototype.title",
        "prototype.name",
        "prototype.over_limit",
        "verdict.path",
        "verdict.state",
        "verdict.verdict",
        "verdict.artifact",
        "pause.path",
        "resume.path",
        "resume.over_limit",
        "reflow.path",
        "reflow.flow",
        "sync.dry_run",
        "sync.limit",
        "drop.path",
        "drop.reason",
        "run.path",
        "run.mount",
        "run.title",
        "run.name",
        "run.task",
        "run.flow",
        "run.over_limit",
        "run.through",
        "run.until",
        "run.inline",
        "run.budget",
        "run.sandbox",
        "run.for",
        "stop.path",
        "stop.reason",
        "stop.question",
        "stop.summary",
        "migrate.apply",
    ];
    expected.sort_unstable();
    assert_eq!(described, expected);
}

#[test]
fn attach_takes_at_least_one_thing_to_change() {
    let manifest = manifest();
    let attach = &manifest["capabilities"][2];
    assert_eq!(attach["name"], "attach");
    let branches = attach["inputSchema"]["anyOf"].as_array().unwrap();
    assert_eq!(branches.len(), 7);
    assert_eq!(branches[0], json!({"required": ["ref"]}));
}

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates, which is what the plugin
/// ships. `SNAPSHOTS=overwrite` rewrites the files.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Changes.schemas() {
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

/// This repository's own board holds a copy of the committed
/// `changes/flow/v1` schema, byte for byte, so `board check` in a Board
/// checkout validates the flow files the crate ships. `SNAPSHOTS=overwrite`
/// rewrites the copy from the committed schema.
#[expect(
    clippy::disallowed_methods,
    reason = "the board's copy is a golden, selected and written as crumbkit-test's are"
)]
#[test]
fn the_boards_own_flow_schema_is_the_committed_one() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let name = format!("{}.schema.json", flow::Flow::TYPE);
    let committed = std::fs::read(crate_root.join("types").join(&name)).unwrap();
    let copy = crate_root.join("../../.board/types").join(&name);
    if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::write(&copy, &committed).unwrap();
    }
    let board = std::fs::read(&copy).unwrap();
    assert!(
        board == committed,
        "{} differs from the committed {name}; run this test with SNAPSHOTS=overwrite",
        copy.display()
    );
}
