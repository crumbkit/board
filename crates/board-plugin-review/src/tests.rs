//! The manifest parses as Board reads it, each committed schema is what its
//! struct generates, the comment's text, and the constants copied from the
//! changes plugin.

use std::path::Path;

use board_model::canonical_json;
use board_plugin::VariableName;

use super::*;

const PR: &str = "https://github.com/o/r/pull/7";

#[test]
fn the_manifest_parses_as_a_built_in_one_with_each_capabilitys_variables_and_time() {
    let manifest = Review.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("review", "0.2.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [BriefRecord::TYPE, ReportRecord::TYPE]);
    let found: Vec<(&str, Vec<&str>, Option<u64>)> = (manifest.capabilities.iter())
        .map(|capability| {
            (
                capability.name.as_str(),
                capability.env.iter().map(VariableName::as_str).collect(),
                capability.timeout.map(|timeout| timeout.as_secs()),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("run", run::ENV.to_vec(), Some(1800)),
            (
                "post",
                vec!["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"],
                Some(60)
            ),
            ("covered", vec![], None),
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
    assert_eq!(
        described,
        [
            "covered.base",
            "covered.change",
            "covered.head",
            "post.pr",
            "post.report",
            "run.brief"
        ]
    );
}

#[test]
fn a_runs_output_names_a_range_or_a_subject_as_a_brief_does() {
    let target = json!([{"required": ["range"]}, {"required": ["subject"]}]);
    let manifest = manifest();
    assert_eq!(manifest["capabilities"][0]["outputSchema"]["oneOf"], target);
    let (_, brief) = Review.schemas().remove(0);
    assert_eq!(brief["oneOf"], target);
}

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates, which is what the Python plugin
/// shipped. `SNAPSHOTS=overwrite` rewrites the files.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Review.schemas() {
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

fn report(fields: &Value) -> ReportRecord {
    let mut report = json!({
        "$type": ReportRecord::TYPE,
        "range": format!("{}..{}", "a".repeat(40), "b".repeat(40)),
        "patch_ids": [],
        "findings": [],
        "verdict": "changes",
        "by": {"program": "claude-code"},
    });
    for (name, value) in fields.as_object().unwrap() {
        report[name] = value.clone();
    }
    serde_json::from_value(report).unwrap()
}

#[test]
fn a_file_holding_backticks_gets_a_longer_code_span_and_a_plain_file_one_backtick() {
    let findings = json!([
        {"severity": "medium", "file": "a``b.rs", "text": "t"},
        {"severity": "low", "file": "c.rs", "text": "u"},
    ]);
    let text = post::comment(
        "/reviews/r1/report",
        PR,
        &report(&json!({"findings": findings})),
    );
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.contains(&"- **medium** ``` a``b.rs ``` t"), "{text}");
    assert!(lines.contains(&"- **low** `c.rs` u"), "{text}");
}

#[test]
fn a_report_with_no_findings_says_so_and_names_the_report_it_follows() {
    let fields = json!({"verdict": "approve", "follows": "/reviews/r0/report"});
    let text = post::comment("/reviews/r1/report", PR, &report(&fields));
    assert!(text.contains("\nNo findings.\n"), "{text}");
    assert!(text.contains("\nFollows /reviews/r0/report.\n"), "{text}");
}

/// A report names the agent's program as the reviewer when its file name
/// is a slug, and names nobody when it is not. The `claude` command is
/// recorded as `claude-code`.
#[test]
fn a_program_whose_name_is_no_slug_names_no_reviewer_and_its_twin_names_the_program() {
    let program = |name: &str| run::reviewer(name).and_then(|actor| actor.program);
    for (refused, admitted, recorded) in [
        ("/opt/My Agent", "/opt/my-agent", "my-agent"),
        ("bin/Claude", "bin/claude", "claude-code"),
        ("/", "claude", "claude-code"),
        ("codex!", "codex", "codex"),
    ] {
        assert_eq!((refused, program(refused)), (refused, None));
        assert_eq!(
            (admitted, program(admitted)),
            (admitted, Some(recorded.to_owned()))
        );
    }
}
