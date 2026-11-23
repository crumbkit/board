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
    team.track(first).identified();
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

    /// The output of `board share to --yes` with `flags` and `--to /team`
    /// unless they name another.
    fn share(&self, flags: &[&str]) -> Value {
        self.personal.call("share.to", &yes(flags))
    }

    /// The preview `board share to` prints without `--yes`.
    fn preview(&self, flags: &[&str]) -> Value {
        self.personal.call("share.to", &with_to(flags))
    }

    /// The failure `board share to --yes` prints when it is refused.
    fn refused(&self, flags: &[&str]) -> Value {
        self.personal.refusal("share.to", &yes(flags))
    }

    /// The failure the preview prints when it is refused.
    fn refused_preview(&self, flags: &[&str]) -> Value {
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

    /// The command `next` prints, which names this board with `-C`, with
    /// `rest` after `board -C <root>`.
    fn next(&self, rest: &str) -> String {
        format!("board -C {} {rest}", real(self.personal.board.workdir()))
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

/// `flags` with `--to /team`, as [`with_to`] adds it, and a bare `--yes`.
fn yes<'a>(flags: &[&'a str]) -> Vec<&'a str> {
    let mut flags = with_to(flags);
    flags.push("--yes");
    flags
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
            "shared": "/evals/x",
            "extras": [
                {"path": "/notes/cases.md", "via": "ref", "from": "/suites/x"},
                {"path": "/suites/x", "via": "ref", "from": "/evals/x"},
                {"path": "/suites/x/runs/1", "via": "ref", "from": "/evals/x"},
            ],
            "extras_total": 3,
            "not_copied": [],
            "not_copied_total": 0,
            "wholes": [],
            "wholes_total": 0,
            "next": null,
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
    let dry = boards.preview(&["--path", "/evals/x"]);
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
    let previewed = [&call[..], &["--local_attachments", "--allow-uncopied"]].concat();
    let preview = boards.preview(&previewed);
    assert_eq!(preview["copied"], json!(SHARED));
    assert_eq!(
        preview["next"],
        boards.next("share to --path /evals/x --to /team --yes --local_attachments")
    );
    assert_eq!(boards.head().id, before);
    let arguments: Vec<&str> = ["--json", "share", "to"]
        .into_iter()
        .chain(with_to(&call))
        .chain(["--local_attachments"])
        .collect();
    let bare = boards.personal.run(&arguments);
    assert_eq!(bare.code, 0, "{}", text(&bare.stdout));
    assert_eq!(
        json(&bare)["data"]["output"]["next"],
        boards.next("share to --path /evals/x --to /team --yes --local_attachments")
    );
    for wrong in ["--local-attachments", "--allow_uncopied"] {
        let dashed = [&call[..], &[wrong]].concat();
        let arguments: Vec<&str> = ["--json", "call", "share.to"]
            .into_iter()
            .chain(yes(&dashed))
            .collect();
        let refused = boards.personal.run(&arguments);
        assert_eq!(
            (refused.code, json(&refused)["code"].clone()),
            (2, json!("board::cli::usage")),
            "{wrong}"
        );
    }
    assert_eq!(boards.head().id, before);
    let arguments: Vec<&str> = ["--json", "share", "to"]
        .into_iter()
        .chain(yes(&[&call[..], &["--allow-uncopied"]].concat()))
        .collect();
    let written = boards.personal.run(&arguments);
    assert_eq!(written.code, 0, "{}", text(&written.stdout));
    assert_eq!(boards.pin(), boards.head().id.to_string());
    assert_ne!(boards.head().id, before);
}

