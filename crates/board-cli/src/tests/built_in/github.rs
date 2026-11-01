//! The github plugin through the binary: `github.ref` asks a stub gh once
//! and writes a record once, and its refusals come in pairs. These were the
//! plugin's pytest suite.
//!
//! The plugin's processes see a `PATH` of a directory of this fixture's
//! own, which holds the stub gh, then `/usr/bin` and `/bin`, so no test
//! reaches GitHub. The stub records its arguments in `gh-argv.txt` and each
//! run in `gh-calls.txt`, prints `gh-out.txt` to stdout and `gh-err.txt` to
//! stderr, and exits with the code in `gh-exit.txt`, 0 when that file is
//! absent.

use super::*;

const GH: &str = r#"#!/bin/sh
here="$(dirname "$0")"
printf '%s\n' "$@" > "$here/gh-argv.txt"
echo call >> "$here/gh-calls.txt"
cat "$here/gh-out.txt" 2>/dev/null
cat "$here/gh-err.txt" >&2 2>/dev/null
exit "$(cat "$here/gh-exit.txt" 2>/dev/null || echo 0)"
"#;

/// A board that enables github, with the stub gh first on its plugins'
/// `PATH`.
struct Board {
    board: Fixture,
    bin: Scenario,
}

fn board() -> Board {
    let mut board = Fixture::enabling(&["github"]);
    let bin = Scenario::new();
    bin.write("gh", Item::Executable(GH));
    board.path = format!("{}:{PATH}", bin.workdir().display());
    Board { board, bin }
}

/// What `gh api repos/octo/widgets/issues/<number>` prints, cut to the
/// fields the plugin reads and one more.
fn issue(number: u64, title: &str, pull: bool) -> Value {
    let kind = if pull { "pull" } else { "issues" };
    let mut answer = json!({
        "number": number,
        "title": title,
        "html_url": format!("https://github.com/octo/widgets/{kind}/{number}"),
        "state": "open",
    });
    if pull {
        answer["pull_request"] =
            json!({"url": format!("https://api.github.com/repos/octo/widgets/pulls/{number}")});
    }
    answer
}

impl Board {
    /// Makes the stub gh print `answer`, as JSON unless it is text, and
    /// `stderr`, and exit with `code`.
    fn gh_answers(&self, answer: &Value, code: u8, stderr: &str) {
        let out = answer
            .as_str()
            .map_or_else(|| answer.to_string(), str::to_owned);
        self.bin.write("gh-out.txt", Item::File(&out));
        self.bin.write("gh-err.txt", Item::File(stderr));
        self.bin.write("gh-exit.txt", Item::File(&code.to_string()));
    }

    /// How many times the stub gh ran.
    fn gh_calls(&self) -> usize {
        std::fs::read_to_string(self.bin.workdir().join("gh-calls.txt"))
            .map_or(0, |calls| calls.lines().count())
    }

    /// The arguments of the stub gh's last run.
    fn gh_argv(&self) -> Vec<String> {
        let argv = std::fs::read_to_string(self.bin.workdir().join("gh-argv.txt")).unwrap();
        argv.lines().map(str::to_owned).collect()
    }

    /// The output of `board call github.ref --ref <reference>`.
    fn call(&self, reference: &str) -> Value {
        self.board.call("github.ref", &["--ref", reference])
    }

    /// The failure `board call github.ref --ref <reference>` prints.
    fn refusal(&self, reference: &str) -> Value {
        self.board.refusal("github.ref", &["--ref", reference])
    }

    /// The record at `path`, from the board's root.
    fn read(&self, path: &str) -> Value {
        serde_json::from_str(&self.board.read(path)).unwrap()
    }

    /// Whether anything is at `path` in the board's working tree.
    fn exists(&self, path: &str) -> bool {
        self.board.board.workdir().join(path).exists()
    }
}

