//! The share plugin through the binary: `share.to` from a board into a
//! mounted team board that commits its puts. `share/refusals.rs` holds the
//! refusals of what the set holds and of where it goes. These were the
//! plugin's pytest suite.
//!
//! The source board does not commit its puts, where the pytest suite's
//! did; the share moves the mount's pin in its working tree either way.

use super::*;

mod refusals;

const EVAL_SCHEMA: &str = ".board/types/evals/eval/v1.schema.json";
const RUN_SCHEMA: &str = ".board/types/runs/run/v1.schema.json";
const SUITE_SCHEMA: &str = ".board/types/runs/suite/v1.schema.json";
/// What sharing `/evals/x` copies, by logical path.
const SHARED: [&str; 5] = [
    "/evals/x",
    "/evals/x/evidence/log.txt",
    "/notes/cases.md",
    "/suites/x",
    "/suites/x/runs/1",
];
/// The files sharing `/evals/x` writes in the team board.
const FILES: [&str; 8] = [
    EVAL_SCHEMA,
    RUN_SCHEMA,
    SUITE_SCHEMA,
    "evals/x.json",
    "evals/x/evidence/log.txt",
    "notes/cases.md",
    "suites/x.json",
    "suites/x/runs/1.json",
];
const TYPES: [&str; 3] = ["evals/eval/v1", "runs/run/v1", "runs/suite/v1"];
/// A Markdown task's type, whose frontmatter references are followed.
const TASK_SCHEMA: &str = ".board/types/lr/task/v1.schema.json";
const TASK_TYPE: &str = r#"{"type": "object", "properties": {"eval": {"type": "string", "format": "board-ref"}, "see": {"type": "string", "format": "board-ref"}}}"#;

fn eval() -> Value {
    json!({
        "$type": "evals/eval/v1",
        "question": "Does gix resolve a ref faster than git2?",
        "subjects": ["o/gix", "o/git2"],
        "suites": ["/suites/x"],
        "runs": ["/suites/x/runs/1"],
        "status": "concluded",
        "verdict": "supported",
    })
}

fn run_record() -> Value {
    json!({
        "$type": "runs/run/v1",
        "suite": "/suites/x",
        "by": {"program": "claude-code"},
        "started": "2026-10-01T09:00:00Z",
        "scope": "full",
        "results": [{"case": "cold", "outcome": "pass", "evidence": ["/evals/x/evidence/log.txt"]}],
    })
}

/// A board enabling share, evals and runs, which mounts at `/team` a team
/// board that commits its puts, as git would with an identity of its own.
struct Boards {
    personal: Fixture,
    team: Scenario,
}

fn boards() -> Boards {
    let personal = Fixture::enabling(&["share", "evals", "runs"]);
    let team = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": true}"#;
    team.write(".board/board.json", Item::File(config));
    let first = team.commit(&[(".board/board.json", config)]);
    team.track(first)
        .write(".git/config", Item::Appended(IDENTITY));
    mount(&personal, "team.json", &team, first);
    Boards { personal, team }
}

impl Boards {
    /// Writes `/evals/x`, its suite naming `cases_file`, the cases file,
    /// its run and that run's evidence, with `fields` in the eval.
    fn experiment(&self, cases_file: &str, fields: &Value) -> &Self {
        let suite = json!({
            "$type": "runs/suite/v1",
            "subjects": ["o/gix", "o/git2"],
            "method": "Resolve 1,000 refs and compare wall time.",
            "cases_file": cases_file,
        });
        self.personal
            .write("evals/x.json", &record(eval(), fields))
            .write("suites/x.json", &suite.to_string())
            .write("suites/x/runs/1.json", &run_record().to_string())
            .write("evals/x/evidence/log.txt", "resolved 1000 refs in 41 ms\n")
            .write("notes/cases.md", "# Cases\n\n- cold\n");
        self
    }

    fn standard(&self) -> &Self {
        self.experiment("/notes/cases.md", &json!({}))
    }

    /// The output of `board share to` with `flags` and `--to /team`
    /// unless they name another.
    fn share(&self, flags: &[&str]) -> Value {
        self.personal.call("share.to", &with_to(flags))
    }

    /// The failure `board share to` prints when it is refused.
    fn refused(&self, flags: &[&str]) -> Value {
        self.personal.refusal("share.to", &with_to(flags))
    }

