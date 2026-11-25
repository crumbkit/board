//! What `report` prints for a board of rules, groups and suites, read
//! through the recording host `check`'s tests use, with git on its `PATH`.

use std::ffi::OsString;

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

/// A board that enables rules and runs, holding `files`, with git on the
/// host's `PATH` and `variables` set.
fn board(files: &[(&str, &str)], variables: &[(&str, &str)]) -> (Scenario, Recording) {
    let (scenario, host) = enabling(&["rules", "runs"], files);
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
            pair("/rules/elsewhere", "written"),
            pair("/rules/version/behind", "behind"),
            pair("/rules/version/within", "within"),
            pair("/rules/written", "written"),
        ],
        "a rule outside the imposed root is no row: {report}"
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
    assert_eq!(report["$type"], "rules/report/v1");
    shipped("rules/report/v1").validate(&report).unwrap();
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
            (&json!("/rules/groups/none"), &json!("not_reached")),
            (&json!("/rules/groups/pii"), &json!("passing")),
            (&json!("/rules/groups/rust"), &json!("failing")),
            (&json!("/rules/groups/server"), &json!("failing")),
        ],
        "{report}"
    );
    let rust = &report["groups"][4]["counts"];
    assert_eq!((&rust["over"], &rust["within"]), (&json!(1), &json!(1)));
    assert_eq!(report["groups"][5]["counts"]["over"], 1);
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
    let applying = crate::for_path::for_path(&host, &json!({"path": "/rules/groups/rust"}));
    assert_eq!(applying.unwrap()["rules"], json!([]));
    shipped("rules/report/v1").validate(&report).unwrap();
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
    shipped("rules/report/v1").validate(&report).unwrap();
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
    assert_eq!(each["omitted"], json!({"files": 3, "over": 3, "behind": 0}));
    assert_eq!(each["result"], "over");
}

/// Who ran the suite, which no report shows.
const RUNNER: &str = "zelda-qx";

/// A compliance suite naming `subject` of `kind`, and its one run, whose
/// `versions` name `commit` and whose case `fmt` failed with a note.
fn compliance(kind: &str, subject: &str, commit: &str) -> [(&'static str, String); 2] {
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
        "versions": {"commit": commit},
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
    let [suite, run] = compliance(kind, subject, commit);
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
    shipped("rules/report/v1").validate(&report).unwrap();
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
fn run_commit_reads_versions_commit_first_then_the_first_hex_value() {
    assert_eq!(
        run_commit(&json!({"versions": {"a": "abcdef1", "commit": "1234567"}})),
        Some("1234567".to_owned())
    );
    assert_eq!(
        run_commit(&json!({"versions": {"model": "opus", "repo": "abcdef1"}})),
        Some("abcdef1".to_owned())
    );
    assert_eq!(run_commit(&json!({"versions": {"model": "opus"}})), None);
    assert_eq!(run_commit(&json!({})), None);
}

#[test]
fn a_path_below_the_root_drops_the_root_and_any_other_path_is_kept() {
    assert_eq!(below("/rules/rust/version", "/rules"), "rust/version");
    assert_eq!(below("/rulesx/a", "/rules"), "/rulesx/a");
    assert_eq!(below("/other/a", "/rules"), "/other/a");
}
