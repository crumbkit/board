//! What `report` prints for a board of rules, groups and suites, read
//! through the recording host `check`'s tests use, with git on its `PATH`.

use std::ffi::OsString;

use board_model::canonical_json;
use board_ops::{Plugin, Variables};
use board_scenario::Scenario;
use serde_json::json;

use super::*;
use crate::Rules;
use crate::check::tests::{Recording, enabling, enabling_in};

/// Where git is on every machine the tests run on.
const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

/// `host` with git on its `PATH` and `variables` set.
fn with_git(mut host: Recording, variables: &[(&str, &str)]) -> Recording {
    let mut set = vec![("PATH".to_owned(), OsString::from(PATH))];
    set.extend((variables.iter()).map(|(name, value)| ((*name).to_owned(), OsString::from(value))));
    host.settings.environment = Variables(set);
    host
}

/// The type `report` prints.
const REPORT: &str = "rules/report/v2";

/// A board that enables rules, runs, boundary, github, plans and review, holding
/// `files`, with git on the host's `PATH` and `variables` set.
fn board(files: &[(&str, &str)], variables: &[(&str, &str)]) -> (Scenario, Recording) {
    let plugins = ["rules", "runs", "boundary", "github", "plans", "review"];
    let (scenario, host) = enabling(&plugins, files);
    (scenario, with_git(host, variables))
}

