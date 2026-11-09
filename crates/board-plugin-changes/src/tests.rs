//! The manifest parses as Board reads it, describes the plugin with
//! SKILL.md's description and each input with a sentence, and each
//! committed schema is what its struct generates.

use std::path::Path;

use board_model::canonical_json;
use board_plugin::VariableName;

use super::*;
use crate::flow::development;

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
            "changes/lifecycle/v1",
            "changes/plan/v1",
            "changes/prototype/v1",
            "changes/reopen/v1",
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
            ("lifecycle", vec![], None),
            ("brief", vec![], None),
            ("prototype", days.clone(), Some(120)),
            ("verdict", vec![], Some(120)),
            ("pause", vec![], None),
            ("resume", days.clone(), None),
            ("reopen", days, None),
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
        "plan.considered",
        "plan.owed",
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
        "reopen.path",
        "reopen.reason",
        "reopen.session",
        "reopen.program",
        "reopen.person",
        "reopen.agent",
        "reopen.over_limit",
        "reflow.path",
        "reflow.flow",
        "sync.dry_run",
        "sync.limit",
        "drop.path",
        "drop.reason",
        "lifecycle.doc",
        "lifecycle.external",
        "lifecycle.path",
        "lifecycle.text",
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
        "migrate.flows",
        "migrate.plans",
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

/// Every text the plugin ships: its skills, its README and its manifest,
/// which holds the capability and field help.
fn shipped_texts() -> Vec<(&'static str, String)> {
    vec![
        (
            "SKILL.md",
            include_str!("../../../plugins/changes/SKILL.md").to_owned(),
        ),
        (
            "README.md",
            include_str!("../../../plugins/changes/README.md").to_owned(),
        ),
        (
            "skills/flow/SKILL.md",
            include_str!("../../../plugins/changes/skills/flow/SKILL.md").to_owned(),
        ),
        (
            "skills/understanding-check/SKILL.md",
            include_str!("../../../plugins/changes/skills/understanding-check/SKILL.md").to_owned(),
        ),
        ("manifest", manifest().to_string()),
    ]
}

/// `text` with each run of white space as one space, so a sentence compares
/// across the line breaks of a Markdown file.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the flow's plan instruction says each of the new sections holds,
/// sentence by sentence, which the skill and the README repeat word for word.
const SECTION_RULES: [&str; 3] = [
    "Failure paths has one row per step that can fail: what fails, the state it leaves, the \
     recovery, and the order of the check against the destructive step it guards.",
    "Consumers lists every caller and reader of what the change produces or alters, and names \
     the files searched.",
    "Lifecycle starts with None and a reason, or names the record `board changes lifecycle` \
     writes and summarises its states in one line.",
];

#[test]
fn the_flow_the_skill_and_the_readme_say_what_each_new_section_holds_in_the_same_words() {
    let flow = development();
    let instruction = flat(&flow.phase("plan").unwrap().instruction);
    let skill = flat(include_str!("../../../plugins/changes/SKILL.md"));
    let readme = flat(include_str!("../../../plugins/changes/README.md"));
    for rule in SECTION_RULES {
        assert!(instruction.contains(rule), "the flow lacks: {rule}");
        assert!(skill.contains(rule), "SKILL.md lacks: {rule}");
        assert!(readme.contains(rule), "README.md lacks: {rule}");
    }
}

/// The name of one person's private notes repository, which no example
/// path in shipped text uses.
const PRIVATE_MOUNT: &str = "notes-personal";

/// The shipped texts that name the private notes repository.
fn naming_the_private_mount(texts: &[(&'static str, String)]) -> Vec<&'static str> {
    (texts.iter())
        .filter(|(_, text)| text.contains(PRIVATE_MOUNT))
        .map(|(name, _)| *name)
        .collect()
}

#[test]
fn a_text_naming_the_private_notes_repository_is_found_and_a_neutral_one_is_not() {
    let texts = [
        (
            "private",
            "such as /repos/notes-personal/tasks/<slug>.md".to_owned(),
        ),
        ("neutral", "such as /repos/notes/tasks/<slug>.md".to_owned()),
    ];
    assert_eq!(naming_the_private_mount(&texts), ["private"]);
}

#[test]
fn no_shipped_text_names_the_private_notes_repository() {
    assert_eq!(
        naming_the_private_mount(&shipped_texts()),
        Vec::<&str>::new()
    );
}

#[test]
fn the_skill_description_covers_starting_resuming_and_listing_work_within_the_skill_limit() {
    let description = skill_description();
    assert!(
        description.contains("start, resume or list work on a repository the board mounts"),
        "{description}"
    );
    assert!(description.contains("changes open"), "{description}");
    // Agent Skills cap a skill's description at 1,024 characters.
    assert!(description.chars().count() <= 1024, "{description}");
}