/// The issue record `octo/widgets#7` with `fields`.
fn seven(fields: &Value) -> String {
    record(
        json!({"$type": "github/issue/v1", "owner": "octo", "repo": "widgets", "number": 7}),
        fields,
    )
}

#[test]
fn a_pull_request_is_written_at_its_pulls_path_with_its_title() {
    let board = board();
    board.gh_answers(&issue(12, "Add a widget", true), 0, "");
    assert_eq!(
        board.call("octo/widgets#12"),
        json!({
            "path": "/gh/octo/widgets/pulls/12",
            "type": "github/pull/v1",
            "written": true,
            "url": "https://github.com/octo/widgets/pull/12",
            "title": "Add a widget",
        })
    );
    assert_eq!(
        board.read("gh/octo/widgets/pulls/12.json"),
        json!({
            "$type": "github/pull/v1",
            "owner": "octo",
            "repo": "widgets",
            "number": 12,
            "url": "https://github.com/octo/widgets/pull/12",
            "title": "Add a widget",
        })
    );
    assert_eq!(board.gh_argv(), ["api", "repos/octo/widgets/issues/12"]);
}

#[test]
fn an_issue_is_written_at_its_issues_path() {
    let board = board();
    board.gh_answers(&issue(7, "Widgets wobble", false), 0, "");
    let output = board.call("octo/widgets#7");
    assert_eq!(
        (&output["path"], &output["type"], &output["written"]),
        (
            &json!("/gh/octo/widgets/issues/7"),
            &json!("github/issue/v1"),
            &json!(true)
        )
    );
    assert_eq!(
        board.read("gh/octo/widgets/issues/7.json"),
        json!({
            "$type": "github/issue/v1",
            "owner": "octo",
            "repo": "widgets",
            "number": 7,
            "url": "https://github.com/octo/widgets/issues/7",
            "title": "Widgets wobble",
        })
    );
}

#[test]
fn a_second_ref_writes_nothing_asks_gh_nothing_and_reports_the_record_already_there() {
    let board = board();
    board.gh_answers(&issue(12, "Add a widget", true), 0, "");
    let mut first = board.call("octo/widgets#12");
    let written = board.board.read("gh/octo/widgets/pulls/12.json");
    board.gh_answers(&issue(12, "Renamed since", true), 0, "");
    let again = board.call("octo/widgets#12");
    first["written"] = json!(false);
    assert_eq!(again, first);
    assert_eq!(board.board.read("gh/octo/widgets/pulls/12.json"), written);
    assert_eq!(board.gh_calls(), 1);
}

#[test]
fn an_issue_already_recorded_is_reported_without_asking_gh() {
    let board = board();
    board.board.write(
        "gh/octo/widgets/issues/7.json",
        &seven(&json!({"url": "https://github.com/octo/widgets/issues/7"})),
    );
    assert_eq!(
        board.call("octo/widgets#7"),
        json!({
            "path": "/gh/octo/widgets/issues/7",
            "type": "github/issue/v1",
            "written": false,
            "url": "https://github.com/octo/widgets/issues/7",
            "title": null,
        })
    );
    assert_eq!(board.gh_calls(), 0);
}

#[test]
fn an_empty_title_is_left_out_of_the_record() {
    let board = board();
    board.gh_answers(&issue(7, "", false), 0, "");
    assert_eq!(board.call("octo/widgets#7")["title"], Value::Null);
    let record = board.read("gh/octo/widgets/issues/7.json");
    assert!(record.get("title").is_none(), "{record}");
}

#[test]
fn owner_and_repository_are_stored_in_lower_case_and_the_url_as_github_gives_it() {
    let board = board();
    let mut answer = issue(12, "Widgets wobble", true);
    answer["html_url"] = json!("https://github.com/Octo/Widgets/pull/12");
    board.gh_answers(&answer, 0, "");
    assert_eq!(
        board.call("Octo/Widgets#12")["path"],
        "/gh/octo/widgets/pulls/12"
    );
    let record = board.read("gh/octo/widgets/pulls/12.json");
    assert_eq!(
        (&record["owner"], &record["repo"], &record["url"]),
        (
            &json!("octo"),
            &json!("widgets"),
            &json!("https://github.com/Octo/Widgets/pull/12")
        )
    );
}