#[test]
fn without_yes_the_share_lists_what_it_would_copy_and_the_command_that_writes_and_writes_nothing() {
    let boards = boards();
    boards.standard();
    let before = boards.head().id;
    let output = boards.preview(&["--path", "/evals/x"]);
    assert_eq!(
        (&output["commit"], &output["copied"], &output["types"]),
        (&Value::Null, &json!(SHARED), &json!(TYPES))
    );
    assert_eq!(
        (
            &output["shared"],
            &output["extras_total"],
            &output["extras"][1]
        ),
        (
            &json!("/evals/x"),
            &json!(3),
            &json!({"path": "/suites/x", "via": "ref", "from": "/evals/x"})
        )
    );
    assert_eq!(
        output["next"],
        boards.next("share to --path /evals/x --to /team --yes")
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
fn a_reference_to_a_directory_copies_the_entries_under_it_and_a_link_to_a_directory_under_it_is_not_listed()
 {
    let boards = boards();
    boards.experiment("/notes", &json!({}));
    boards
        .personal
        .write("notes/more/other.md", "# Other\n")
        .write("notes/cases.md", "# Cases\n\nSee [more](more/).\n");
    let output = boards.share(&["--path", "/evals/x"]);
    let mut expected: Vec<&str> = SHARED.to_vec();
    expected.push("/notes/more/other.md");
    expected.sort_unstable();
    assert_eq!(
        (&output["copied"], &output["not_copied"]),
        (&json!(expected), &json!([]))
    );
    assert_eq!(
        std::fs::read_to_string(boards.team.workdir().join("notes/more/other.md")).unwrap(),
        "# Other\n"
    );
}

#[test]
fn a_notes_relative_and_absolute_links_to_notes_are_followed_and_the_notes_copied() {
    let boards = boards();
    boards.standard();
    boards
        .personal
        .write(
            "notes/cases.md",
            "# Cases\n\nSee [other](other.md) and [third](/notes/more/third.md).\n",
        )
        .write("notes/other.md", "# Other\n")
        .write("notes/more/third.md", "# Third\n");
    let output = boards.share(&["--path", "/evals/x"]);
    let mut copied: Vec<&str> = SHARED.to_vec();
    copied.extend(["/notes/more/third.md", "/notes/other.md"]);
    copied.sort_unstable();
    assert_eq!(
        (&output["copied"], &output["not_copied"]),
        (&json!(copied), &json!([]))
    );
    let link = |path: &str| json!({"path": path, "via": "link", "from": "/notes/cases.md"});
    let extras = output["extras"].as_array().unwrap();
    for path in ["/notes/more/third.md", "/notes/other.md"] {
        assert!(extras.contains(&link(path)), "{path}: {extras:?}");
        assert!(boards.exists(&path[1..]), "{path}");
    }
}

#[test]
fn a_markdown_records_frontmatter_references_schema_and_relative_body_link_are_copied() {
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
    copied.extend(["/notes/other.md", "/tasks/t.md"]);
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
    assert!(boards.exists("notes/other.md"));
}

#[test]
fn a_body_link_to_a_directory_is_not_copied_and_a_frontmatter_reference_to_one_copies_its_entries()
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
    let dry = ["--path", "/tasks/t.md"];
    let preview = boards.preview(&dry);
    assert_eq!(
        (&preview["copied"], &preview["not_copied"]),
        (
            &json!(["/other/y.md", "/tasks/t.md"]),
            &json!([{"path": "/see", "reason": "directory", "from": "/tasks/t.md"}])
        )
    );
    boards.personal.write(
        "tasks/t.md",
        "---\n$type: lr/task/v1\nsee: /see\n---\n[s](/see)\n",
    );
    let preview = boards.preview(&dry);
    assert_eq!(
        (&preview["copied"], &preview["not_copied"]),
        (&json!(["/see/x.md", "/tasks/t.md"]), &json!([]))
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
    let output = boards.preview(&["--path", "/evals/x"]);
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
    let dry = ["--to", "/team", "--path", "/evals/x"];
    let before = boards.personal.answer("share.to", &dry, false);
    boards.personal.write("evals/x/evidence/more.txt", "more\n");
    let warm = boards.personal.answer("share.to", &dry, false);
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm, boards.personal.answer("share.to", &dry, true));
}

/// A decision's type, whose `project` and `about` are references.
const DECISION_SCHEMA: &str = ".board/types/lr/decision/v1.schema.json";
const DECISION_TYPE: &str = r#"{"type": "object", "properties": {"project": {"type": "string", "format": "board-ref"}, "about": {"type": "string", "format": "board-ref"}, "text": {"type": "string"}}}"#;
/// What sharing the project copies: the project, the decision and the
/// question whose `project` names it, and the note the decision is about.
const PROJECT: [&str; 4] = [
    "/notes/why.md",
    "/projects/p/decisions/a",
    "/projects/p/questions/b",
    "/tasks/p.md",
];

fn decision(fields: &Value) -> String {
    record(json!({"$type": "lr/decision/v1"}), fields)
}

impl Boards {
    /// Writes the project `/tasks/p.md` with `body`, the decision
    /// `/projects/p/decisions/a` and the question `/projects/p/questions/b`
    /// whose `project` names it, and `/notes/why.md`, which the decision is
    /// `about`.
    fn project(&self, body: &str) -> &Self {
        let a = json!({"project": "/tasks/p.md", "about": "/notes/why.md"});
        self.personal
            .write(TASK_SCHEMA, TASK_TYPE)
            .write(DECISION_SCHEMA, DECISION_TYPE)
            .write(
                "tasks/p.md",
                &format!("---\n$type: lr/task/v1\n---\n# Project\n\n{body}"),
            )
            .write("projects/p/decisions/a.json", &decision(&a))
            .write(
                "projects/p/questions/b.json",
                &decision(&json!({"project": "/tasks/p.md"})),
            )
            .write("notes/why.md", "# Why\n");
        self
    }

    /// The project, whose body links to the directory `/projects` and to
    /// `/team/notes/team.md` through the mount.
    fn uncopied(&self) -> &Self {
        self.publish(&[("notes/team.md", "# Team\n")]);
        self.project("See [projects](/projects) and [team](/team/notes/team.md).\n")
    }
}

/// What the preview of [`Boards::uncopied`] lists as not copied.
fn not_copied() -> Value {
    json!([
        {"path": "/projects", "reason": "directory", "from": "/tasks/p.md"},
        {"path": "/team/notes/team.md", "reason": "mount", "from": "/tasks/p.md"},
    ])
}

#[test]
fn a_projects_decision_and_question_that_name_it_by_typed_ref_are_copied_and_records_naming_it_in_prose_or_a_link_are_not()
 {
    let boards = boards();
    boards.project("Decided in /projects/p/decisions/c.\n");
    boards
        .personal
        .write(
            "projects/p/decisions/c.json",
            &decision(&json!({"text": "For /tasks/p.md: keep one file."})),
        )
        .write("notes/mentions.md", "# Mentions\n\nSee [p](/tasks/p.md).\n");
    let preview = boards.preview(&["--path", "/tasks/p.md"]);
    assert_eq!(
        (&preview["extras"], &preview["extras_total"]),
        (
            &json!([
                {"path": "/notes/why.md", "via": "ref", "from": "/projects/p/decisions/a"},
                {"path": "/projects/p/decisions/a", "via": "back_ref", "from": "/tasks/p.md"},
                {"path": "/projects/p/questions/b", "via": "back_ref", "from": "/tasks/p.md"},
            ]),
            &json!(3)
        )
    );
    let output = boards.share(&["--path", "/tasks/p.md"]);
    assert_eq!(
        (&output["copied"], &output["types"]),
        (&json!(PROJECT), &json!(["lr/decision/v1", "lr/task/v1"]))
    );
    let clone = Scenario::clone_of(&boards.team);
    for path in &PROJECT[1..3] {
        let got = board_in(clone.workdir(), &["--json", "get", path]);
        assert_eq!(got.code, 0, "{path}: {}", text(&got.stdout));
    }
    assert!(!boards.exists("projects/p/decisions/c.json"));
    assert!(!boards.exists("notes/mentions.md"));
}

#[test]
fn a_back_ref_records_own_reference_is_followed_and_a_record_that_names_only_the_back_ref_is_not_copied()
 {
    let boards = boards();
    boards.project("");
    boards.personal.write(
        "projects/p/decisions/d.json",
        &decision(&json!({"about": "/projects/p/decisions/a"})),
    );
    let output = boards.share(&["--path", "/tasks/p.md"]);
    assert_eq!(output["copied"], json!(PROJECT));
    assert!(!boards.exists("projects/p/decisions/d.json"));
}

#[test]
fn a_link_to_a_directory_and_one_through_the_mount_are_listed_as_not_copied_and_next_allows_them() {
    let boards = boards();
    boards.uncopied();
    let before = boards.head().id;
    let preview = boards.preview(&["--path", "/tasks/p.md"]);
    assert_eq!(
        (
            &preview["copied"],
            &preview["not_copied"],
            &preview["not_copied_total"],
            &preview["next"]
        ),
        (
            &json!(PROJECT),
            &not_copied(),
            &json!(2),
            &json!(boards.next("share to --path /tasks/p.md --to /team --yes --allow-uncopied"))
        )
    );
    assert_eq!(boards.head().id, before);
}

#[test]
fn a_link_through_a_link_in_this_board_and_one_to_nothing_are_listed_with_reasons_link_and_missing()
{
    let boards = boards();
    boards.project("See [why](/latest/why.md) and [gone](/notes/gone.md).\n");
    boards.personal.write(
        "latest.json",
        r#"{"$type": "board/mount/v1", "path": "/notes"}"#,
    );
    let preview = boards.preview(&["--path", "/tasks/p.md"]);
    assert_eq!(
        (&preview["copied"], &preview["not_copied"]),
        (
            &json!(PROJECT),
            &json!([
                {"path": "/latest/why.md", "reason": "link", "from": "/tasks/p.md"},
                {"path": "/notes/gone.md", "reason": "missing", "from": "/tasks/p.md"},
            ])
        )
    );
}

/// The board's copies of the tasks and plans plugins' schemas.
const TASKS_SCHEMA: &str = ".board/types/tasks/task/v2.schema.json";
const DECISIONS_SCHEMA: &str = ".board/types/plans/decision/v1.schema.json";
/// A change record's type, whose `task` names a task and whose `paths`
/// name what it edits.
const CHANGE_SCHEMA: &str = ".board/types/lr/change/v1.schema.json";
const CHANGE_TYPE: &str = r#"{"type": "object", "properties": {"task": {"type": "string", "format": "board-ref"}, "paths": {"type": "array", "items": {"type": "string", "format": "board-ref"}}}}"#;
/// What sharing `/tasks/remove.md` copies from [`Boards::reading_list`].
const READING: [&str; 4] = [
    "/projects/tidy/decisions/d",
    "/projects/tidy/questions/q",
    "/tasks/remove.md",
    "/tasks/tidy.md",
];

/// A `tasks/task/v2` task titled `title`, with `fields` in its frontmatter.
fn task(title: &str, fields: &str) -> String {
    format!("---\n$type: tasks/task/v2\ntitle: {title}\nstatus: open\n{fields}---\n# {title}\n")
}

/// A `plans/decision/v1` whose `refs` names each of `refs`.
fn plan_decision(refs: &[&str]) -> String {
    json!({
        "$type": "plans/decision/v1",
        "decision": "Keep one list.",
        "why": "Two lists drift.",
        "by": {"person": "ana"},
        "decided": "2026-11-20T09:00:00Z",
        "refs": refs,
    })
    .to_string()
}

/// A `plans/question/v1` that `blocks` the work at `blocks`.
fn plan_question(blocks: &str) -> String {
    json!({
        "$type": "plans/question/v1",
        "question": "Which list stays?",
        "by": {"person": "ana"},
        "asked": "2026-11-20T09:00:00Z",
        "status": "open",
        "blocks": blocks,
    })
    .to_string()
}

/// A change record whose `task` names `task` and whose `paths` names each
/// of `paths`.
fn change(task: &str, paths: &[&str]) -> String {
    json!({"$type": "lr/change/v1", "task": task, "paths": paths}).to_string()
}

/// The source board also enables tasks and plans.
fn tasks_boards() -> Boards {
    let boards = boards();
    for plugin in ["tasks", "plans"] {
        let enabled = boards.personal.run(&["plugin", "enable", plugin]);
        assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
    }
    boards
}

/// A repository holding `a.md`, which the source board mounts at
/// `/repos/library` and the team board does not, and its pointer.
fn library_mount(boards: &Boards) -> (Scenario, String) {
    let library = Scenario::new();
    let commit = library.commit(&[("a.md", "# A\n")]);
    library.track(commit);
    let pointer = mount_of(&library.path(), commit);
    boards.personal.write("repos/library.json", &pointer);
    (library, pointer)
}

impl Boards {
    /// Writes the project `/tasks/tidy.md`, its tasks `/tasks/remove.md`
    /// and `/tasks/other.md` whose `parent` names it, a decision whose
    /// `refs` names it and a question that `blocks` it.
    fn reading_list(&self) -> &Self {
        let parent = "parent: /tasks/tidy.md\n";
        self.personal
            .write(CHANGE_SCHEMA, CHANGE_TYPE)
            .write("tasks/tidy.md", &task("Tidy", "kind: project\n"))
            .write("tasks/remove.md", &task("Remove", parent))
            .write("tasks/other.md", &task("Other", parent))
            .write(
                "projects/tidy/decisions/d.json",
                &plan_decision(&["/tasks/tidy.md"]),
            )
            .write(
                "projects/tidy/questions/q.json",
                &plan_question("/tasks/tidy.md"),
            );
        self
    }
}

#[test]
fn a_shared_task_brings_its_projects_decision_and_question_and_not_its_sibling_task() {
    let boards = tasks_boards();
    boards.reading_list();
    let preview = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        (
            &preview["extras"],
            &preview["wholes"],
            &preview["wholes_total"],
            &preview["not_copied"]
        ),
        (
            &json!([
                {"path": "/projects/tidy/decisions/d", "via": "back_ref", "from": "/tasks/tidy.md"},
                {"path": "/projects/tidy/questions/q", "via": "back_ref", "from": "/tasks/tidy.md"},
                {"path": "/tasks/tidy.md", "via": "ref", "from": "/tasks/remove.md"},
            ]),
            &json!([{"path": "/tasks/tidy.md", "from": "/tasks/remove.md", "back_refs": 2}]),
            &json!(1),
            &json!([])
        )
    );
    let output = boards.share(&["--path", "/tasks/remove.md"]);
    assert_eq!(output["copied"], json!(READING));
    assert!(!boards.exists("tasks/other.md"));
    let clone = Scenario::clone_of(&boards.team);
    let checked = board_in(clone.workdir(), &["check"]);
    assert_eq!(checked.code, 0, "{}", text(&checked.stdout));
}

