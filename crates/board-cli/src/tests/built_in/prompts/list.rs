//! `prompts.list` through the binary: order, scope by the caller's remote,
//! unmarked mounts and degraded files. These were `test_list.py`.

use super::*;

/// Each listed prompt's repo and title.
fn titles(output: &Value) -> Vec<(String, String)> {
    (output["prompts"].as_array().unwrap().iter())
        .map(|prompt| {
            let field = |name: &str| prompt[name].as_str().unwrap().to_owned();
            (field("repo"), field("title"))
        })
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    (expected.iter())
        .map(|(repo, title)| ((*repo).to_owned(), (*title).to_owned()))
        .collect()
}

/// A `notes/repo/v1` record covering `covers`.
fn notes_record(covers: &[&str], personal: bool) -> String {
    json!({"$type": "notes/repo/v1", "covers": covers, "personal": personal}).to_string()
}

fn list(fixture: &Fixture, flags: &[&str]) -> Value {
    let (passed, output) = call(fixture, "list", flags, "");
    assert!(passed, "{output}");
    output
}

#[test]
fn list_shows_each_prompt_as_written_by_repo_then_title_and_answers_the_same_from_a_cold_index() {
    let board = board();
    board
        .write(
            "prompts/b.md",
            &prompt("B", "{pr}", &json!({"inputs": {"pr": "The number."}})),
        )
        .write(
            "projects/x/prompts/a.md",
            &prompt("A", "", &json!({"runs_in": {"directory": "~/Code"}})),
        );
    mounted(
        &board,
        "notes",
        &[("prompts/c.md", prompt("C", "", &json!({})))],
        true,
        true,
    );
    let output = list(&board, &[]);
    assert_eq!(
        titles(&output),
        pairs(&[("/", "A"), ("/", "B"), ("/repos/notes", "C")])
    );
    assert_eq!(
        output["prompts"][1],
        json!({
            "repo": "/",
            "path": "/prompts/b.md",
            "title": "B",
            "purpose": "A clean-context review of one pull request.",
            "runs_in": {"remote": "example-org/widgets"},
            "inputs": {"pr": "The number."},
        })
    );
    assert_eq!(
        (
            &output["total"],
            &output["degraded"],
            &output["repos"],
            &output["origin_unread"]
        ),
        (
            &json!(3),
            &json!(0),
            &json!(["/", "/repos/notes"]),
            &Value::Null
        )
    );
    assert_eq!(board.answer("prompts.list", &[], true), output);
}

#[test]
fn a_prompt_record_under_prompts_that_is_not_valid_is_counted_as_degraded() {
    let board = board();
    let none = json!({});
    board
        .write("prompts/good.md", &prompt("Good", "", &none))
        .write(
            "prompts/linked.md",
            &prompt("Linked", "[plan](/plan.md)", &none),
        )
        .write(
            "prompts/invalid.md",
            &prompt("Invalid", "", &json!({"purpose": null})),
        )
        .write("notes/elsewhere.md", &prompt("Elsewhere", "", &none));
    let output = list(&board, &[]);
    assert_eq!(titles(&output), pairs(&[("/", "Good"), ("/", "Linked")]));
    assert_eq!(output["degraded"], 1);
}

#[test]
fn a_file_under_prompts_whose_frontmatter_does_not_parse_or_names_an_unknown_type_is_counted() {
    let board = board();
    board
        .write("prompts/good.md", &prompt("Good", "", &json!({})))
        .write(
            "prompts/unparsed.md",
            "---\n$type: \"prompts/prompt/v1\ntitle: [Broken\n---\n",
        )
        .write(
            "prompts/misspelt.md",
            &prompt("Misspelt", "", &json!({"$type": "prompts/promt/v1"})),
        )
        .write(
            "prompts/later.md",
            &prompt("Later", "", &json!({"$type": "prompts/prompt/v2"})),
        );
    let output = list(&board, &[]);
    assert_eq!(titles(&output), pairs(&[("/", "Good")]));
    assert_eq!(output["degraded"], 3);
}

#[test]
fn a_markdown_file_under_prompts_that_is_no_prompt_record_is_not_counted() {
    let board = board();
    board
        .write("prompts/good.md", &prompt("Good", "", &json!({})))
        .write("prompts/untyped.md", "# A note\n")
        .write("prompts/linking.md", "# A note\n\n[gone](/gone.md)\n")
        .write(
            "evals/x/evidence/prompts/scorer.md",
            "# A scorer's prompt, kept as evidence\n",
        );
    let output = list(&board, &[]);
    assert_eq!(titles(&output), pairs(&[("/", "Good")]));
    assert_eq!(output["degraded"], 0);
}

#[test]
fn a_mount_the_board_does_not_mark_is_skipped_and_its_files_are_not_counted() {
    let board = board();
    let code = [
        ("prompts/a.md", prompt("Code", "", &json!({}))),
        ("prompts/broken.md", "# Not a prompt\n".to_owned()),
        ("daily/x.md", "x".to_owned()),
    ];
    mounted(&board, "widgets", &code, false, true);
    mounted(
        &board,
        "notes",
        &[("prompts/a.md", prompt("Notes", "", &json!({})))],
        true,
        true,
    );
    let output = list(&board, &[]);
    assert_eq!(titles(&output), pairs(&[("/repos/notes", "Notes")]));
    assert_eq!(
        (&output["repos"], &output["degraded"]),
        (&json!(["/", "/repos/notes"]), &json!(0))
    );
}