/// A `rules/rule/v1` record on `paths` with `fields` added.
fn rule(paths: &[&str], fields: &Value) -> String {
    let mut record = json!({
        "$type": "rules/rule/v1",
        "requirement": "R.",
        "purpose": "P.",
        "paths": paths,
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record.to_string()
}

fn words(max: u64) -> Value {
    json!({"checker": {"check": "budget", "unit": "words", "max": max}})
}

fn pinned(at_least: &str) -> Value {
    json!({"checker": {"check": "version", "at_least": at_least, "capture": "channel = \"([0-9.]+)\""}})
}

/// A `rules/group/v1` record of `members`.
fn group(title: &str, members: &[&str]) -> String {
    json!({"$type": "rules/group/v1", "title": title, "purpose": "P.", "members": members})
        .to_string()
}

/// The report of `/` named `owner/app`.
fn report_of(host: &Recording) -> Value {
    report(host, &json!({"repository": "owner/app"})).unwrap()
}

/// Each row's id and result.
fn results(report: &Value) -> Vec<(String, String)> {
    (report["rules"].as_array().unwrap().iter())
        .map(|row| {
            (
                row["id"].as_str().unwrap().to_owned(),
                row["result"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// The row of `id`.
fn row<'a>(report: &'a Value, id: &str) -> &'a Value {
    (report["rules"].as_array().unwrap().iter())
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("no row {id}: {report}"))
}

/// The schema the plugin ships for `name`, compiled.
fn shipped(name: &str) -> board_validate::Schema {
    let (_, schema) = (Rules.schemas().into_iter())
        .find(|(record_type, _)| record_type.as_str() == name)
        .unwrap();
    board_validate::Schema::compile(schema).unwrap()
}

fn pair(id: &str, result: &str) -> (String, String) {
    (id.to_owned(), result.to_owned())
}

/// A board with one rule of each result `check` measures, a rule with no
/// checker, one that reaches nothing and one outside the imposed set; with
/// `broken`, also a degraded rule and one whose capture is invalid.
fn every_result(broken: bool) -> (Scenario, Recording) {
    let budget_within = rule(&["docs/*"], &words(9));
    let budget_over = rule(&["docs/*"], &words(1));
    let behind = rule(&["rust-toolchain.toml"], &pinned("1.99"));
    let within = rule(&["rust-toolchain.toml"], &pinned("1.80"));
    let written = rule(&["docs/*"], &json!({}));
    let dangling = rule(&["docs/*"], &json!({"applies_to": ["/nothing/here"]}));
    let capture = rule(
        &["rust-toolchain.toml"],
        &json!({"checker": {"check": "version", "at_least": "1", "capture": "no group"}}),
    );
    let elsewhere = rule(&["nowhere/*"], &json!({}));
    let mut files = vec![
        ("docs/a.md", "one two three"),
        ("rust-toolchain.toml", "channel = \"1.90\"\n"),
        ("rules/budget/within.json", budget_within.as_str()),
        ("rules/budget/over.json", budget_over.as_str()),
        ("rules/version/behind.json", behind.as_str()),
        ("rules/version/within.json", within.as_str()),
        ("rules/written.json", written.as_str()),
        ("rules/elsewhere.json", elsewhere.as_str()),
        ("other/outside.json", written.as_str()),
    ];
    if broken {
        files.push(("rules/broken.json", dangling.as_str()));
        files.push(("rules/capture.json", capture.as_str()));
    }
    board(&files, &[])
}

#[test]
fn every_result_comes_out_and_a_degraded_rule_is_one_unreadable_row_while_the_others_are_measured()
{
    let (_scenario, host) = every_result(true);
    let report = report_of(&host);
    assert_eq!(
        results(&report),
        [
            pair("/rules/broken", "unreadable"),
            pair("/rules/budget/over", "over"),
            pair("/rules/budget/within", "within"),
            pair("/rules/capture", "unreadable"),
            pair("/rules/version/behind", "behind"),
            pair("/rules/version/within", "within"),
            pair("/rules/written", "written"),
        ],
        "a rule outside the imposed root, and one that reaches no file, is no row: {report}"
    );
    assert_eq!(
        row(&report, "/rules/broken")["code"],
        "board::ref::dangling"
    );
    assert_eq!(row(&report, "/rules/broken")["requirement"], "R.");
    assert_eq!(
        row(&report, "/rules/capture")["code"],
        "rules::capture::invalid"
    );
    assert_eq!(row(&report, "/rules/budget/within")["code"], Value::Null);
    let over = row(&report, "/rules/budget/over");
    assert_eq!((&over["used"], &over["max"]), (&json!(3), &json!(1)));
    assert_eq!(over["files"], json!(["/docs/a.md"]));
    let behind = row(&report, "/rules/version/behind");
    assert_eq!(
        behind["behind"],
        json!([{"file": "/rust-toolchain.toml", "line": 1, "found": "1.90"}])
    );
    let refused: Vec<(&Value, &Value)> = (report["refused"].as_array().unwrap().iter())
        .map(|refusal| (&refusal["code"], &refusal["rules"]))
        .collect();
    assert_eq!(
        refused,
        [
            (&json!("board::ref::dangling"), &json!(["/rules/broken"])),
            (
                &json!("rules::capture::invalid"),
                &json!(["/rules/capture"])
            ),
        ]
    );
    assert_eq!(report["drift"]["not_reached"], json!(["/rules/elsewhere"]));
    assert_eq!(report["imposed"], json!({"root": "/rules"}));
    assert_eq!(report["$type"], REPORT);
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn check_refuses_a_board_with_a_degraded_rule_and_its_twin_without_one_agrees_with_the_report() {
    let (_scenario, host) = every_result(true);
    let refused = check::check(&host, &json!({})).unwrap_err();
    assert_eq!(refused.code, "rules::rule::degraded");
    report_of(&host);
    let (_scenario, host) = every_result(false);
    let checked = check::check(&host, &json!({})).unwrap();
    let report = report_of(&host);
    for budget in checked["measured"].as_array().unwrap() {
        let row = row(&report, budget["rule"].as_str().unwrap());
        assert_eq!(
            (&row["used"], &row["max"]),
            (&budget["used"], &budget["max"])
        );
    }
    for pinned in checked["versions"].as_array().unwrap() {
        let row = row(&report, pinned["rule"].as_str().unwrap());
        let found = |pins: &Value| -> Vec<(Value, Value)> {
            (pins.as_array().unwrap().iter())
                .map(|pin| (pin["line"].clone(), pin["found"].clone()))
                .collect()
        };
        assert_eq!(found(&pinned["behind"]), found(&row["behind"]));
    }
    assert_eq!(
        (checked["measured"].as_array().unwrap().len())
            + checked["versions"].as_array().unwrap().len(),
        4
    );
}

#[test]
fn a_rule_in_two_groups_counts_in_both_and_each_group_rolls_up_to_its_state() {
    let over = rule(&["src/*"], &words(1));
    let within = rule(&["src/*"], &words(99));
    let plain = rule(&["docs/*"], &json!({}));
    let dangling = rule(&["docs/*"], &json!({"applies_to": ["/nothing/here"]}));
    let go = rule(&["go/*"], &words(1));
    let groups = [
        group("Rust", &["/rules/rust/*"]),
        group("Server", &["/rules/*-server"]),
        group("PII", &["/rules/rust/pii"]),
        group("Docs", &["/rules/docs/plain"]),
        group("Broken", &["/rules/docs/*", "/rules/rust/pii"]),
        group("None", &["/rules/python/*"]),
        group("Go", &["/rules/go/vet"]),
    ];
    let (_scenario, host) = board(
        &[
            ("src/main.rs", "fn main() {}"),
            ("docs/a.md", "one two three"),
            ("rules/rust/version-server.json", &over),
            ("rules/rust/pii.json", &within),
            ("rules/docs/plain.json", &plain),
            ("rules/docs/broken.json", &dangling),
            ("rules/go/vet.json", &go),
            ("rules/groups/rust.json", &groups[0]),
            ("rules/groups/server.json", &groups[1]),
            ("rules/groups/pii.json", &groups[2]),
            ("rules/groups/docs.json", &groups[3]),
            ("rules/groups/broken.json", &groups[4]),
            ("rules/groups/none.json", &groups[5]),
            ("rules/groups/go.json", &groups[6]),
        ],
        &[],
    );
    let report = report(&host, &json!({"path": "/src", "repository": "owner/app"})).unwrap();
    let states: Vec<(&Value, &Value)> = (report["groups"].as_array().unwrap().iter())
        .map(|group| (&group["id"], &group["state"]))
        .collect();
    assert_eq!(
        states,
        [
            (&json!("/rules/groups/broken"), &json!("unreadable")),
            (&json!("/rules/groups/docs"), &json!("not_reached")),
            (&json!("/rules/groups/go"), &json!("not_reached")),
            (&json!("/rules/groups/none"), &json!("not_reached")),
            (&json!("/rules/groups/pii"), &json!("passing")),
            (&json!("/rules/groups/rust"), &json!("failing")),
            (&json!("/rules/groups/server"), &json!("failing")),
        ],
        "{report}"
    );
    let rust = &report["groups"][5]["counts"];
    assert_eq!((&rust["over"], &rust["within"]), (&json!(1), &json!(1)));
    assert_eq!(report["groups"][6]["counts"]["over"], 1);
    assert_eq!(
        row(&report, "/rules/rust/version-server")["groups"],
        json!(["/rules/groups/rust", "/rules/groups/server"])
    );
    let unmatched: Vec<&Value> = (report["refused"].as_array().unwrap().iter())
        .filter(|refusal| refusal["code"] == "rules::report::member_unmatched")
        .map(|refusal| &refusal["rules"])
        .collect();
    assert_eq!(unmatched, [&json!(["/rules/groups/none"])]);
    assert!(
        (report["rules"].as_array().unwrap().iter())
            .all(|row| !row["id"].as_str().unwrap().starts_with("/rules/groups/")),
        "the rule index never lists a group as a rule: {report}"
    );
    let whole = report_of(&host);
    let go = (whole["groups"].as_array().unwrap().iter())
        .find(|group| group["id"] == "/rules/groups/go")
        .unwrap();
    assert_eq!(
        go["state"], "not_reached",
        "under the root a group of rules that reach no file is not reached: {whole}"
    );
    assert!(
        (whole["rules"].as_array().unwrap().iter()).all(|row| row["id"] != "/rules/go/vet"),
        "under the root a rule that reaches no file is no row: {whole}"
    );
    assert_eq!(whole["drift"]["not_reached"], json!(["/rules/go/vet"]));
    let applying = crate::for_path::for_path(&host, &json!({"path": "/rules/groups/rust"}));
    assert_eq!(applying.unwrap()["rules"], json!([]));
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn a_group_rolls_up_failing_over_unreadable_over_passing_over_written() {
    assert_eq!(
        rolled_up(&[Outcome::Written, Outcome::Unmeasured]),
        GroupState::Written
    );
    assert_eq!(rolled_up(&[]), GroupState::NotReached);
    assert_eq!(
        rolled_up(&[Outcome::Written, Outcome::Zero]),
        GroupState::Passing
    );
    assert_eq!(
        rolled_up(&[Outcome::Unreadable, Outcome::Within]),
        GroupState::Unreadable
    );
    assert_eq!(
        rolled_up(&[Outcome::Unreadable, Outcome::Stale]),
        GroupState::Failing
    );
}

#[test]
fn the_group_schema_admits_a_group_and_refuses_one_with_no_members_or_a_relative_member() {
    let schema = shipped("rules/group/v1");
    let record: Value = serde_json::from_str(&group("Rust", &["/rules/rust/*"])).unwrap();
    schema.validate(&record).unwrap();
    let empty: Value = serde_json::from_str(&group("Rust", &[])).unwrap();
    assert!(schema.validate(&empty).is_err());
    let relative: Value = serde_json::from_str(&group("Rust", &["rules/rust/*"])).unwrap();
    assert!(schema.validate(&relative).is_err());
}

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn source_run_is_filled_only_from_all_four_ci_variables() {
    let all = [
        ("GITHUB_REPOSITORY", "owner/app"),
        ("GITHUB_RUN_ID", "991"),
        ("GITHUB_RUN_NUMBER", "17"),
        ("GITHUB_SHA", SHA),
    ];
    let (_scenario, host) = board(&[], &all);
    let report = report_of(&host);
    assert_eq!(
        report["source_run"],
        json!({"repository": "owner/app", "run_id": 991, "run_number": 17, "commit": SHA})
    );
    assert_eq!(report["refused"], json!([]));
    let (_scenario, host) = board(&[], &[]);
    let report = report_of(&host);
    assert_eq!(report["source_run"], Value::Null);
    assert_eq!(report["refused"], json!([]));
    let mut malformed = all;
    malformed[3] = ("GITHUB_SHA", "0123456");
    let (_scenario, host) = board(&[], &malformed);
    let report = report_of(&host);
    assert_eq!(report["source_run"], Value::Null);
    assert_eq!(
        report["refused"][0]["code"],
        "rules::report::ci_env_invalid"
    );
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn a_repository_with_no_name_is_refused_and_its_twin_named_by_flag_is_reported() {
    let budget = rule(&["*"], &json!({}));
    let (_scenario, host) = board(&[("rules/a.json", &budget)], &[]);
    let refused = report(&host, &json!({})).unwrap_err();
    assert_eq!(refused.code, "rules::report::no_repository");
    let named = report(
        &host,
        &json!({"repository": "https://github.com/Owner/App.git"}),
    )
    .unwrap();
    assert_eq!(named["repository"]["name"], "owner/app");
    let version = named["board"]["version"].as_str().unwrap();
    assert_eq!(version.split('.').count(), 3, "{version}");
}

#[test]
fn lists_are_cut_at_20_and_say_how_many_they_left_out() {
    let files: Vec<(String, String)> = (0..23)
        .map(|at| (format!("docs/{at:02}.md"), "one".to_owned()))
        .collect();
    let mut all: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let budget = rule(
        &["docs/*"],
        &json!({"checker": {"check": "budget", "unit": "words", "max": 0, "per": "file"}}),
    );
    all.push(("rules/each.json", &budget));
    let (_scenario, host) = board(&all, &[]);
    let report = report_of(&host);
    let each = row(&report, "/rules/each");
    assert_eq!(each["files"].as_array().unwrap().len(), LISTED);
    assert_eq!(each["over"].as_array().unwrap().len(), LISTED);
    assert_eq!(
        each["omitted"],
        json!({"files": 3, "over": 3, "behind": 0, "pins": 0})
    );
    assert_eq!(each["result"], "over");
}

/// Who ran the suite, which no report shows.
const RUNNER: &str = "zelda-qx";

/// A compliance suite naming `subject` of `kind`, and its one run, whose
/// `versions` are `versions` and whose case `fmt` failed with a note.
fn compliance(kind: &str, subject: &str, versions: &Value) -> [(&'static str, String); 2] {
    let suite = json!({
        "$type": "runs/suite/v1",
        "subjects": [subject],
        "kind": kind,
        "method": "Check the repository.",
        "cases": [{"id": "fmt"}, {"id": "lint"}],
    });
    let run = json!({
        "$type": "runs/run/v1",
        "suite": "/suites/app",
        "by": {"program": "claude-code", "person": RUNNER},
        "started": "1970-01-01T00:00:00Z",
        "finished": "1970-01-01T01:00:00+01:00",
        "scope": "full",
        "dirty": false,
        "versions": versions,
        "results": [
            {"case": "fmt", "outcome": "fail", "note": "two files are not formatted"},
            {"case": "lint", "outcome": "pass"},
        ],
    });
    [
        ("suites/app.json", suite.to_string()),
        ("suites/app/runs/1.json", run.to_string()),
    ]
}

/// The commits of [`history`].
struct Commits {
    run: board_model::ObjectId,
    side: board_model::ObjectId,
    head: board_model::ObjectId,
}

/// A repository whose history is a run's commit, then three more on the
/// branch HEAD names, and a side branch off the run's commit.
fn history() -> (Scenario, Commits) {
    let scenario = Scenario::new();
    let run = scenario.commit_at(&[("a", "0")], &[], 10, "run");
    let side = scenario.commit_at(&[("a", "side")], &[run], 11, "side");
    let mut head = run;
    for at in 1..=3 {
        head = scenario.commit_at(&[("a", &at.to_string())], &[head], 20 + at, "next");
    }
    (scenario, Commits { run, side, head })
}

/// The report of a board laid over `scenario`'s history, holding
/// [`compliance`] of `kind` naming `subject` for `commit`.
fn reported_at(scenario: &Scenario, kind: &str, subject: &str, commit: &str) -> Value {
    reported_with(scenario, kind, subject, &json!({"commit": commit}))
}

/// [`reported_at`] with the run's `versions` given whole.
fn reported_with(scenario: &Scenario, kind: &str, subject: &str, versions: &Value) -> Value {
    let [suite, run] = compliance(kind, subject, versions);
    let files = [(suite.0, suite.1.as_str()), (run.0, run.1.as_str())];
    let host = with_git(enabling_in(scenario, &["rules", "runs"], &files), &[]);
    report_of(&host)
}

#[test]
fn commits_since_counts_a_run_three_commits_back_and_says_why_it_cannot_count_a_side_branch_or_a_missing_commit()
 {
    let (scenario, commits) = history();
    let report = reported_at(
        &scenario,
        "compliance",
        "owner/app",
        &commits.run.to_string(),
    );
    assert_eq!(report["repository"]["commit"], commits.head.to_string());
    assert_eq!(report["repository"]["committed"], "1970-01-01T00:00:23Z");
    let [suite] = report["suites"].as_array().unwrap().as_slice() else {
        panic!("one suite: {report}")
    };
    assert_eq!(
        (&suite["id"], &suite["cases"], &suite["code"]),
        (&json!("/suites/app"), &json!(2), &Value::Null)
    );
    let run = &suite["run"];
    assert_eq!(run["id"], "/suites/app/runs/1");
    assert_eq!(run["finished"], "1970-01-01T00:00:00Z");
    assert_eq!(run["commit"], commits.run.to_string());
    assert_eq!(run["outcome"], "fail");
    assert_eq!(
        run["failed"],
        json!([{"case": "fmt", "note": "two files are not formatted"}])
    );
    assert_eq!(
        (&run["commits_since"], &run["commits_since_code"]),
        (&json!(3), &Value::Null)
    );
    assert!(
        !report.to_string().contains(RUNNER),
        "no run's `by`: {report}"
    );
    shipped(REPORT).validate(&report).unwrap();
    let report = reported_at(
        &scenario,
        "compliance",
        "owner/app",
        &commits.side.to_string(),
    );
    let run = &report["suites"][0]["run"];
    assert_eq!(
        (&run["commits_since"], &run["commits_since_code"]),
        (
            &Value::Null,
            &json!("rules::report::run_commit_not_ancestor")
        )
    );
    let report = reported_at(&scenario, "compliance", "owner/app", &"a".repeat(40));
    let run = &report["suites"][0]["run"];
    assert_eq!(
        (&run["commits_since"], &run["commits_since_code"]),
        (&Value::Null, &json!("rules::report::run_commit_missing"))
    );
}

#[test]
fn a_suite_that_is_no_compliance_check_or_names_another_repository_is_left_out() {
    let (scenario, commits) = history();
    let commit = commits.run.to_string();
    let named = reported_at(&scenario, "compliance", "Owner/App", &commit);
    assert_eq!(named["suites"].as_array().unwrap().len(), 1);
    let test = reported_at(&scenario, "test", "owner/app", &commit);
    assert_eq!(test["suites"], json!([]));
    let other = reported_at(&scenario, "compliance", "owner/other", &commit);
    assert_eq!(other["suites"], json!([]));
}

#[test]
fn run_commit_reads_versions_commit_alone() {
    assert_eq!(
        run_commit(&json!({"versions": {"a": "abcdef1", "commit": "1234567"}})),
        Some("1234567".to_owned())
    );
    assert_eq!(
        run_commit(&json!({"versions": {"board": "abcdef1", "date": "20261125"}})),
        None,
        "a hex-looking value under another name is no commit"
    );
    assert_eq!(run_commit(&json!({"versions": {"commit": "opus"}})), None);
    assert_eq!(run_commit(&json!({})), None);
}

#[test]
fn a_run_finishes_at_its_finish_else_its_start_and_at_no_time_when_neither_is_one() {
    let finished = suites::finished;
    assert_eq!(
        finished(&json!({"finished": "1970-01-01T01:00:00+01:00", "started": "x"})),
        Some("1970-01-01T00:00:00Z".to_owned())
    );
    assert_eq!(
        finished(&json!({"finished": "soon", "started": "1970-01-01T00:00:05Z"})),
        Some("1970-01-01T00:00:05Z".to_owned())
    );
    assert_eq!(finished(&json!({"finished": "soon", "started": "x"})), None);
}

#[test]
fn a_run_that_names_no_commit_says_so_apart_from_a_commit_the_clone_lacks() {
    let (scenario, commits) = history();
    let versions = json!({"crumbkit/app": commits.run.to_string()});
    let report = reported_with(&scenario, "compliance", "owner/app", &versions);
    let run = &report["suites"][0]["run"];
    assert_eq!(run["commit"], Value::Null, "{report}");
    assert_eq!(
        (&run["commits_since"], &run["commits_since_code"]),
        (&Value::Null, &json!("rules::report::run_commit_unnamed"))
    );
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn a_group_board_marks_invalid_is_a_refusal_and_no_group_row() {
    let plain = rule(&["docs/*"], &json!({}));
    let invalid = json!({"$type": "rules/group/v1", "purpose": "P.", "members": ["/rules/*"]});
    let (_scenario, host) = board(
        &[
            ("docs/a.md", "one"),
            ("rules/plain.json", &plain),
            ("rules/groups/untitled.json", &invalid.to_string()),
        ],
        &[],
    );
    let report = report_of(&host);
    assert_eq!(report["groups"], json!([]), "{report}");
    let refused: Vec<(&Value, &Value)> = (report["refused"].as_array().unwrap().iter())
        .map(|refusal| (&refusal["code"], &refusal["rules"]))
        .collect();
    assert_eq!(
        refused,
        [(
            &json!("rules::report::group_unreadable"),
            &json!(["/rules/groups/untitled"])
        )]
    );
    shipped(REPORT).validate(&report).unwrap();
}

/// A board laid over a history whose one compliance suite names
/// `owner/app`, with `files` beside it.
fn compliance_beside(files: &[(&str, &str)]) -> (Scenario, Recording) {
    let (scenario, commits) = history();
    let versions = json!({"commit": commits.run.to_string()});
    let [suite, run] = compliance("compliance", "owner/app", &versions);
    let mut all = vec![(suite.0, suite.1.as_str()), (run.0, run.1.as_str())];
    all.extend_from_slice(files);
    let host = with_git(enabling_in(&scenario, &["rules", "runs"], &all), &[]);
    (scenario, host)
}

/// Each suite row's id.
fn suite_ids(report: &Value) -> Vec<&Value> {
    (report["suites"].as_array().unwrap().iter())
        .map(|suite| &suite["id"])
        .collect()
}

/// Each refusal's code and what it concerns.
fn refusals(report: &Value) -> Vec<(&Value, &Value)> {
    (report["refused"].as_array().unwrap().iter())
        .map(|refusal| (&refusal["code"], &refusal["rules"]))
        .collect()
}

#[test]
fn a_suite_file_that_is_not_json_is_no_record_so_the_report_neither_lists_nor_refuses_it() {
    let (_scenario, host) =
        compliance_beside(&[("suites/broken.json", "{\"$type\": \"runs/suite/v1\", ")]);
    let report = report_of(&host);
    assert_eq!(suite_ids(&report), [&json!("/suites/app")], "{report}");
    assert_eq!(refusals(&report), [], "{report}");
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn a_zero_rule_keeps_a_measure_recorded_as_a_label_apart_from_one_not_recorded() {
    let zero = json!({
        "$type": "rules/rule/v1",
        "requirement": "R.",
        "purpose": "P.",
        "paths": ["src/*"],
        "checker": {"check": "zero", "suite": "/suites/users", "measures": ["remaining", "status", "absent"]},
    });
    let suite = json!({"$type": "runs/suite/v1", "method": "Count the users."});
    let run = json!({
        "$type": "runs/run/v1",
        "suite": "/suites/users",
        "by": {"program": "claude-code"},
        "started": "1970-01-01T00:00:00Z",
        "scope": "full",
        "dirty": false,
        "results": [],
        "measures": {"remaining": 0, "status": "unknown"},
    });
    let (_scenario, host) = board(
        &[
            ("src/old.rs", "fn old() {}"),
            ("rules/zero.json", &zero.to_string()),
            ("suites/users.json", &suite.to_string()),
            ("suites/users/runs/1.json", &run.to_string()),
        ],
        &[],
    );
    let report = report_of(&host);
    assert_eq!(
        row(&report, "/rules/zero")["zero"]["measures"],
        json!({"remaining": 0, "status": "unknown", "absent": null})
    );
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn a_path_below_the_root_drops_the_root_and_any_other_path_is_kept() {
    assert_eq!(below("/rules/rust/version", "/rules"), "rust/version");
    assert_eq!(below("/rulesx/a", "/rules"), "/rulesx/a");
    assert_eq!(below("/other/a", "/rules"), "/other/a");
}

#[test]
fn a_version_rule_lists_every_pin_it_found_passing_or_not_and_counts_those_past_20() {
    let within = rule(&["rust-toolchain.toml"], &pinned("1.80"));
    let behind = rule(&["rust-toolchain.toml"], &pinned("1.99"));
    let many = rule(&["pins.txt"], &pinned("1.0"));
    let written = rule(&["pins.txt"], &json!({}));
    let lines: Vec<String> = (0..23).map(|at| format!("channel = \"1.{at}\"")).collect();
    let lines = lines.join("\n");
    let (_scenario, host) = board(
        &[
            ("rust-toolchain.toml", "channel = \"1.90\"\n"),
            ("pins.txt", &lines),
            ("rules/within.json", &within),
            ("rules/behind.json", &behind),
            ("rules/many.json", &many),
            ("rules/written.json", &written),
        ],
        &[],
    );
    let report = report_of(&host);
    let pin = json!([{"file": "/rust-toolchain.toml", "line": 1, "found": "1.90"}]);
    let within = row(&report, "/rules/within");
    assert_eq!(within["result"], "within");
    assert_eq!(within["pins"], pin, "a passing pin is listed: {report}");
    assert_eq!(within["behind"], json!([]));
    assert_eq!(row(&report, "/rules/behind")["pins"], pin);
    let many = row(&report, "/rules/many");
    let pins = many["pins"].as_array().unwrap();
    assert_eq!(pins.len(), LISTED);
    assert_eq!(
        pins[19],
        json!({"file": "/pins.txt", "line": 20, "found": "1.19"})
    );
    assert_eq!(many["omitted"]["pins"], 3);
    assert_eq!(row(&report, "/rules/written")["pins"], json!([]));
    shipped(REPORT).validate(&report).unwrap();
}

/// A `boundary/boundary/v1` record checking `rules` on the way to `to`,
/// with `fields` added.
fn boundary(to: &str, rules: &[&str], fields: &Value) -> String {
    let checks: Vec<Value> = (rules.iter())
        .map(|rule| json!({"rule": rule, "parts": ["pr.*"]}))
        .collect();
    let mut record = json!({
        "$type": "boundary/boundary/v1",
        "to": to,
        "actions": ["github.*"],
        "checks": checks,
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record.to_string()
}

/// Each row's id and targets.
fn targets(report: &Value) -> Vec<(String, Value)> {
    (report["rules"].as_array().unwrap().iter())
        .map(|row| {
            (
                row["id"].as_str().unwrap().to_owned(),
                row["targets"].clone(),
            )
        })
        .collect()
}

#[test]
fn a_rule_a_boundary_to_the_repository_names_targets_a_body_and_a_rule_with_paths_a_file() {
    let file = rule(&["docs/*"], &json!({}));
    let both = rule(&["docs/*"], &words(9));
    let body = json!({"$type": "rules/rule/v1", "requirement": "R.", "purpose": "P."});
    let body = body.to_string();
    let (_scenario, host) = board(
        &[
            ("docs/a.md", "one two three"),
            ("rules/file.json", &file),
            ("rules/both.json", &both),
            ("rules/body.json", &body),
            ("rules/other-to.json", &body),
            ("rules/other-from.json", &body),
            ("rules/own-from.json", &body),
            (
                "boundaries/app.json",
                &boundary(
                    "github:Owner/*",
                    &["/rules/both", "/rules/body"],
                    &json!({}),
                ),
            ),
            (
                "boundaries/other-to.json",
                &boundary("github:owner/other", &["/rules/other-to"], &json!({})),
            ),
            (
                "boundaries/other-from.json",
                &boundary(
                    "github:owner/*",
                    &["/rules/other-from"],
                    &json!({"from": ["/repos/other"]}),
                ),
            ),
            (
                "boundaries/own-from.json",
                &boundary("slack:*", &["/rules/own-from"], &json!({"from": ["/"]})),
            ),
        ],
        &[],
    );
    let report = report_of(&host);
    assert_eq!(
        targets(&report),
        [
            ("/rules/body".to_owned(), json!(["body"])),
            ("/rules/both".to_owned(), json!(["file", "body"])),
            ("/rules/file".to_owned(), json!(["file"])),
            ("/rules/own-from".to_owned(), json!(["body"])),
        ],
        "a boundary whose from or to misses the repository adds no target: {report}"
    );
    assert_eq!(row(&report, "/rules/body")["result"], "written");
    assert_eq!(
        report["drift"]["not_reached"],
        json!(["/rules/other-from", "/rules/other-to"])
    );
    assert_eq!(refusals(&report), [], "{report}");
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn a_boundary_board_marks_degraded_gives_its_rules_no_body_and_is_a_refusal() {
    let body = json!({"$type": "rules/rule/v1", "requirement": "R.", "purpose": "P."});
    let body = body.to_string();
    let file = rule(&["docs/*"], &json!({}));
    let broken = boundary(
        "github:owner/*",
        &["/rules/body", "/rules/file"],
        &json!({"actions": []}),
    );
    let (_scenario, host) = board(
        &[
            ("docs/a.md", "one"),
            ("rules/body.json", &body),
            ("rules/file.json", &file),
            ("boundaries/broken.json", &broken),
        ],
        &[],
    );
    let report = report_of(&host);
    assert_eq!(
        targets(&report),
        [("/rules/file".to_owned(), json!(["file"]))],
        "{report}"
    );
    assert_eq!(report["drift"]["not_reached"], json!(["/rules/body"]));
    let [refusal] = report["refused"].as_array().unwrap().as_slice() else {
        panic!("one refusal: {report}")
    };
    assert_eq!(refusal["code"], "rules::report::boundary_unreadable");
    assert_eq!(refusal["rules"], json!(["/boundaries/broken"]));
    assert_eq!(refusal["severity"], "refusal");
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn a_rule_names_the_suite_that_judges_it_and_one_naming_no_valid_suite_keeps_its_result_with_one_warning()
 {
    let judged = |suite: &str| {
        let mut fields = words(9);
        fields["judged_by"] = json!(suite);
        rule(&["docs/*"], &fields)
    };
    let suite = json!({"$type": "runs/suite/v1", "method": "Review the repository."});
    let invalid = json!({"$type": "runs/suite/v1"});
    let (_scenario, host) = board(
        &[
            ("docs/a.md", "one"),
            ("suites/review.json", &suite.to_string()),
            ("suites/broken.json", &invalid.to_string()),
            ("rules/judged.json", &judged("/suites/review")),
            ("rules/unread.json", &judged("/suites/broken")),
            ("rules/missing.json", &judged("/suites/missing")),
            ("rules/plain.json", &rule(&["docs/*"], &json!({}))),
        ],
        &[],
    );
    let report = report_of(&host);
    assert_eq!(row(&report, "/rules/judged")["judged_by"], "/suites/review");
    assert_eq!(row(&report, "/rules/plain")["judged_by"], Value::Null);
    for (id, suite) in [
        ("/rules/judged", "/suites/review"),
        ("/rules/missing", "/suites/missing"),
        ("/rules/unread", "/suites/broken"),
    ] {
        let judged = row(&report, id);
        assert_eq!(judged["judged_by"], suite);
        assert_eq!(
            (&judged["result"], &judged["used"]),
            (&json!("within"), &json!(1)),
            "a judged_by the report cannot read leaves the measured result: {report}"
        );
    }
    assert_eq!(
        report["refused"],
        json!([
            {
                "code": "rules::report::judged_by_unreadable",
                "severity": "warning",
                "rules": ["/rules/missing"],
                "message": "the rule /rules/missing's judged_by names /suites/missing, which Board does not read as a valid suite",
            },
            {
                "code": "rules::report::judged_by_unreadable",
                "severity": "warning",
                "rules": ["/rules/unread"],
                "message": "the rule /rules/unread's judged_by names /suites/broken, which Board does not read as a valid suite",
            },
        ]),
        "{report}"
    );
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn an_obligation_names_the_record_that_left_it_owed_and_one_naming_no_record_keeps_its_result_with_one_warning()
 {
    let pull = json!({
        "$type": "github/pull/v1",
        "owner": "owner",
        "repo": "app",
        "number": 1460,
        "url": "https://github.com/owner/app/pull/1460",
        "title": "A change.",
    });
    let decision = json!({
        "$type": "plans/decision/v1",
        "decision": "Owe the work.",
        "why": "It waits.",
        "by": {"program": "claude-code"},
        "decided": "2026-11-27T00:00:00Z",
    });
    let review = json!({
        "$type": "review/report/v1",
        "findings": [],
        "verdict": "approve",
        "range": "22c3c4183e9a1fe22fddf20cb9c22d3eaca84851..d7ccf6c4e83d36c4ee9bb28baad70904c3175a78",
        "patch_ids": ["b70c6a1b00cd49d4a89c22c0d9d204a26d50abab"],
    });
    let owed = |by: &str| {
        let mut fields = words(9);
        fields["kind"] = json!("obligation");
        fields["owed_by"] = json!(by);
        rule(&["docs/*"], &fields)
    };
    let (_scenario, host) = board(
        &[
            ("docs/a.md", "one"),
            ("github/owner/app/pulls/1460.json", &pull.to_string()),
            ("decisions/owe.json", &decision.to_string()),
            ("reviews/one/report.json", &review.to_string()),
            ("rules/owed.json", &owed("/github/owner/app/pulls/1460")),
            ("rules/decided.json", &owed("/decisions/owe")),
            ("rules/reviewed.json", &owed("/reviews/one/report")),
            ("rules/gone.json", &owed("/github/owner/app/pulls/9")),
            ("rules/standing.json", &rule(&["docs/*"], &json!({}))),
        ],
        &[],
    );
    let report = report_of(&host);
    assert_eq!(
        row(&report, "/rules/owed")["owed_by"],
        json!({
            "ref": "/github/owner/app/pulls/1460",
            "type": "github/pull/v1",
            "number": 1460,
            "url": "https://github.com/owner/app/pull/1460",
        })
    );
    assert_eq!(
        row(&report, "/rules/decided")["owed_by"],
        json!({"ref": "/decisions/owe", "type": "plans/decision/v1", "number": null, "url": null}),
        "{report}"
    );
    assert_eq!(
        row(&report, "/rules/reviewed")["owed_by"],
        json!({"ref": "/reviews/one/report", "type": "review/report/v1", "number": null, "url": null}),
        "{report}"
    );
    assert_eq!(row(&report, "/rules/standing")["owed_by"], Value::Null);
    let gone = row(&report, "/rules/gone");
    assert_eq!(
        gone["owed_by"],
        json!({"ref": "/github/owner/app/pulls/9", "type": null, "number": null, "url": null}),
        "{report}"
    );
    assert_eq!(
        (&gone["result"], &gone["used"]),
        (&json!("within"), &json!(1)),
        "an owed_by the report cannot read leaves the measured result: {report}"
    );
    let refused: Vec<(&Value, &Value, &Value)> = (report["refused"].as_array().unwrap().iter())
        .map(|refusal| (&refusal["code"], &refusal["rules"], &refusal["severity"]))
        .collect();
    assert_eq!(
        refused,
        [(
            &json!("rules::report::owed_by_unreadable"),
            &json!(["/rules/gone"]),
            &json!("warning")
        )],
        "{report}"
    );
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn each_refused_entry_says_whether_it_is_a_warning_or_a_refusal() {
    let dangling = rule(&["docs/*"], &json!({"applies_to": ["/nothing/here"]}));
    let capture = rule(
        &["docs/*"],
        &json!({"checker": {"check": "version", "at_least": "1", "capture": "no group"}}),
    );
    let mut judged = words(9);
    judged["judged_by"] = json!("/suites/missing");
    let mut owed = words(9);
    owed["kind"] = json!("obligation");
    owed["owed_by"] = json!("/github/owner/app/pulls/9");
    let zero = json!({"checker": {"check": "zero", "suite": "/suites/gone", "measures": ["left"]}});
    let invalid_group =
        json!({"$type": "rules/group/v1", "purpose": "P.", "members": ["/rules/*"]});
    let invalid_boundary = boundary("github:owner/*", &["/rules/plain"], &json!({"actions": []}));
    let (_scenario, host) = board(
        &[
            ("docs/a.md", "one"),
            ("docs/bad.json", r#"{"$type": "rules/group/v1"}"#),
            ("rules/broken.json", &dangling),
            ("rules/capture.json", &capture),
            ("rules/budget.json", &rule(&["docs/*"], &words(9))),
            ("rules/judged.json", &rule(&["docs/*"], &judged)),
            ("rules/owed.json", &rule(&["docs/*"], &owed)),
            ("rules/zero.json", &rule(&["docs/*"], &zero)),
            ("rules/plain.json", &rule(&["docs/*"], &json!({}))),
            (
                "rules/groups/none.json",
                &group("None", &["/rules/python/*"]),
            ),
            ("rules/groups/untitled.json", &invalid_group.to_string()),
            ("boundaries/broken.json", &invalid_boundary),
        ],
        &[("GITHUB_RUN_ID", "1")],
    );
    let report = report_of(&host);
    let mut severities: Vec<(String, String)> = (report["refused"].as_array().unwrap().iter())
        .map(|refusal| {
            (
                refusal["code"].as_str().unwrap().to_owned(),
                refusal["severity"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    severities.sort();
    severities.dedup();
    let pairs = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        (pairs.iter())
            .map(|(code, severity)| ((*code).to_owned(), (*severity).to_owned()))
            .collect()
    };
    assert_eq!(
        severities,
        pairs(&[
            ("board::record::invalid", "refusal"),
            ("board::ref::dangling", "refusal"),
            ("rules::capture::invalid", "refusal"),
            ("rules::report::boundary_unreadable", "refusal"),
            ("rules::report::ci_env_invalid", "warning"),
            ("rules::report::group_unreadable", "refusal"),
            ("rules::report::judged_by_unreadable", "warning"),
            ("rules::report::member_unmatched", "warning"),
            ("rules::report::owed_by_unreadable", "warning"),
            ("runs::suite::not_found", "refusal"),
        ]),
        "a degraded file, a zero rule's code and each of the report's own codes: {report}"
    );
    shipped(REPORT).validate(&report).unwrap();
}

#[test]
fn the_v2_schema_says_refused_rules_can_name_a_suite_and_run_commit_unnamed_covers_an_unreadable_run()
 {
    let (_, schema) = (Rules.schemas().into_iter())
        .find(|(record_type, _)| record_type.as_str() == REPORT)
        .unwrap();
    let described = |pointer: &str| {
        (schema.pointer(pointer))
            .and_then(|field| field["description"].as_str())
            .unwrap_or_else(|| panic!("no description at {pointer}"))
            .to_owned()
    };
    let rules = described("/$defs/Refused/properties/rules");
    assert!(rules.contains("suite"), "{rules}");
    let code = described("/$defs/SuiteRun/properties/commits_since_code");
    assert!(code.contains("cannot read the run"), "{code}");
    assert!(code.contains("7 to 40"), "{code}");
    let v1 = (Rules.schemas().into_iter())
        .find(|(record_type, _)| record_type.as_str() == "rules/report/v1")
        .map(|(_, schema)| canonical_json(&schema).as_str().to_owned());
    assert_eq!(
        v1.as_deref(),
        Some(include_str!("../../types/rules/report/v1.schema.json")),
        "Board keeps shipping rules/report/v1 as it was released"
    );
}