#[test]
fn a_shared_v3_task_whose_parent_is_a_v2_project_brings_the_projects_decision_and_question() {
    let boards = tasks_boards();
    boards.reading_list();
    let v3 = task("Remove", "parent: /tasks/tidy.md\n").replace("tasks/task/v2", "tasks/task/v3");
    boards.personal.write("tasks/remove.md", &v3);
    let preview = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        preview["wholes"],
        json!([{"path": "/tasks/tidy.md", "from": "/tasks/remove.md", "back_refs": 2}])
    );
    let output = boards.share(&["--path", "/tasks/remove.md"]);
    assert_eq!(output["copied"], json!(READING));
    let clone = Scenario::clone_of(&boards.team);
    let checked = board_in(clone.workdir(), &["check"]);
    assert_eq!(checked.code, 0, "{}", text(&checked.stdout));
}

#[test]
fn a_shared_task_brings_no_change_record_of_its_project_however_many_name_it() {
    let boards = tasks_boards();
    boards.reading_list();
    let _library = library_mount(&boards);
    for number in 0..300 {
        boards.personal.write(
            &format!("changes/c{number:03}.json"),
            &change(
                "/tasks/tidy.md",
                &["/repos/library/a.md", "/repos/library/b.md"],
            ),
        );
    }
    let started = std::time::Instant::now();
    let preview = boards.preview(&["--path", "/tasks/remove.md"]);
    let took = started.elapsed();
    assert_eq!(
        (
            &preview["copied"],
            &preview["extras_total"],
            &preview["not_copied"]
        ),
        (&json!(READING), &json!(3), &json!([]))
    );
    assert!(took < std::time::Duration::from_secs(10), "{took:?}");
}