    /// Commits `files` in the team board onto its HEAD, in its working tree
    /// and index, and moves the mount's pin to the commit.
    fn publish(&self, files: &[(&str, &str)]) -> ObjectId {
        let mut all: BTreeMap<String, String> = (self.team.head().unwrap().files.into_iter())
            .map(|(at, bytes)| (at, String::from_utf8(bytes).unwrap()))
            .collect();
        for (at, text) in files {
            all.insert((*at).to_owned(), (*text).to_owned());
            self.team.write(at, Item::File(text));
        }
        let borrowed: Vec<(&str, &str)> = (all.iter())
            .map(|(at, text)| (at.as_str(), text.as_str()))
            .collect();
        let commit = self.team.commit(&borrowed);
        self.team.track(commit);
        mount(&self.personal, "team.json", &self.team, commit);
        commit
    }

    /// The commit the mount of the team board is pinned to.
    fn pin(&self) -> String {
        let pointer: Value = serde_json::from_str(&self.personal.read("team.json")).unwrap();
        pointer["commit"].as_str().unwrap().to_owned()
    }

    fn head(&self) -> board_scenario::HeadCommit {
        self.team.head().unwrap()
    }

    /// The files the team's HEAD added or changed since `before`.
    fn changed(&self, before: &board_scenario::HeadCommit) -> Vec<String> {
        (self.head().files.into_iter())
            .filter(|(at, bytes)| before.files.get(at) != Some(bytes))
            .map(|(at, _)| at)
            .collect()
    }

    fn exists(&self, path: &str) -> bool {
        self.team.workdir().join(path).exists()
    }
}

/// `flags` with `--to /team` first unless they name another destination.
fn with_to<'a>(flags: &[&'a str]) -> Vec<&'a str> {
    let named = flags.contains(&"--to");
    (!named)
        .then_some(["--to", "/team"])
        .into_iter()
        .flatten()
        .chain(flags.iter().copied())
        .collect()
}

/// `directory` with its links resolved, as Board names a repository.
fn real(directory: &Path) -> String {
    std::fs::canonicalize(directory)
        .unwrap()
        .display()
        .to_string()
}

fn strings(paths: &[&str]) -> Value {
    json!(paths)
}

#[test]
fn one_call_copies_an_eval_its_runs_their_evidence_and_its_schemas_in_one_commit_and_a_clone_checks_clean()
 {
    let boards = boards();
    boards.standard();
    let before = boards.head();
    let output = boards.share(&["--path", "/evals/x"]);
    let head = boards.head();
    assert_eq!(
        output,
        json!({
            "repository": real(boards.team.workdir()),
            "commit": head.id.to_string(),
            "types": TYPES,
            "copied": SHARED,
            "uris": [],
            "total": 5,
            "truncated": false,
            "unchanged": 0,
        })
    );
    assert_eq!(boards.changed(&before), FILES);
    assert_eq!(
        head.parents,
        [before.id],
        "the fixture's commit and the share's"
    );
    assert_eq!(boards.pin(), head.id.to_string());
    assert!(
        boards
            .personal
            .board
            .workdir()
            .join("evals/x.json")
            .exists()
    );
    let clone = Scenario::clone_of(&boards.team);
    let checked = board_in(clone.workdir(), &["check"]);
    assert_eq!(checked.code, 0, "{}", text(&checked.stdout));
    let got = json(&board_in(clone.workdir(), &["--json", "get", "/evals/x"]));
    let location = &got["data"][0]["location"];
    assert_eq!(
        (&location["repository"], &location["path"]),
        (&json!(real(clone.workdir())), &json!("evals/x.json"))
    );
}

#[test]
fn a_share_into_a_team_board_with_an_origin_gives_each_copys_uri_and_one_without_gives_none() {
    let boards = boards();
    boards.standard();
    let dry = boards.share(&["--path", "/evals/x", "--dry_run", "true"]);
    assert_eq!(dry["uris"], json!([]));
    let remote = "[remote \"origin\"]\n\turl = git@github.com:acme/team-board.git\n";
    boards.team.write(".git/config", Item::Appended(remote));
    let output = boards.share(&["--path", "/evals/x"]);
    let uris: Vec<String> = (SHARED.iter())
        .map(|path| format!("board://acme/team-board{path}"))
        .collect();
    assert_eq!(output["uris"], json!(uris));
}

