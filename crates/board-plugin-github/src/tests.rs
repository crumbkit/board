//! The manifest parses as Board reads it, the committed schemas are what
//! the record structs generate, a reference parses to its object's paths,
//! gh's run becomes an answer or `github::gh::failed`, and a capability the
//! plugin does not offer is refused.

use std::cell::RefCell;
use std::io;
use std::path::Path;

use board_model::canonical_json;
use board_ops::Run;
use board_plugin::child::{Exited, ProcessError};
use serde_json::json;

use super::*;
use objects::{Kind, Object};

/// A host whose only answer is the run of a process, which it records; a
/// test here reaches nothing else of the board.
struct Running {
    ran: RefCell<Vec<(String, Vec<String>)>>,
    answer: fn() -> Result<Exited, ProcessError>,
}

fn running(answer: fn() -> Result<Exited, ProcessError>) -> Running {
    Running {
        ran: RefCell::new(Vec::new()),
        answer,
    }
}

impl Host for Running {
    fn query(
        &self,
        _sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("this test reaches no query")
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("github resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("github reads records through a query")
    }

    fn put(&self, _lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("this test writes no record")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("github calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("github opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("github asks the owner nothing")
    }

    fn settings(&self) -> &board_ops::Settings {
        panic!("this test reads no settings")
    }

    fn run(&self, run: &Run<'_>) -> Result<Exited, ProcessError> {
        let ran = (run.program.to_owned(), run.arguments.to_vec());
        self.ran.borrow_mut().push(ran);
        assert_eq!(run.deadline.as_secs(), 20);
        (self.answer)()
    }
}

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites them.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Github.schemas() {
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
    let skill = include_str!("../../../plugins/github/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Github.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("github", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [PullRecord::TYPE, IssueRecord::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["ref"]);
    let declared = &super::manifest()["capabilities"][0]["env"];
    assert_eq!(
        declared,
        &json!(["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"])
    );
    assert_eq!(super::manifest()["description"], skill_description());
}

#[test]
fn every_input_property_carries_a_sentence_that_says_what_it_means() {
    let manifest = super::manifest();
    let input = &manifest["capabilities"][0]["inputSchema"];
    assert_eq!(input["additionalProperties"], false);
    let properties = input["properties"].as_object().unwrap();
    let names: Vec<&String> = properties.keys().collect();
    assert_eq!(names, ["ref"]);
    let sentence = properties["ref"]["description"].as_str().unwrap();
    assert!(sentence.ends_with('.'), "{sentence}");
}

#[test]
fn a_reference_parses_in_lower_case_to_its_paths_and_endpoint() {
    let object = Object::parse("Octo/.Wid_gets-2#12").unwrap();
    assert_eq!(
        (object.owner.as_str(), object.repo.as_str(), object.number),
        ("octo", ".wid_gets-2", 12)
    );
    assert_eq!(object.path(Kind::Pull), "/gh/octo/.wid_gets-2/pulls/12");
    assert_eq!(object.path(Kind::Issue), "/gh/octo/.wid_gets-2/issues/12");
    assert_eq!(object.endpoint(), "repos/octo/.wid_gets-2/issues/12");
    assert_eq!(Object::parse("octo/widgets"), None);
}

fn exited(code: Option<i32>, stdout: &str, stderr: &str) -> Exited {
    Exited {
        code,
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

/// What `gh::api` gives for the run `answer` returns, and how gh was run.
fn asked(
    answer: fn() -> Result<Exited, ProcessError>,
) -> Result<Option<Map<String, Value>>, PluginError> {
    let host = running(answer);
    let answered = gh::api(&host, "repos/octo/widgets/issues/7");
    let ran = host.ran.into_inner();
    let arguments = ["api", "repos/octo/widgets/issues/7"].map(str::to_owned);
    assert_eq!(ran, [("gh".to_owned(), arguments.to_vec())]);
    answered
}

/// The message of the `github::gh::failed` that `gh::api` gives.
fn failed(answer: fn() -> Result<Exited, ProcessError>) -> String {
    let failure = asked(answer).unwrap_err();
    assert_eq!(failure.code, "github::gh::failed");
    assert_eq!(failure.fields["endpoint"], "repos/octo/widgets/issues/7");
    failure.message
}

#[test]
fn gh_api_gives_the_object_gh_prints_and_none_for_githubs_404() {
    let object = asked(|| Ok(exited(Some(0), r#"{"title": "T"}"#, ""))).unwrap();
    assert_eq!(object.unwrap()["title"], "T");
    let missing = asked(|| Ok(exited(Some(1), "{}", "gh: Not Found (HTTP 404)\n"))).unwrap();
    assert_eq!(missing, None);
}

#[test]
fn gh_api_fails_naming_gh_s_error_when_gh_fails_cannot_start_or_prints_no_object() {
    assert_eq!(
        failed(|| Ok(exited(Some(4), "", "gh auth login\n"))),
        "`gh api repos/octo/widgets/issues/7` exited 4: gh auth login; check gh on PATH and \
         `gh auth status`"
    );
    let long = failed(|| Ok(exited(Some(1), "", &"x".repeat(2500))));
    assert!(
        long.contains(&format!("exited 1: {};", "x".repeat(2000))),
        "{long}"
    );
    assert!(!long.contains(&"x".repeat(2001)), "{long}");
    assert!(failed(|| Ok(exited(Some(0), "[1]", ""))).contains("printed no JSON object"));
    assert!(failed(|| Ok(exited(Some(0), "not json", ""))).contains("printed no JSON object"));
    let missing = failed(|| {
        Err(ProcessError::NotStarted {
            source: io::Error::from(io::ErrorKind::NotFound),
        })
    });
    assert!(missing.contains("did not start"), "{missing}");
    let slow = failed(|| Err(ProcessError::Timeout { stderr: Vec::new() }));
    assert!(slow.contains("did not finish: it ran past 20 s"), "{slow}");
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_ref_reaches_its_own_check() {
    let host = running(|| panic!("a refused call runs nothing"));
    let other: CapabilityName = "push".parse().unwrap();
    let refused = Github.call(&host, &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "github::call::unknown_capability");
    assert_eq!(
        refused.message,
        "github offers no capability push; it offers ref"
    );
    let known: CapabilityName = "ref".parse().unwrap();
    let refused = Github.call(&host, &known, json!({})).unwrap_err();
    assert_eq!(refused.code, "github::call::bad_input");
}
