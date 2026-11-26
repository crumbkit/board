//! `board rules report` through the binary: a controls board with imposed
//! rules, groups and a compliance suite, mounting a repository that holds
//! rules of its own, prints one `rules/report/v1` document that the schema
//! `board schemas show` prints admits.

use board_scenario::seed::mount_of;

use super::*;

/// A rule of `requirement` on the mounted repository's `paths`, with
/// `fields` added.
fn imposed(requirement: &str, paths: &[&str], fields: &Value) -> String {
    record(
        json!({
            "$type": "rules/rule/v1",
            "requirement": requirement,
            "purpose": "A test.",
            "paths": paths,
        }),
        fields,
    )
}

/// A `rules/group/v1` record of `members`.
fn group(title: &str, members: &[&str]) -> String {
    json!({"$type": "rules/group/v1", "title": title, "purpose": "A test.", "members": members})
        .to_string()
}

/// A board that enables rules and runs, imposing a budget on the mounted
/// repository's Rust files, which it breaches, a rule it also holds with
/// another requirement, and a Go rule that reaches nothing; a group of the
/// Rust rules; and the repository mounted at /repos/app, holding its own
/// copy of the shared rule and one rule the imposed set lacks.
fn controls() -> (Fixture, Scenario, ObjectId) {
    let board = Fixture::enabling(&["rules", "runs"]);
    let words = json!({"checker": {"check": "budget", "unit": "words", "max": 2}});
    board
        .write(
            "rules/rust/short.json",
            &imposed("Keep Rust files short.", &["repos/app/src/*"], &words),
        )
        .write(
            "rules/rust/shared.json",
            &imposed("Name things well.", &["repos/app/src/*"], &json!({})),
        )
        .write(
            "rules/go/vet.json",
            &imposed("Vet Go.", &["repos/app/*.go"], &json!({})),
        )
        .write("rules/groups/rust.json", &group("Rust", &["/rules/rust/*"]));
    let own = |requirement: &str| imposed(requirement, &["src/*"], &json!({}));
    let (repository, commit) = repository(
        &["rules"],
        &[
            ("src/main.rs", "fn main() { run(); }".to_owned()),
            ("rules/rust/shared.json", own("Name things clearly.")),
            (
                "rules/extra.json",
                own("Something only this repository wants."),
            ),
        ],
    );
    mount(&board, "repos/app.json", &repository, commit);
    (board, repository, commit)
}

/// The report of /repos/app named `owner/app`, through `board call`.
fn reported(board: &Fixture) -> Value {
    board.call(
        "rules.report",
        &["--path", "/repos/app", "--repository", "owner/app"],
    )
}

/// The schema of `name` that `board plugin enable rules` wrote into the
/// board's `.board/types/`, compiled.
fn shown(board: &Fixture, name: &str) -> board_validate::Schema {
    let text = board.read(&format!(".board/types/{name}.schema.json"));
    board_validate::Schema::compile(serde_json::from_str(&text).unwrap()).unwrap()
}

#[test]
fn the_report_lists_imposed_and_own_rules_rolls_up_the_group_and_names_the_drift() {
    let (board, _repository, commit) = controls();
    let report = reported(&board);
    let rows: Vec<(&Value, &Value, &Value)> = (report["rules"].as_array().unwrap().iter())
        .map(|row| (&row["id"], &row["source"], &row["result"]))
        .collect();
    assert_eq!(
        rows,
        [
            (
                &json!("/rules/rust/shared"),
                &json!("imposed"),
                &json!("written")
            ),
            (
                &json!("/rules/rust/short"),
                &json!("imposed"),
                &json!("over")
            ),
            (
                &json!("/rules/extra"),
                &json!("repository"),
                &json!("unmeasured")
            ),
            (
                &json!("/rules/rust/shared"),
                &json!("repository"),
                &json!("unmeasured")
            ),
        ],
        "{report}"
    );
    let short = &report["rules"][1];
    assert_eq!(
        short["files"],
        json!(["/src/main.rs"]),
        "paths from the repository's root"
    );
    assert_eq!((&short["used"], &short["max"]), (&json!(5), &json!(2)));
    assert_eq!(short["groups"], json!(["/rules/groups/rust"]));
    assert_eq!(
        report["groups"],
        json!([{
            "id": "/rules/groups/rust",
            "title": "Rust",
            "state": "failing",
            "counts": {
                "within": 0, "over": 1, "behind": 0, "zero": 0, "not_zero": 0, "no_run": 0,
                "stale": 0, "missing_measure": 0, "written": 1, "unmeasured": 1, "unreadable": 0,
            },
        }])
    );
    assert_eq!(
        report["drift"],
        json!({
            "extra": ["/rules/extra"],
            "differs": [{"id": "/rules/rust/shared", "fields": ["requirement"]}],
            "not_reached": ["/rules/go/vet"],
        })
    );
    assert_eq!(
        report["repository"]["name"], "owner/app",
        "{}",
        report["repository"]
    );
    assert_eq!(report["repository"]["commit"], commit.to_string());
    assert_eq!(report["repository"]["uncommitted"], false);
    assert_eq!(report["source_run"], Value::Null);
    let version = report["board"]["version"].as_str().unwrap();
    assert!(
        (version.split('.')).all(|part| part.parse::<u64>().is_ok()),
        "{version}"
    );
    shown(&board, "rules/report/v1").validate(&report).unwrap();
}