#[test]
fn a_decisions_reference_to_a_change_and_through_a_mount_is_listed_and_neither_is_copied_nor_refused()
 {
    let boards = tasks_boards();
    boards.reading_list();
    let _library = library_mount(&boards);
    boards
        .personal
        .write(
            "changes/c.json",
            &change("/tasks/tidy.md", &["/repos/library/a.md"]),
        )
        .write(
            "projects/tidy/decisions/d.json",
            &plan_decision(&["/tasks/tidy.md", "/changes/c", "/repos/library/a.md"]),
        );
    let call = ["--path", "/tasks/remove.md"];
    let preview = boards.preview(&call);
    let decision = "/projects/tidy/decisions/d";
    assert_eq!(
        (&preview["copied"], &preview["not_copied"], &preview["next"]),
        (
            &json!(READING),
            &json!([
                {"path": "/changes/c", "reason": "beyond", "from": decision},
                {"path": "/repos/library/a.md", "reason": "mount", "from": decision},
            ]),
            &json!(
                boards.next("share to --path /tasks/remove.md --to /team --yes --allow-uncopied")
            )
        )
    );
    let refused = boards.refused(&call);
    assert_eq!(
        (&refused["code"], &refused["fields"]["paths"]),
        (
            &json!("share::closure::uncopied"),
            &strings(&["/changes/c", "/repos/library/a.md"])
        )
    );
    let output = boards.share(&["--path", "/tasks/remove.md", "--allow-uncopied"]);
    assert_eq!(output["copied"], json!(READING));
    assert!(!boards.exists("changes/c.json"));
}