#[test]
fn a_schema_the_team_holds_is_not_added_again_and_one_it_lacks_lands_with_the_records() {
    let boards = boards();
    boards.standard();
    let schema = boards.personal.read(RUN_SCHEMA);
    let published = boards.publish(&[(RUN_SCHEMA, &schema)]);
    let before = boards.head();
    let output = boards.share(&["--path", "/evals/x"]);
    assert_eq!(output["types"], json!(["evals/eval/v1", "runs/suite/v1"]));
    assert_eq!(boards.head().parents, [published]);
    let changed = boards.changed(&before);
    assert!(changed.contains(&EVAL_SCHEMA.to_owned()));
    assert!(!changed.contains(&RUN_SCHEMA.to_owned()));
}

#[test]
fn a_path_the_team_holds_differently_refuses_the_share_and_writes_nothing_and_one_it_holds_equally_is_left()
 {
    let boards = boards();
    boards.standard();
    let other = record(run_record(), &json!({"by": {"person": "someone-else"}}));
    let before = boards.publish(&[("suites/x/runs/1.json", &other)]);
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::target::differs");
    assert_eq!(refused["fields"]["paths"], strings(&["/suites/x/runs/1"]));
    assert_eq!(boards.head().id, before);
    assert!(!boards.exists(".board/types/evals"));
    let indented = serde_json::to_string_pretty(&run_record()).unwrap();
    boards.publish(&[("suites/x/runs/1.json", &indented)]);
    let output = boards.share(&["--path", "/evals/x"]);
    assert_eq!(
        (&output["total"], &output["unchanged"]),
        (&json!(4), &json!(1))
    );
}

#[test]
fn a_schema_the_team_holds_differently_refuses_the_share_and_an_equal_one_is_admitted() {
    let boards = boards();
    boards.standard();
    let schema: Value = serde_json::from_str(&boards.personal.read(RUN_SCHEMA)).unwrap();
    let other = record(schema.clone(), &json!({"title": "Another run"}));
    let before = boards.publish(&[(RUN_SCHEMA, &other)]);
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::target::differs");
    assert_eq!(
        refused["fields"]["paths"],
        strings(&["/.board/types/runs/run/v1.schema"])
    );
    assert_eq!(boards.head().id, before);
    boards.publish(&[(RUN_SCHEMA, &schema.to_string())]);
    boards.share(&["--path", "/evals/x"]);
}

#[test]
fn sharing_again_writes_nothing() {
    let boards = boards();
    boards.standard();
    let first = boards.share(&["--path", "/evals/x"])["commit"].clone();
    let output = boards.share(&["--path", "/evals/x"]);
    assert_eq!(
        (
            &output["commit"],
            &output["copied"],
            &output["unchanged"],
            &output["types"]
        ),
        (&Value::Null, &json!([]), &json!(5), &json!([]))
    );
    assert_eq!(json!(boards.head().id.to_string()), first);
    assert_eq!(json!(boards.pin()), first);
}

#[test]
fn a_share_killed_before_the_pin_moved_is_finished_by_sharing_again() {
    let boards = boards();
    boards.standard();
    let behind = boards.head().id;
    let first = boards.share(&["--path", "/evals/x"])["commit"].clone();
    mount(&boards.personal, "team.json", &boards.team, behind);
    let output = boards.share(&["--path", "/evals/x"]);
    assert_eq!(output["commit"], first);
    assert_eq!(json!(boards.head().id.to_string()), first);
    assert_eq!(json!(boards.pin()), first);
}

#[test]
fn a_share_killed_before_the_mounted_board_committed_is_finished_by_sharing_again() {
    let boards = boards();
    boards.standard();
    let before = boards.head();
    boards.share(&["--path", "/evals/x"]);
    // As `git reset --mixed HEAD~1` in the team board: the copies stay in
    // its working tree, and neither board has committed them.
    let reset = format!("{}\n", before.id);
    boards
        .team
        .write(".git/refs/heads/main", Item::File(&reset));
    boards.team.track(before.id);
    mount(&boards.personal, "team.json", &boards.team, before.id);
    boards.share(&["--path", "/evals/x"]);
    let head = boards.head();
    assert_eq!(head.parents, [before.id]);
    assert_eq!(boards.changed(&before), FILES);
    assert_eq!(boards.pin(), head.id.to_string());
    assert_eq!(
        boards.team.index().len(),
        head.files.len(),
        "nothing is left to commit"
    );
}