#[test]
fn a_clone_with_no_origin_is_refused_with_no_repository_and_its_twin_with_an_origin_is_named() {
    let (board, repository, _commit) = controls();
    let refusal = board.refusal("rules.report", &["--path", "/repos/app"]);
    assert!(
        refusal.to_string().contains("rules::report::no_repository"),
        "{refusal}"
    );
    let config = std::fs::read_to_string(repository.workdir().join(".git/config")).unwrap();
    let origin = "[remote \"origin\"]\n\turl = git@github.com:Owner/App.git\n";
    repository.write(".git/config", Item::File(&format!("{config}{origin}")));
    let report = board.call("rules.report", &["--path", "/repos/app"]);
    assert_eq!(report["repository"]["name"], "owner/app");
}

#[test]
fn board_rules_report_prints_the_document_and_exits_0_with_a_failing_rule() {
    let (board, _repository, _commit) = controls();
    let output = board.run(&[
        "rules",
        "report",
        "--path",
        "/repos/app",
        "--repository",
        "owner/app",
    ]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["$type"], "rules/report/v1");
    assert_eq!(report["rules"][1]["result"], "over");
}

#[test]
fn a_report_in_github_actions_names_its_run_and_one_with_a_malformed_variable_says_so() {
    let (board, _repository, _commit) = controls();
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let mut variables = [
        ("GITHUB_REPOSITORY", "owner/app"),
        ("GITHUB_RUN_ID", "12345"),
        ("GITHUB_RUN_NUMBER", "7"),
        ("GITHUB_SHA", sha),
    ];
    let arguments = [
        "--json",
        "call",
        "rules.report",
        "--path",
        "/repos/app",
        "--repository",
        "owner/app",
    ];
    let output = board.run_inheriting(&arguments, "", &variables);
    let report = json(&output)["data"]["output"].clone();
    assert_eq!(
        report["source_run"],
        json!({"repository": "owner/app", "run_id": 12345, "run_number": 7, "commit": sha})
    );
    variables[2] = ("GITHUB_RUN_NUMBER", "seven");
    let output = board.run_inheriting(&arguments, "", &variables);
    let report = json(&output)["data"]["output"].clone();
    assert_eq!(report["source_run"], Value::Null);
    assert_eq!(
        report["refused"][0]["code"],
        "rules::report::ci_env_invalid"
    );
}

#[test]
fn help_for_the_report_shows_its_example_commands() {
    let board = board();
    let help = board.run(&["rules", "report", "-h"]);
    assert_eq!(help.code, 0, "{}", text(&help.stderr));
    let help = text(&help.stdout);
    for example in [
        "board --checkout rules report --path /repos/app > rules-report.json",
        "board rules report --path /repos/app --repository owner/app",
        "board rules report --path /repos/app --imposed /rules/imposed",
        "board rules report --path /",
        "board schemas show rules/report/v1",
    ] {
        assert!(help.contains(example), "{example}: {help}");
    }
}

/// The newest run of a compliance suite on a repository of four commits,
/// whose one run is on the first, in a board mounting a clone of the
/// repository at its last commit: a whole clone, or with `depth` a shallow
/// one of that depth, as `actions/checkout` makes.
#[expect(
    clippy::disallowed_methods,
    reason = "the test clones the repository with git, shallow or whole, as a CI checkout does"
)]
fn counted(depth: Option<&str>) -> Value {
    let repository = Scenario::new();
    let first = repository.commit(&[("a", "0")]);
    for at in 1..=3 {
        repository.commit(&[("a", &at.to_string())]);
    }
    let head = repository.head().unwrap().id;
    let clones = Scenario::new();
    let clone = clones.workdir().join("app");
    let source = format!("file://{}", repository.path());
    let mut arguments = vec!["clone", "-q"];
    if let Some(depth) = depth {
        arguments.extend(["--depth", depth]);
    }
    arguments.extend([source.as_str(), clone.to_str().unwrap()]);
    let made = std::process::Command::new("git")
        .args(&arguments)
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    let board = Fixture::enabling(&["rules", "runs"]);
    board
        .write("repos/app.json", &mount_of(clone.to_str().unwrap(), head))
        .write(
            "suites/app.json",
            &json!({
                "$type": "runs/suite/v1",
                "subjects": ["owner/app"],
                "kind": "compliance",
                "method": "Check the repository.",
            })
            .to_string(),
        )
        .write(
            "suites/app/runs/1.json",
            &json!({
                "$type": "runs/run/v1",
                "suite": "/suites/app",
                "started": "2026-11-25T10:00:00Z",
                "scope": "full",
                "dirty": false,
                "versions": {"commit": first.to_string()},
                "results": [],
            })
            .to_string(),
        );
    let report = reported(&board);
    drop(clones);
    report["suites"][0]["run"].clone()
}

#[test]
fn a_whole_clone_counts_the_commits_since_the_run_and_a_depth_1_clone_says_the_run_commit_is_missing()
 {
    let whole = counted(None);
    assert_eq!(
        (&whole["commits_since"], &whole["commits_since_code"]),
        (&json!(3), &Value::Null),
        "{whole}"
    );
    let shallow = counted(Some("1"));
    assert_eq!(
        (&shallow["commits_since"], &shallow["commits_since_code"]),
        (&Value::Null, &json!("rules::report::run_commit_missing")),
        "{shallow}"
    );
}