#[test]
fn a_ref_differing_only_in_case_finds_the_same_record_without_asking_gh() {
    let board = board();
    board.gh_answers(&issue(12, "Widgets wobble", true), 0, "");
    let mut first = board.call("octo/widgets#12");
    first["written"] = json!(false);
    assert_eq!(board.call("Octo/WIDGETS#12"), first);
    assert_eq!(board.gh_calls(), 1);
}

#[test]
fn a_hand_written_record_in_capitals_is_degraded_and_one_in_lower_case_is_reported() {
    let board = board();
    let url = "https://github.com/octo/widgets/issues/7";
    board.board.write(
        "gh/octo/widgets/issues/7.json",
        &seven(&json!({"owner": "Octo", "url": url})),
    );
    assert_eq!(
        board.refusal("octo/widgets#7")["code"],
        "github::record::degraded"
    );
    board.board.write(
        "gh/octo/widgets/issues/7.json",
        &seven(&json!({"url": url})),
    );
    assert_eq!(board.call("octo/widgets#7")["written"], false);
}

#[test]
fn each_malformed_ref_is_refused_and_its_nearest_well_formed_twin_is_admitted() {
    let board = board();
    let pairs = [
        ("octo/widgets#0", "octo", "widgets", 1),
        ("octo/widgets", "octo", "widgets", 2),
        ("octo#3", "octo", "octo", 3),
        ("octo/widgets/x#4", "octo", "widgets-x", 4),
        ("octo/..#5", "octo", ".github", 5),
    ];
    for (bad, owner, repo, number) in pairs {
        let refused = board.refusal(bad);
        assert_eq!(refused["code"], "board::plugin::bad_input", "{bad}");
        let mut answer = issue(number, "Widgets wobble", false);
        answer["html_url"] = json!(format!("https://github.com/{owner}/{repo}/issues/{number}"));
        board.gh_answers(&answer, 0, "");
        let good = format!("{owner}/{repo}#{number}");
        assert_eq!(
            board.call(&good)["path"],
            format!("/gh/{owner}/{repo}/issues/{number}"),
            "{good}"
        );
    }
    assert_eq!(board.gh_calls(), pairs.len());
}