#[test]
fn a_shared_task_brings_the_decisions_of_every_project_above_it() {
    let boards = tasks_boards();
    boards.reading_list();
    boards
        .personal
        .write("tasks/root.md", &task("Root", "kind: project\n"))
        .write(
            "tasks/tidy.md",
            &task("Tidy", "kind: project\nparent: /tasks/root.md\n"),
        )
        .write(
            "projects/root/decisions/e.json",
            &plan_decision(&["/tasks/root.md"]),
        );
    let preview = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        (&preview["copied"], &preview["wholes"]),
        (
            &json!([
                "/projects/root/decisions/e",
                "/projects/tidy/decisions/d",
                "/projects/tidy/questions/q",
                "/tasks/remove.md",
                "/tasks/root.md",
                "/tasks/tidy.md",
            ]),
            &json!([
                {"path": "/tasks/root.md", "from": "/tasks/tidy.md", "back_refs": 1},
                {"path": "/tasks/tidy.md", "from": "/tasks/remove.md", "back_refs": 2},
            ])
        )
    );
}

#[test]
fn a_cycle_of_parents_ends_with_each_whole_found_once() {
    let boards = tasks_boards();
    boards.reading_list();
    boards
        .personal
        .write(
            "tasks/root.md",
            &task("Root", "kind: project\nparent: /tasks/tidy.md\n"),
        )
        .write(
            "tasks/tidy.md",
            &task("Tidy", "kind: project\nparent: /tasks/root.md\n"),
        );
    let preview = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        (&preview["copied"], &preview["wholes"]),
        (
            &json!([
                "/projects/tidy/decisions/d",
                "/projects/tidy/questions/q",
                "/tasks/remove.md",
                "/tasks/root.md",
                "/tasks/tidy.md",
            ]),
            &json!([
                {"path": "/tasks/root.md", "from": "/tasks/tidy.md", "back_refs": 0},
                {"path": "/tasks/tidy.md", "from": "/tasks/remove.md", "back_refs": 2},
            ])
        )
    );
}