#[test]
fn list_refuses_a_list_of_marked_repos_naming_a_mount_the_board_lacks_and_answers_once_it_is_mounted()
 {
    let board = board();
    mounted(
        &board,
        "notes",
        &[("prompts/a.md", prompt("Notes", "", &json!({})))],
        true,
        true,
    );
    mark(&board, &["/repos/gone"]);
    let (passed, refused) = call(&board, "list", &[], "");
    assert!(!passed);
    assert_eq!(refused["code"], "prompts::repos::invalid");
    mounted(
        &board,
        "gone",
        &[("README.md", "x".to_owned())],
        false,
        true,
    );
    let output = list(&board, &[]);
    assert_eq!(output["repos"], json!(["/", "/repos/gone", "/repos/notes"]));
}

/// A board with its own prompt and three marked notes repos: a personal
/// one, one covering example-org and one covering acme.
fn routed() -> Board {
    let board = board();
    board.write("prompts/own.md", &prompt("Own", "", &json!({})));
    let notes = |name: &str, title: &str, record: String| {
        let files = [
            ("prompts/a.md", prompt(title, "", &json!({}))),
            ("notes-repo.json", record),
        ];
        mounted(&board, name, &files, true, true);
    };
    notes("personal", "Personal", notes_record(&[], true));
    notes("work", "Work", notes_record(&["example-org/*"], false));
    notes("other", "Other", notes_record(&["acme/*"], false));
    board
}

#[test]
fn the_callers_remote_chooses_its_covering_repo_with_the_personal_repo_and_repo_names_one() {
    let board = routed();
    let checkout = Scenario::new();
    let origin = "[remote \"origin\"]\n\turl = git@github.com:example-org/home.git\n";
    checkout.write(".git/config", Item::Appended(origin));
    let (_, output) = call_from(&board, Some(checkout.workdir()), "list", &[], "");
    assert_eq!(
        titles(&output),
        pairs(&[
            ("/", "Own"),
            ("/repos/personal", "Personal"),
            ("/repos/work", "Work")
        ])
    );
    assert_eq!(output["origin_unread"], Value::Null);
    assert_eq!(
        titles(&list(&board, &["--remote", "acme/x"])),
        pairs(&[
            ("/", "Own"),
            ("/repos/other", "Other"),
            ("/repos/personal", "Personal")
        ])
    );
    assert_eq!(
        titles(&list(&board, &["--repo", "/repos/work"])),
        pairs(&[("/repos/work", "Work")])
    );
    let (passed, refused) = call(&board, "list", &["--repo", "/repos/elsewhere"], "");
    assert!(!passed);
    assert_eq!(refused["code"], "prompts::repo::not_found");
}

#[test]
fn a_caller_repository_without_origin_reads_only_the_board_and_the_personal_repo_and_says_why() {
    let board = routed();
    let (bare, originated) = (Scenario::new(), Scenario::new());
    let origin = "[remote \"origin\"]\n\turl = https://github.com/acme/x\n";
    originated.write(".git/config", Item::Appended(origin));
    let (_, output) = call_from(&board, Some(bare.workdir()), "list", &[], "");
    assert_eq!(
        titles(&output),
        pairs(&[("/", "Own"), ("/repos/personal", "Personal")])
    );
    let why = format!(
        "git gave no origin for {}: error: No such remote 'origin'",
        bare.workdir().display()
    );
    assert_eq!(output["origin_unread"], json!(why));
    let (_, output) = call_from(&board, Some(originated.workdir()), "list", &[], "");
    assert_eq!(
        titles(&output),
        pairs(&[
            ("/", "Own"),
            ("/repos/other", "Other"),
            ("/repos/personal", "Personal")
        ])
    );
    assert_eq!(output["origin_unread"], Value::Null);
}

#[test]
fn list_shows_at_most_100_prompts_and_counts_them_all() {
    let board = board();
    for number in 0..101 {
        let title = format!("P{number:03}");
        board.write(
            &format!("prompts/p{number:03}.md"),
            &prompt(&title, "", &json!({})),
        );
    }
    let output = list(&board, &[]);
    assert_eq!(
        (
            output["prompts"].as_array().unwrap().len(),
            &output["total"],
            &output["prompts"][99]["title"]
        ),
        (100, &json!(101), &json!("P099"))
    );
}

#[test]
fn a_notes_repo_whose_record_is_not_valid_is_refused_when_a_remote_routes_and_a_valid_one_routes() {
    let board = routed();
    assert!(call(&board, "list", &["--remote", "acme/x"], "").0);
    let files = [
        ("prompts/a.md", prompt("Broken", "", &json!({}))),
        (
            "notes-repo.json",
            json!({"$type": "notes/repo/v1", "covers": "acme/*"}).to_string(),
        ),
    ];
    mounted(&board, "broken", &files, true, true);
    let (passed, refused) = call(&board, "list", &["--remote", "acme/x"], "");
    assert!(!passed);
    assert_eq!(
        refused["code"], "prompts::repo::invalid_record",
        "{refused}"
    );
    assert_eq!(refused["fields"]["path"], "/repos/broken/notes-repo");
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.starts_with("/repos/broken/notes-repo is not a valid notes/repo/v1 record"),
        "{message}"
    );
    assert_eq!(list(&board, &[])["total"], 5);
}
