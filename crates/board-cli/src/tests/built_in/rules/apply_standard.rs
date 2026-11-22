//! The apply-standard skill that the runs plugin ships: the files it embeds,
//! its sample standard read by `rules.from-standards`, and its example suite
//! and run admitted by the runs plugin's schemas.

use super::*;
use crate::skills::SKILLS;

/// The text of `file` in the embedded apply-standard skill.
fn shipped_file(file: &str) -> String {
    let skill = (SKILLS.iter())
        .find(|skill| skill.name == "apply-standard")
        .expect("apply-standard is embedded");
    let (_, bytes) = (skill.files.iter())
        .find(|(path, _)| *path == file)
        .unwrap_or_else(|| panic!("apply-standard embeds no {file}"));
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// `sample` with the Where it applies cell of the row for `id` written as
/// plain words, so it holds no inline code.
fn without_pattern(sample: &str, id: &str) -> String {
    let row = format!("| {id} |");
    let lines = sample.lines().map(|line| {
        if !line.starts_with(&row) {
            return line.to_owned();
        }
        let mut cells: Vec<&str> = line.split('|').collect();
        cells[3] = " every file ";
        cells.join("|")
    });
    lines.collect::<Vec<_>>().join("\n")
}

/// A board that mounts a standards repository whose one profile is
/// `profile`, and an adopter at /repos/app whose map adopts it. The
/// repositories live as long as the values returned.
fn adopting_sample(profile: &str) -> (Fixture, Scenario, Scenario) {
    let board = board();
    let standards = library(
        &board,
        "repos/standards.json",
        &[("profiles/sample.md", profile)],
    );
    let map = "schema = \"rust-standards/adoption/v1\"\nprofiles = [\"sample\"]\n";
    let app = library(
        &board,
        "repos/app.json",
        &[("rust-standards.toml", map), ("README.md", "# app\n")],
    );
    (board, standards.0, app.0)
}

/// The document `rules.from-standards` prints for the adopter.
fn read_standards(board: &Fixture) -> (Output, Value) {
    let ran = board.run(&[
        "--json",
        "call",
        "rules.from-standards",
        "--standards",
        "/repos/standards",
        "--map",
        "/repos/app/rust-standards.toml",
        "--into",
        "/controls/app/standards",
    ]);
    let document = json(&ran);
    (ran, document)
}

#[test]
fn the_skill_ships_its_procedure_and_its_sample_standard_in_the_runs_plugin() {
    let skill = (SKILLS.iter())
        .find(|skill| skill.name == "apply-standard")
        .expect("apply-standard is embedded");
    assert_eq!(skill.plugin, "runs");
    let files: Vec<&str> = skill.files.iter().map(|(path, _)| *path).collect();
    assert_eq!(files, ["SKILL.md", "sample-standard.md"]);
}

#[test]
fn the_sample_standard_gives_six_rules_with_paths_and_a_row_without_a_pattern_is_refused() {
    let sample = shipped_file("sample-standard.md");
    let (board, _standards, _app) = adopting_sample(&without_pattern(&sample, "SAMPLE-3"));
    let (ran, document) = read_standards(&board);
    assert_ne!(ran.code, 0);
    assert_eq!(
        document["code"],
        json!("rules::standards::no_pattern"),
        "{document}"
    );
    assert_eq!(document["fields"]["rule"], json!("SAMPLE-3"));
    let (board, _standards, _app) = adopting_sample(&sample);
    let (ran, document) = read_standards(&board);
    assert_eq!(ran.code, 0, "{document}");
    let output = &document["data"]["output"];
    assert_eq!(output["profiles"][0]["rules"], json!(6));
    let lines = output["lines"].as_array().unwrap();
    let paths: Vec<&str> = lines.iter().map(|l| l["path"].as_str().unwrap()).collect();
    let expected: Vec<String> = (1..=6)
        .map(|n| format!("/controls/app/standards/sample-{n}"))
        .collect();
    assert_eq!(paths, expected);
    for line in lines {
        let patterns = line["record"]["paths"].as_array().unwrap();
        assert!(!patterns.is_empty(), "{line}");
    }
}

/// Each fenced JSON block of `text`, parsed after `fill`.
fn json_blocks(text: &str, fill: &[(&str, &str)]) -> Vec<Value> {
    let mut blocks = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() != "```json" {
            continue;
        }
        let mut block = (lines.by_ref())
            .take_while(|line| line.trim() != "```")
            .collect::<Vec<_>>()
            .join("\n");
        for (placeholder, value) in fill {
            block = block.replace(placeholder, value);
        }
        assert!(!block.contains('<'), "a placeholder is left: {block}");
        blocks.push(serde_json::from_str(&block).unwrap());
    }
    blocks
}

/// The first of `blocks` whose `$type` is `kind`.
fn of_type<'a>(blocks: &'a [Value], kind: &str) -> &'a Value {
    (blocks.iter())
        .find(|block| block["$type"] == kind)
        .unwrap_or_else(|| panic!("no {kind} example"))
}

/// The note the skill gives a rule whose patterns match no file.
const ABSENT: &str = "passes by absence: no file matches";