#[test]
fn a_shared_project_brings_its_tasks_and_change_records_and_a_shared_eval_no_other_eval_or_run_of_its_suite()
 {
    let boards = tasks_boards();
    boards.reading_list().standard();
    boards
        .personal
        .write(
            "changes/c.json",
            &change("/tasks/tidy.md", &["/notes/n.md"]),
        )
        .write("notes/n.md", "# N\n")
        .write("evals/y.json", &record(eval(), &json!({"runs": []})))
        .write(
            "suites/y/runs/1.json",
            &record(
                run_record(),
                &json!({"results": [{"case": "cold", "outcome": "pass"}]}),
            ),
        );
    let project = boards.preview(&["--path", "/tasks/tidy.md"]);
    assert_eq!(
        (&project["copied"], &project["wholes"]),
        (
            &json!([
                "/changes/c",
                "/notes/n.md",
                "/projects/tidy/decisions/d",
                "/projects/tidy/questions/q",
                "/tasks/other.md",
                "/tasks/remove.md",
                "/tasks/tidy.md",
            ]),
            &json!([])
        )
    );
    let eval = boards.preview(&["--path", "/evals/x"]);
    assert_eq!(
        (&eval["copied"], &eval["wholes"]),
        (&json!(SHARED), &json!([]))
    );
}

#[test]
fn a_project_a_task_waits_on_brings_none_of_its_records_and_one_it_is_part_of_does() {
    let boards = tasks_boards();
    boards.reading_list();
    boards.personal.write(
        "tasks/remove.md",
        &task("Remove", "after: [/tasks/tidy.md]\n"),
    );
    let output = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        (
            &output["copied"],
            &output["wholes"],
            &output["wholes_total"]
        ),
        (
            &json!(["/tasks/remove.md", "/tasks/tidy.md"]),
            &json!([]),
            &json!(0)
        )
    );
    boards.personal.write(
        "tasks/remove.md",
        &task("Remove", "parent: /tasks/tidy.md\n"),
    );
    let output = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(output["wholes_total"], json!(1));
}

/// The board's copy of the schema at `at`, without `keyword` at `pointer`.
fn unmark(boards: &Boards, at: &str, pointer: &str, keyword: &str) {
    let mut schema: Value = serde_json::from_str(&boards.personal.read(at)).unwrap();
    (schema
        .pointer_mut(pointer)
        .unwrap()
        .as_object_mut()
        .unwrap())
    .remove(keyword)
    .expect("the plugin's schema carries the keyword");
    boards.personal.write(at, &schema.to_string());
}

#[test]
fn a_task_schema_without_the_part_of_keyword_shares_a_task_and_its_project_alone() {
    let boards = tasks_boards();
    boards.reading_list();
    unmark(
        &boards,
        TASKS_SCHEMA,
        "/properties/parent",
        "x-board-part-of",
    );
    let output = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        (&output["copied"], &output["wholes"]),
        (&json!(["/tasks/remove.md", "/tasks/tidy.md"]), &json!([]))
    );
}