#[test]
fn a_shared_path_holding_other_uncommitted_bytes_is_refused_naming_it_and_admitted_once_they_are_removed()
 {
    let boards = boards();
    boards.standard();
    boards
        .team
        .write("notes/cases.md", Item::File("# Edited\n"));
    let before = boards.head().id;
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(
        (&refused["code"], &refused["fields"]["code"]),
        (
            &json!("share::board::refused"),
            &json!("board::put::invalid")
        )
    );
    let refusals = refused["fields"]["refusals"].as_array().unwrap();
    assert_eq!(refusals.len(), 1, "{refused}");
    assert_eq!(refusals[0]["code"], "board::mount::stale");
    let message = refusals[0]["message"].as_str().unwrap();
    assert!(message.contains("/team/notes/cases.md"), "{message}");
    assert_eq!(
        (boards.head().id.to_string(), boards.pin()),
        (before.to_string(), before.to_string())
    );
    assert_eq!(
        std::fs::read_to_string(boards.team.workdir().join("notes/cases.md")).unwrap(),
        "# Edited\n"
    );
    boards.team.write("notes/cases.md", Item::Deleted);
    boards.share(&["--path", "/evals/x"]);
}

#[test]
fn the_documented_command_line_runs_with_flags_named_after_the_input_and_a_dashed_flag_is_refused()
{
    let boards = boards();
    boards.standard();
    let before = boards.head().id;
    let call = ["--path", "/evals/x"];
    let dry = [
        &call[..],
        &["--dry_run", "true", "--local_attachments", "true"],
    ]
    .concat();
    assert_eq!(boards.share(&dry)["copied"], json!(SHARED));
    assert_eq!(boards.head().id, before);
    let dashed = [&call[..], &["--dry-run", "true"]].concat();
    let arguments: Vec<&str> = ["--json", "call", "share.to"]
        .into_iter()
        .chain(with_to(&dashed))
        .collect();
    let refused = boards.personal.run(&arguments);
    assert_eq!(
        (refused.code, json(&refused)["code"].clone()),
        (2, json!("board::cli::usage"))
    );
    boards.share(&call);
    assert_eq!(boards.pin(), boards.head().id.to_string());
    assert_ne!(boards.head().id, before);
}

#[test]
fn a_dry_run_lists_what_it_would_copy_and_writes_nothing() {
    let boards = boards();
    boards.standard();
    let before = boards.head().id;
    let output = boards.share(&["--path", "/evals/x", "--dry_run", "true"]);
    assert_eq!(
        (&output["commit"], &output["copied"], &output["types"]),
        (&Value::Null, &json!(SHARED), &json!(TYPES))
    );
    assert_eq!(boards.head().id, before);
    assert_eq!(boards.pin(), before.to_string());
}

#[test]
fn a_path_with_nothing_in_the_boards_own_tree_is_refused_and_one_with_a_record_is_shared() {
    let boards = boards();
    boards.standard();
    let refused = boards.refused(&["--path", "/evals/y"]);
    assert_eq!(refused["code"], "share::path::not_found");
    assert_eq!(
        refused["message"],
        "/evals/y names nothing in this board's own tree"
    );
    boards.share(&["--path", "/evals/x"]);
}