/// The suite and the run SKILL.md shows, with their placeholders filled.
fn examples() -> (Value, Value) {
    let fill = [
        ("<owner>/<name>", "example/app"),
        ("<standard file>", "standards/sample-standard.md"),
        ("<repository>-<standard>", "app-sample-standard"),
        ("<session>", "local_0b5c1e2a-3f4d-4e6a-9b7c-1d2e3f4a5b6c"),
        ("<started>", "2026-11-21T09:00:00Z"),
        ("<finished>", "2026-11-21T09:04:00Z"),
        ("<commit>", "38a3b7e0c2d14f5a6b7c8d9e0f1a2b3c4d5e6f70"),
        ("<hex>", &"ab".repeat(32)),
    ];
    let blocks = json_blocks(&shipped_file("SKILL.md"), &fill);
    let suite = of_type(&blocks, "runs/suite/v1").clone();
    (suite, of_type(&blocks, "runs/run/v1").clone())
}

const SUITE: &str = "/suites/app-sample-standard";
const NEW_RUN: [&str; 4] = ["put", "--new", "--var", "suite=app-sample-standard"];

/// Writes `record` with `arguments` and asserts that `board put` admits it.
fn admitted(board: &Fixture, arguments: &[&str], record: &Value) {
    let put = board.run_with(arguments, &record.to_string());
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
}

/// The cases the run's results give `outcome`.
fn cases(run: &Value, outcome: &str) -> Vec<Value> {
    (run["results"].as_array().unwrap().iter())
        .filter(|result| result["outcome"] == outcome)
        .map(|result| result["case"].clone())
        .collect()
}

#[test]
fn the_skills_example_suite_and_run_are_admitted_and_latest_names_the_failed_rule() {
    let (suite, run) = examples();
    let board = Fixture::enabling(&["runs"]);
    admitted(&board, &["put", SUITE], &suite);
    let failed = cases(&run, "fail");
    assert_eq!(failed.len(), 1, "{run}");
    let mut noteless = run.clone();
    for result in noteless["results"].as_array_mut().unwrap() {
        if result["outcome"] == "fail" {
            result.as_object_mut().unwrap().remove("note");
        }
    }
    let refused = board.run_with(&NEW_RUN, &noteless.to_string());
    assert_eq!(
        refused.error.as_deref(),
        INVALID,
        "{}",
        text(&refused.stderr)
    );
    admitted(&board, &NEW_RUN, &run);
    let latest = board.call("runs.latest", &["--suite", SUITE]);
    assert_eq!(latest["run"]["outcome"], json!("fail"), "{latest}");
    assert_eq!(latest["run"]["failed"], json!(failed), "{latest}");
}

#[test]
fn a_rule_that_passes_by_absence_is_a_passed_result_that_latest_does_not_count_as_failed() {
    let (suite, run) = examples();
    let absent: Vec<&Value> = (run["results"].as_array().unwrap().iter())
        .filter(|result| result["note"] == ABSENT)
        .collect();
    assert_eq!(absent.len(), 1, "{run}");
    assert_eq!(absent[0]["outcome"], json!("pass"));
    let board = Fixture::enabling(&["runs"]);
    admitted(&board, &["put", SUITE], &suite);
    admitted(&board, &NEW_RUN, &run);
    let latest = board.call("runs.latest", &["--suite", SUITE]);
    assert_eq!(
        latest["run"]["failed"],
        json!(cases(&run, "fail")),
        "{latest}"
    );
    let mut failing = run.clone();
    failing["started"] = json!("2026-11-22T09:00:00Z");
    failing["finished"] = json!("2026-11-22T09:04:00Z");
    for result in failing["results"].as_array_mut().unwrap() {
        if result["note"] == ABSENT {
            result["outcome"] = json!("fail");
        }
    }
    admitted(&board, &NEW_RUN, &failing);
    let latest = board.call("runs.latest", &["--suite", SUITE]);
    let mut failed = cases(&run, "fail");
    failed.push(absent[0]["case"].clone());
    let mut listed = latest["run"]["failed"].as_array().unwrap().clone();
    let key = |case: &Value| case.as_str().unwrap().to_owned();
    listed.sort_by_key(key);
    failed.sort_by_key(key);
    assert_eq!(listed, failed, "{latest}");
}

#[test]
fn rewriting_the_suite_without_a_rule_keeps_the_earlier_runs_result_for_it() {
    let (suite, run) = examples();
    let board = Fixture::enabling(&["runs"]);
    admitted(&board, &["put", SUITE], &suite);
    admitted(&board, &NEW_RUN, &run);
    let removed = cases(&run, "fail");
    let mut fewer = suite.clone();
    (fewer["cases"].as_array_mut().unwrap()).retain(|case| !removed.contains(&case["id"]));
    assert_eq!(
        fewer["cases"].as_array().unwrap().len() + removed.len(),
        suite["cases"].as_array().unwrap().len()
    );
    admitted(&board, &["put", SUITE], &fewer);
    let checked = board.checked("/suites/app-sample-standard");
    assert!(checked.values().all(Option::is_none), "{checked:?}");
    assert_eq!(checked.len(), 2, "{checked:?}");
    let latest = board.call("runs.latest", &["--suite", SUITE]);
    assert_eq!(latest["run"]["failed"], json!(removed), "{latest}");
}