#[test]
fn a_decision_schema_without_the_goes_with_whole_keyword_leaves_the_decision_out() {
    let boards = tasks_boards();
    boards.reading_list();
    unmark(&boards, DECISIONS_SCHEMA, "", "x-board-goes-with-whole");
    let output = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        (&output["copied"], &output["wholes"]),
        (
            &json!([
                "/projects/tidy/questions/q",
                "/tasks/remove.md",
                "/tasks/tidy.md"
            ]),
            &json!([{"path": "/tasks/tidy.md", "from": "/tasks/remove.md", "back_refs": 1}])
        )
    );
}

#[test]
fn a_board_without_the_decision_schema_shares_the_rest_of_the_project() {
    let boards = tasks_boards();
    boards.reading_list();
    boards.personal.board.write(DECISIONS_SCHEMA, Item::Deleted);
    let output = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        output["copied"],
        json!([
            "/projects/tidy/questions/q",
            "/tasks/remove.md",
            "/tasks/tidy.md"
        ])
    );
}

#[test]
fn a_wholes_own_reference_through_a_mount_the_team_lacks_is_refused_and_one_it_mounts_is_kept() {
    let boards = tasks_boards();
    boards.reading_list();
    let (_library, pointer) = library_mount(&boards);
    boards.personal.write(
        "tasks/tidy.md",
        &task("Tidy", "kind: project\nbrief: /repos/library/a.md\n"),
    );
    let call = ["--path", "/tasks/remove.md", "--allow-uncopied"];
    let refused = boards.refused_preview(&call);
    assert_eq!(
        (&refused["code"], &refused["fields"]["paths"]),
        (
            &json!("share::closure::unresolved"),
            &strings(&["/repos/library/a.md"])
        )
    );
    boards.publish(&[("repos/library.json", &pointer)]);
    assert_eq!(boards.preview(&call)["copied"], json!(READING));
}

#[test]
fn a_decisions_reference_through_a_mount_the_team_also_mounts_is_not_listed() {
    let boards = tasks_boards();
    boards.reading_list();
    let (_library, pointer) = library_mount(&boards);
    boards.personal.write(
        "projects/tidy/decisions/d.json",
        &plan_decision(&["/tasks/tidy.md", "/repos/library/a.md"]),
    );
    boards.publish(&[("repos/library.json", &pointer)]);
    let call = ["--path", "/tasks/remove.md"];
    let preview = boards.preview(&call);
    assert_eq!(
        (&preview["copied"], &preview["not_copied"]),
        (&json!(READING), &json!([]))
    );
    assert_eq!(boards.share(&call)["copied"], json!(READING));
}

#[test]
fn a_path_through_a_mount_that_a_back_ref_and_a_decision_both_name_is_listed_once_from_the_back_ref()
 {
    let boards = tasks_boards();
    boards.reading_list();
    let _library = library_mount(&boards);
    let mounted = "/repos/library/a.md";
    boards
        .personal
        .write(
            "projects/tidy/decisions/d.json",
            &plan_decision(&["/tasks/tidy.md", mounted]),
        )
        .write(
            "projects/tidy/decisions/e.json",
            &plan_decision(&["/tasks/remove.md", mounted]),
        );
    let preview = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        (&preview["not_copied"], &preview["not_copied_total"]),
        (
            &json!([{"path": mounted, "reason": "mount", "from": "/projects/tidy/decisions/e"}]),
            &json!(1)
        )
    );
}

#[test]
fn a_path_a_link_and_a_back_ref_both_name_through_a_mount_the_team_lacks_is_listed_once() {
    let boards = tasks_boards();
    boards.reading_list();
    let _library = library_mount(&boards);
    let mounted = "/repos/library/a.md";
    boards
        .personal
        .write(
            "tasks/remove.md",
            &format!(
                "---\n$type: tasks/task/v2\ntitle: Remove\nstatus: open\nparent: /tasks/tidy.md\n---\nSee [a]({mounted}).\n"
            ),
        )
        .write(
            "projects/tidy/decisions/e.json",
            &plan_decision(&["/tasks/remove.md", mounted]),
        );
    let preview = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        (&preview["not_copied"], &preview["not_copied_total"]),
        (
            &json!([{"path": mounted, "reason": "mount", "from": "/tasks/remove.md"}]),
            &json!(1)
        )
    );
}