#[test]
fn the_eval_record_is_copied_as_its_value() {
    let boards = boards();
    boards.standard();
    boards.share(&["--path", "/evals/x"]);
    let copied: Value = serde_json::from_str(
        &std::fs::read_to_string(boards.team.workdir().join("evals/x.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(copied, eval());
}

#[test]
fn a_reference_to_a_directory_copies_the_entries_under_it() {
    let boards = boards();
    boards.experiment("/notes", &json!({}));
    boards.personal.write("notes/more/other.md", "# Other\n");
    let output = boards.share(&["--path", "/evals/x"]);
    let mut expected: Vec<&str> = SHARED.to_vec();
    expected.push("/notes/more/other.md");
    expected.sort_unstable();
    assert_eq!(output["copied"], json!(expected));
    assert_eq!(
        std::fs::read_to_string(boards.team.workdir().join("notes/more/other.md")).unwrap(),
        "# Other\n"
    );
}

#[test]
fn a_note_a_record_references_is_copied_and_the_notes_and_directories_it_links_to_are_not() {
    let boards = boards();
    boards.standard();
    boards
        .personal
        .write(
            "notes/cases.md",
            "# Cases\n\nSee [other](other.md) and [more](more/).\n",
        )
        .write("notes/other.md", "# Other\n")
        .write("notes/more/third.md", "# Third\n");
    let output = boards.share(&["--path", "/evals/x"]);
    assert_eq!(output["copied"], json!(SHARED));
    assert!(boards.exists("notes/cases.md"));
    assert!(!boards.exists("notes/other.md"));
}

#[test]
fn a_markdown_records_frontmatter_references_and_schema_are_copied_and_its_body_links_are_not() {
    let boards = boards();
    boards.standard();
    let task =
        "---\n$type: lr/task/v1\neval: /evals/x\n---\n# Task\n\nSee [other](../notes/other.md).\n";
    boards
        .personal
        .write(TASK_SCHEMA, TASK_TYPE)
        .write("tasks/t.md", task)
        .write("notes/other.md", "# Other\n");
    let output = boards.share(&["--path", "/tasks/t.md"]);
    let mut copied: Vec<&str> = SHARED.to_vec();
    copied.push("/tasks/t.md");
    copied.sort_unstable();
    let mut types: Vec<&str> = TYPES.to_vec();
    types.push("lr/task/v1");
    types.sort_unstable();
    assert_eq!(
        (&output["copied"], &output["types"]),
        (&json!(copied), &json!(types))
    );
    assert_eq!(
        std::fs::read_to_string(boards.team.workdir().join("tasks/t.md")).unwrap(),
        task
    );
    assert!(boards.exists(TASK_SCHEMA));
    assert!(!boards.exists("notes/other.md"));
}

#[test]
fn a_body_link_written_as_a_frontmatter_pointer_is_not_followed_and_the_reference_at_that_pointer_is()
 {
    let boards = boards();
    boards.standard();
    // The reference at the pointer `/see` reaches /other, and the link
    // written `/see` reaches /see.
    boards
        .personal
        .write(TASK_SCHEMA, TASK_TYPE)
        .write("see/x.md", "# Linked\n")
        .write("other/y.md", "# Referenced\n")
        .write(
            "tasks/t.md",
            "---\n$type: lr/task/v1\nsee: /other\n---\n[s](/see)\n",
        );
    let dry = ["--path", "/tasks/t.md", "--dry_run", "true"];
    assert_eq!(
        boards.share(&dry)["copied"],
        json!(["/other/y.md", "/tasks/t.md"])
    );
    boards.personal.write(
        "tasks/t.md",
        "---\n$type: lr/task/v1\nsee: /see\n---\n[s](/see)\n",
    );
    assert_eq!(
        boards.share(&dry)["copied"],
        json!(["/see/x.md", "/tasks/t.md"])
    );
}

#[test]
fn the_output_lists_at_most_100_paths_and_counts_them_all() {
    let boards = boards();
    boards.standard();
    for number in 0..101 {
        let path = format!("evals/x/evidence/{number:03}.txt");
        boards.personal.write(&path, &format!("{number}\n"));
    }
    let output = boards.share(&["--path", "/evals/x", "--dry_run", "true"]);
    assert_eq!(
        (
            output["copied"].as_array().unwrap().len(),
            &output["total"],
            &output["truncated"]
        ),
        (100, &json!(106), &json!(true))
    );
}

#[test]
fn a_share_answers_from_a_warm_index_as_from_a_cold_one_after_the_set_changes() {
    let boards = boards();
    boards.standard();
    let dry = ["--to", "/team", "--path", "/evals/x", "--dry_run", "true"];
    let before = boards.personal.answer("share.to", &dry, false);
    boards.personal.write("evals/x/evidence/more.txt", "more\n");
    let warm = boards.personal.answer("share.to", &dry, false);
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm, boards.personal.answer("share.to", &dry, true));
}