#[test]
fn a_number_github_does_not_know_is_refused_and_one_it_knows_is_admitted() {
    let board = board();
    board.gh_answers(
        &json!(r#"{"message":"Not Found"}"#),
        1,
        "gh: Not Found (HTTP 404)\n",
    );
    let failure = board.refusal("octo/widgets#99");
    assert_eq!(failure["code"], "github::ref::not_found");
    let message = failure["message"].as_str().unwrap();
    assert!(message.contains("octo/widgets#99"), "{message}");
    assert!(!board.exists("gh"));
    board.gh_answers(&issue(99, "Widgets wobble", false), 0, "");
    assert_eq!(board.call("octo/widgets#99")["written"], true);
}

#[test]
fn a_failing_gh_is_refused_with_its_error_and_a_working_one_is_admitted() {
    let board = board();
    board.gh_answers(
        &json!(""),
        4,
        "To get started with GitHub CLI, please run:  gh auth login\n",
    );
    let failure = board.refusal("octo/widgets#7");
    assert_eq!(failure["code"], "github::gh::failed");
    let message = failure["message"].as_str().unwrap();
    assert!(message.contains("gh auth login"), "{message}");
    board.gh_answers(&issue(7, "Widgets wobble", false), 0, "");
    assert_eq!(board.call("octo/widgets#7")["written"], true);
}

#[test]
fn gh_missing_from_path_is_refused_and_gh_on_path_is_admitted() {
    let mut board = board();
    board.gh_answers(&issue(7, "Widgets wobble", false), 0, "");
    // A `PATH` of an empty directory, since a runner may have gh in
    // `/usr/bin`.
    let empty = Scenario::new();
    let with_gh = std::mem::replace(&mut board.board.path, empty.workdir().display().to_string());
    let failure = board.refusal("octo/widgets#7");
    assert_eq!(failure["code"], "github::gh::failed");
    let message = failure["message"].as_str().unwrap();
    assert!(message.contains("did not start"), "{message}");
    board.board.path = with_gh;
    assert_eq!(board.call("octo/widgets#7")["written"], true);
}

#[test]
fn an_answer_without_a_page_url_is_refused_and_one_with_it_is_admitted() {
    let board = board();
    let answer = issue(7, "Widgets wobble", false);
    let mut without = answer.clone();
    without.as_object_mut().unwrap().remove("html_url");
    board.gh_answers(&without, 0, "");
    let failure = board.refusal("octo/widgets#7");
    assert_eq!(failure["code"], "github::gh::failed");
    let message = failure["message"].as_str().unwrap();
    assert!(
        message.contains("run `gh api repos/octo/widgets/issues/7`"),
        "{message}"
    );
    assert!(!message.contains("gh auth status"), "{message}");
    board.gh_answers(&answer, 0, "");
    assert_eq!(board.call("octo/widgets#7")["written"], true);
}

#[test]
fn a_degraded_record_already_there_is_refused_and_a_valid_one_is_reported() {
    let board = board();
    board
        .board
        .write("gh/octo/widgets/issues/7.json", &seven(&json!({})));
    let failure = board.refusal("octo/widgets#7");
    assert_eq!(failure["code"], "github::record::degraded");
    assert_eq!(board.gh_calls(), 0);
    board.board.write(
        "gh/octo/widgets/issues/7.json",
        &seven(&json!({"url": "https://github.com/octo/widgets/issues/7"})),
    );
    assert_eq!(board.call("octo/widgets#7")["written"], false);
}

#[test]
fn board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships() {
    let board = board();
    let report = board.board.json(&["plugin", "list"]);
    let plugin = (report["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "github")
        .unwrap();
    assert_eq!(
        (&plugin["status"], &plugin["enabled"], &plugin["offers"]),
        (&json!("ok"), &json!(true), &json!(["ref"]))
    );
    let shipped = shipped("github");
    assert_eq!(shipped.len(), 2, "the pull and issue schemas");
    for (path, text) in shipped {
        assert_eq!(board.board.read(&path), text, "{path}");
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_by_board_and_ref_is_answered() {
    let board = board();
    board.board.write(
        "gh/octo/widgets/issues/7.json",
        &seven(&json!({"url": "https://github.com/octo/widgets/issues/7"})),
    );
    let refused = board
        .board
        .refusal("github.push", &["--ref", "octo/widgets#7"]);
    assert_eq!(refused["code"], "board::plugin::not_installed", "{refused}");
    assert_eq!(board.call("octo/widgets#7")["written"], false);
}

#[test]
fn ref_answers_from_a_warm_index_as_from_a_cold_one_after_the_record_changes() {
    let board = board();
    let flags = ["--ref", "octo/widgets#7"];
    let url = json!({"url": "https://github.com/octo/widgets/issues/7"});
    let path = "gh/octo/widgets/issues/7.json";
    board.board.write(path, &seven(&url));
    let before = board.board.answer("github.ref", &flags, false);
    let mut retitled = url.clone();
    retitled["title"] = json!("Widgets wobble");
    board.board.write(path, &seven(&retitled));
    let warm = board.board.answer("github.ref", &flags, false);
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm["title"], "Widgets wobble");
    assert_eq!(warm, board.board.answer("github.ref", &flags, true));
    assert_eq!(board.gh_calls(), 0);
}