#[test]
fn a_decisions_reference_through_a_link_in_this_board_is_listed_as_link_and_through_a_mount_as_mount()
 {
    let boards = tasks_boards();
    boards.reading_list();
    let _library = library_mount(&boards);
    boards
        .personal
        .write("notes/why.md", "# Why\n")
        .write(
            "latest.json",
            r#"{"$type": "board/mount/v1", "path": "/notes"}"#,
        )
        .write(
            "projects/tidy/decisions/d.json",
            &plan_decision(&["/tasks/tidy.md", "/latest/why.md", "/repos/library/a.md"]),
        );
    let preview = boards.preview(&["--path", "/tasks/remove.md"]);
    let decision = "/projects/tidy/decisions/d";
    assert_eq!(
        preview["not_copied"],
        json!([
            {"path": "/latest/why.md", "reason": "link", "from": decision},
            {"path": "/repos/library/a.md", "reason": "mount", "from": decision},
        ])
    );
}

#[test]
fn a_back_refs_reference_through_a_link_in_this_board_is_listed_as_link_from_the_back_ref() {
    let boards = tasks_boards();
    boards.reading_list();
    boards
        .personal
        .write("notes/why.md", "# Why\n")
        .write(
            "latest.json",
            r#"{"$type": "board/mount/v1", "path": "/notes"}"#,
        )
        .write(
            "projects/tidy/decisions/e.json",
            &plan_decision(&["/tasks/remove.md", "/latest/why.md"]),
        );
    let preview = boards.preview(&["--path", "/tasks/remove.md"]);
    assert_eq!(
        (&preview["not_copied"], &preview["not_copied_total"]),
        (
            &json!([{"path": "/latest/why.md", "reason": "link", "from": "/projects/tidy/decisions/e"}]),
            &json!(1)
        )
    );
}

const PULL_SCHEMA: &str = ".board/types/lr/pull/v1.schema.json";
const PULL_TYPE: &str = r#"{"type": "object", "properties": {"repository": {"type": "string", "format": "board-ref", "x-board-type": "board/repo/v2"}}}"#;
const REPOSITORY: &str = r#"{"$type": "board/repo/v2", "remote": "crumbkit/board"}"#;

#[test]
fn a_shared_record_brings_the_repository_record_it_names_and_one_the_team_holds_already_is_unchanged()
 {
    let boards = boards();
    let pull = r#"{"$type": "lr/pull/v1", "repository": "/repositories/crumbkit/board"}"#;
    boards
        .personal
        .write(PULL_SCHEMA, PULL_TYPE)
        .write("repositories/crumbkit/board.json", REPOSITORY)
        .write("pulls/a.json", pull)
        .write("pulls/b.json", pull);
    let output = boards.share(&["--path", "/pulls/a"]);
    assert_eq!(
        (&output["copied"], &output["types"]),
        (
            &json!(["/pulls/a", "/repositories/crumbkit/board"]),
            &json!(["lr/pull/v1"])
        )
    );
    assert!(!boards.exists(".board/types/board"));
    let before = boards.head();
    let output = boards.share(&["--path", "/pulls/b"]);
    assert_eq!(
        (&output["copied"], &output["unchanged"]),
        (&json!(["/pulls/b"]), &json!(1))
    );
    assert_eq!(boards.changed(&before), ["pulls/b.json"]);
}

#[test]
fn a_shared_task_leaves_out_the_priorities_record_that_covers_it_and_brings_a_decision_naming_it() {
    let boards = tasks_boards();
    boards.reading_list();
    let enabled = boards.personal.run(&["plugin", "enable", "priorities"]);
    assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
    let entry =
        json!({"id": "tidy", "title": "Tidy the reading list", "covers": ["/tasks/remove.md"]});
    boards
        .personal
        .write(
            "priorities.json",
            &json!({"$type": "priorities/list/v1", "short": [entry], "long": []}).to_string(),
        )
        .write(
            "projects/tidy/decisions/e.json",
            &plan_decision(&["/tasks/remove.md"]),
        );
    let call = ["--path", "/tasks/remove.md", "--allow-uncopied"];
    let preview = boards.preview(&call);
    let mut copied = READING.to_vec();
    copied.insert(1, "/projects/tidy/decisions/e");
    assert_eq!(
        (&preview["copied"], &preview["not_copied"]),
        (
            &json!(copied),
            &json!([{"path": "/priorities", "reason": "withheld", "from": "/tasks/remove.md"}])
        )
    );
    assert_eq!(boards.share(&call)["copied"], json!(copied));
    assert!(!boards.exists("priorities.json"));
}
