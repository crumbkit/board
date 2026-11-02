//! The manifest parses as Board reads it, the committed schema is what the
//! record struct generates, the key and site checks agree with its patterns,
//! acli's run becomes a title or the reason there is none, and a capability
//! the plugin does not offer is refused.

use std::io;
use std::path::Path;

use board_model::canonical_json;
use board_plugin::child::{Exited, ProcessError};
use serde_json::json;

use super::*;
use issue::{is_key, is_site, title_from, title_of};

/// A host that does nothing; each test here is refused before the plugin
/// reaches the board.
struct Nothing;

impl Host for Nothing {
    fn query(
        &self,
        _sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("jira runs no query")
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("jira resolves no paths")
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
        panic!("jira calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("jira opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("jira asks the owner nothing")
    }

    fn settings(&self) -> &board_ops::Settings {
        panic!("a refused call reads no settings")
    }
}

/// The committed schema file holds, byte for byte, the canonical JSON of
/// the schema the struct generates. `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema file is a golden, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_is_what_its_struct_generates() {
    for (record_type, schema) in Jira.schemas() {
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
    let skill = include_str!("../../../plugins/jira/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Jira.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("jira", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [IssueRecord::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["ref"]);
    assert_eq!(super::manifest()["description"], skill_description());
}

#[test]
fn every_input_property_carries_a_sentence_that_says_what_it_means() {
    let manifest = super::manifest();
    let input = &manifest["capabilities"][0]["inputSchema"];
    assert_eq!(input["additionalProperties"], false);
    let mut described = Vec::new();
    for (name, property) in input["properties"].as_object().unwrap() {
        let sentence = property["description"].as_str().unwrap();
        assert!(sentence.ends_with('.'), "{name}: {sentence}");
        described.push(name.as_str());
    }
    assert_eq!(described, ["key", "site"]);
}

#[test]
fn the_key_and_site_checks_admit_exactly_what_the_schemas_patterns_admit() {
    let [(_, schema)] = <[_; 1]>::try_from(Jira.schemas()).unwrap();
    let schema = board_validate::Schema::compile(schema).unwrap();
    let issue = |site: &str, key: &str| {
        json!({
            "$type": "jira/issue/v1",
            "site": site,
            "key": key,
            "url": format!("https://{site}.atlassian.net/browse/{key}"),
        })
    };
    let keys = [
        "ACME-123", "AC_ME2-7", "AB-1", "acme-123", "ACME123", "ACME-0", "A-1", "ACME-12a",
        "1ACME-2", "ACME-1-2", "ACME-", "-1", "ÄB-1", "ACME-1\n",
    ];
    for key in keys {
        let admitted = schema.validate(&issue("acme", key)).is_ok();
        assert_eq!(is_key(key), admitted, "{key}");
    }
    let sites = [
        "acme",
        "acme-eu2",
        "a",
        "0",
        "Acme",
        "acme.atlassian.net",
        "-acme",
        "acme-",
        "acme/x",
        "",
    ];
    for site in sites {
        let admitted = schema.validate(&issue(site, "ACME-1")).is_ok();
        assert_eq!(is_site(site), admitted, "{site}");
    }
    assert!(is_key("AC_ME2-7") && is_site("acme-eu2"));
}

const VIEW: &str = r#"{"key": "ACME-123", "fields": {"summary": "Fix the login redirect"}}"#;
const NO_SUMMARY: &str = "acli printed no JSON work item with a summary";

/// What `title_of` gives for a run that exited with `code` and printed
/// `stdout` and `stderr`.
fn exited(code: Option<i32>, stdout: &str, stderr: &str) -> (Option<String>, Option<String>) {
    let ran = Ok(Exited {
        code,
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    });
    title_of(&ran, "acme")
}

/// What `title_of` gives for a run that did not finish.
fn stopped(error: ProcessError) -> Option<String> {
    let (title, note) = title_of(&Err(error), "acme");
    assert_eq!(title, None);
    note
}

fn note(text: &str) -> (Option<String>, Option<String>) {
    (None, Some(text.to_owned()))
}

#[test]
fn title_from_reads_the_summary_and_reports_output_without_one() {
    assert_eq!(
        title_from(VIEW, "acme"),
        (Some("Fix the login redirect".to_owned()), None)
    );
    assert_eq!(title_from("not json", "acme"), note(NO_SUMMARY));
    assert_eq!(
        title_from(r#"{"fields": {"summary": ""}}"#, "acme"),
        note(NO_SUMMARY)
    );
}

#[test]
fn title_from_drops_a_title_read_from_another_site_and_keeps_one_from_the_same_site() {
    let other =
        r#"{"self": "https://other.atlassian.net/rest/api/3/issue/1", "fields": {"summary": "T"}}"#;
    let same =
        r#"{"self": "https://acme.atlassian.net/rest/api/3/issue/1", "fields": {"summary": "T"}}"#;
    assert_eq!(
        title_from(other, "acme"),
        note("acli is signed in to another site (https://other.atlassian.net/rest/api/3/issue/1)")
    );
    assert_eq!(title_from(same, "acme"), (Some("T".to_owned()), None));
}

#[test]
fn a_run_that_exits_zero_gives_its_summary_and_one_that_fails_gives_the_end_of_its_stderr() {
    assert_eq!(
        exited(Some(0), VIEW, ""),
        (Some("Fix the login redirect".to_owned()), None)
    );
    assert_eq!(
        exited(
            Some(1),
            VIEW,
            "starting\nunauthorized: run acli jira auth login\n"
        ),
        note("acli exited 1: unauthorized: run acli jira auth login")
    );
    let long = "x".repeat(400);
    let (_, kept) = exited(Some(2), "", &long);
    assert_eq!(kept, Some(format!("acli exited 2: {}", "x".repeat(300))));
    assert_eq!(exited(Some(3), "", ""), note("acli exited 3: "));
    assert_eq!(
        exited(None, "", "killed"),
        note("acli exited on a signal: killed")
    );
}

#[test]
fn a_run_that_cannot_start_runs_past_its_deadline_or_prints_too_much_gives_why_there_is_no_title() {
    let missing = io::Error::from(io::ErrorKind::NotFound);
    assert_eq!(
        stopped(ProcessError::NotStarted { source: missing }).as_deref(),
        Some("acli is not on PATH, so the record has no title")
    );
    let denied = io::Error::new(io::ErrorKind::PermissionDenied, "permission denied");
    assert_eq!(
        stopped(ProcessError::NotStarted { source: denied }).as_deref(),
        Some("acli did not start (permission denied), so the record has no title")
    );
    assert_eq!(
        stopped(ProcessError::Timeout { stderr: Vec::new() }).as_deref(),
        Some("acli ran past 10 s, so the record has no title")
    );
    let large = ProcessError::TooLarge {
        limit: 16,
        stderr: Vec::new(),
    };
    assert_eq!(
        stopped(large).as_deref(),
        Some("acli printed more than 16 bytes, so the record has no title")
    );
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_ref_reaches_its_own_check() {
    let other: CapabilityName = "view".parse().unwrap();
    let refused = Jira.call(&Nothing, &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "jira::call::unknown_capability");
    assert_eq!(
        refused.message,
        "jira offers no capability view; it offers ref"
    );
    let known: CapabilityName = "ref".parse().unwrap();
    let refused = Jira
        .call(&Nothing, &known, json!({"key": "acme-1", "site": "acme"}))
        .unwrap_err();
    assert_eq!(refused.code, "jira::key::invalid");
    assert_eq!(refused.fields["key"], "acme-1");
    let refused = Jira
        .call(&Nothing, &known, json!({"key": "ACME-1", "site": "Acme"}))
        .unwrap_err();
    assert_eq!(refused.code, "jira::site::invalid");
    assert_eq!(refused.fields["site"], "Acme");
}
