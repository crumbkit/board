//! Writing a flow: a change on the board itself (`--mount /`), the
//! `flow-valid` and `exists` check kinds, a flow named by its path, and
//! `changes.run --sandbox`, which dry-runs a stand-in agent in a throwaway
//! copy of the board and the repository. No test starts a real agent.

use super::*;
use crate::tests::help::binary;

const AUTHOR: &str = "/changes/author";
/// The draft the authoring change writes: one phase, `draft`, whose only
/// check waits on the owner, and a stop through it.
fn draft() -> Value {
    json!({
        "$type": "changes/flow/v1",
        "name": "trial",
        "summary": "A flow being written.",
        "stop": {"through": "draft"},
        "phases": [{"name": "draft", "agent": "lead", "instruction": "Draft {change}.",
            "checks": [{"id": "approved", "kind": "records", "type": "plans/decision/v1",
                        "actor": "owner", "where": {"owner_signed": true},
                        "remedy": "Ask the owner to approve {change}."}]}]
    })
}

/// The input of `start` for a change titled `title` on the board itself,
/// with `fields`.
fn on_board(title: &str, fields: &Value) -> Value {
    let input = json!({"mount": "/", "task": TASK, "title": title});
    serde_json::from_str(&record(input, fields)).unwrap()
}

/// The fixture board with /repos/x mounted and the change /changes/author
/// on the flow-authoring flow, holding the draft at /changes/author/draft.
fn authoring() -> Changes {
    let board = Changes::with_tasks();
    board.mount_x(&[("README.md", "x\n")]);
    board.call(
        "start",
        &on_board("Author", &json!({"flow": "flow-authoring"})),
    );
    board.write("changes/author/draft.json", &draft().to_string());
    board
}

#[test]
fn a_change_on_the_board_itself_reads_no_repository_and_records_no_mount() {
    let board = Changes::new();
    let output = board.call(
        "start",
        &on_board("Author", &json!({"flow": "flow-authoring"})),
    );
    assert_eq!(
        (
            &output["phase"],
            &output["codeowners"],
            &output["suite"],
            &output["route"]
        ),
        (
            &json!("interview"),
            &Value::Null,
            &Value::Null,
            &json!("unowned")
        )
    );
    assert!(output["note"].as_str().unwrap().contains("no repository"));
    let change = board.read("changes/author.json");
    assert!(change.get("mount").is_none(), "{change}");
    assert_eq!(board.code(AUTHOR), None);
    let next = board.call("next", &json!({"path": AUTHOR}));
    assert_eq!(
        next["flow"]["phases"],
        json!(["interview", "draft", "dry-run", "adopt"])
    );
}

#[test]
fn the_board_itself_is_refused_for_a_flow_that_reads_a_repository_and_admitted_for_one_that_does_not()
 {
    let board = Changes::new();
    let refused = board.refusal("start", &on_board("Author", &json!({"flow": "small"})));
    assert_eq!(refused["code"], "changes::mount::needs_repository");
    assert_eq!(
        refused["fields"]["checks"],
        json!(["suite-run", "not-small", "lifecycle-paths"])
    );
    board.call(
        "start",
        &on_board("Author", &json!({"flow": "flow-authoring"})),
    );
}

#[test]
fn start_refuses_a_board_flow_that_cannot_run_and_admits_its_fixed_twin() {
    let board = Changes::new();
    let mut flow = draft();
    flow["phases"][0]["instruction"] = json!("Draft {branch}.");
    board.write("flows/trial.json", &flow.to_string());
    let refused = board.refusal("start", &on_board("A", &json!({"flow": "trial"})));
    assert_eq!(refused["code"], "changes::flow::invalid");
    assert_eq!(
        refused["fields"]["problems"],
        json!([
            "phase draft, the instruction names the unknown placeholder {branch} \
             (changes::flow::unknown_placeholder)"
        ])
    );
    assert_eq!(
        refused["fields"]["codes"],
        json!(["changes::flow::unknown_placeholder"])
    );
    board.write("flows/trial.json", &draft().to_string());
    board.call("start", &on_board("A", &json!({"flow": "trial"})));
}

#[test]
fn start_follows_a_flow_named_by_its_path_and_refuses_a_path_that_holds_none() {
    let board = authoring();
    let refused = board.refusal(
        "start",
        &on_board("A", &json!({"flow": "/changes/author/none"})),
    );
    assert_eq!(refused["code"], "changes::flow::not_found");
    let output = board.call(
        "start",
        &on_board("A", &json!({"flow": "/changes/author/draft"})),
    );
    assert_eq!(output["flow"]["name"], "trial");
}

/// The ids and statuses of the checks `next` lists for `path`, with each
/// detail.
fn gate_of(board: &Changes, path: &str) -> Vec<(String, String, String)> {
    let output = board.call("next", &json!({"path": path}));
    (output["gate"].as_array().unwrap().iter())
        .map(|check| {
            (
                check["check"].as_str().unwrap().to_owned(),
                check["status"].as_str().unwrap().to_owned(),
                check["detail"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// Moves the change at `/changes/<name>` to `phase` by rewriting it.
fn move_to(board: &Changes, name: &str, phase: &str) {
    let mut change = board.read(&format!("changes/{name}.json"));
    change["phase"] = json!(phase);
    board.write(&format!("changes/{name}.json"), &change.to_string());
}

#[test]
fn flow_valid_passes_an_attached_draft_without_problems_and_exists_once_the_flow_is_installed() {
    let board = Changes::new();
    let checks = json!({
        "$type": "changes/flow/v1", "name": "checking", "summary": "Two checks.",
        "stop": {"until": "done"},
        "phases": [{"name": "draft", "agent": "lead", "instruction": "Draft.", "checks": [
            {"id": "valid", "kind": "flow-valid"},
            {"id": "installed", "kind": "exists", "path": "/flows/{draft}"}]}]
    });
    board.write("flows/checking.json", &checks.to_string());
    board.call("start", &on_board("Author", &json!({"flow": "checking"})));
    let missing = gate_of(&board, AUTHOR);
    assert_eq!(
        missing,
        [
            (
                "valid".to_owned(),
                "missing".to_owned(),
                "no changes/flow/v1 draft is attached".to_owned()
            ),
            (
                "installed".to_owned(),
                "missing".to_owned(),
                "the path /flows/{draft} names {draft}, which has no value yet, so where the \
                 record belongs is not known"
                    .to_owned()
            ),
        ]
    );
    let mut unasked = draft();
    unasked["phases"][0]["checks"][0]
        .as_object_mut()
        .unwrap()
        .remove("remedy");
    board.write("changes/author/draft.json", &unasked.to_string());
    board.call(
        "attach",
        &json!({"path": AUTHOR, "ref": "/changes/author/draft"}),
    );
    let found = gate_of(&board, AUTHOR);
    assert_eq!(
        found[0].2,
        "/changes/author/draft: phase draft, check approved: only the owner or a reviewer can \
         pass it, and it has no remedy that tells an unattended agent how to ask \
         (changes::flow::no_remedy)"
    );
    assert_eq!(found[1].2, "nothing is at /flows/trial");
    let remedy = &board.call("next", &json!({"path": AUTHOR}))["gate"][1]["remedy"];
    assert_eq!(remedy, "Write the record at /flows/trial with `board put`.");
    board.write("changes/author/draft.json", &draft().to_string());
    board.write("flows/trial.json", &draft().to_string());
    let passing: Vec<&str> = (gate_of(&board, AUTHOR).iter())
        .map(|(_, status, _)| if status == "pass" { "pass" } else { "missing" })
        .collect();
    assert_eq!(passing, ["pass", "pass"]);
}

#[test]
fn the_adopt_check_needs_one_owner_decision_whose_refs_name_both_the_draft_and_the_eval() {
    let board = authoring();
    board.write(
        ".board/types/evals/eval/v1.schema.json",
        r#"{"type": "object"}"#,
    );
    board.write("evals/trial.json", &typed("evals/eval/v1", &json!({})));
    for each in ["/changes/author/draft", "/evals/trial"] {
        board.call("attach", &json!({"path": AUTHOR, "ref": each}));
    }
    move_to(&board, "author", "adopt");
    board.write(
        "changes/author/adopted.json",
        &typed(
            "plans/decision/v1",
            &json!({"refs": ["/changes/author/draft"]}),
        ),
    );
    board.call(
        "attach",
        &json!({"path": AUTHOR, "ref": "/changes/author/adopted"}),
    );
    assert_eq!(gate_of(&board, AUTHOR)[0].1, "missing");
    board.write(
        "changes/author/adopted.json",
        &typed(
            "plans/decision/v1",
            &json!({"refs": ["/changes/author/draft", "/evals/trial"]}),
        ),
    );
    let gate = gate_of(&board, AUTHOR);
    assert_eq!(
        (gate[0].1.as_str(), gate[1].2.as_str()),
        ("pass", "nothing is at /flows/trial")
    );
}

/// The stand-in agent: it keeps its environment, directory and `HOME` in
/// the directory its first argument names, then tries to write to the
/// board from its directory and to the personal board with the `board`
/// its second argument names, and commits in its directory.
const ESCAPING: &str = r#"out="$1"; board="$2"
pwd -P > "$out/pwd"
printf '%s' "$HOME" > "$out/home"
echo escaped | "$board" put --file /notes/escape.md > "$out/put" 2>&1
echo personal | "$board" --personal put --file /notes/personal.md >> "$out/put" 2>&1
echo agent > agent.txt
git add agent.txt && git commit -qm agent >> "$out/put" 2>&1
exit 0
"#;

/// The stand-in agent's argv, writing into `out`.
fn escaping(board: &Changes, out: &Path) -> String {
    let script = board.root().join("escaping.sh");
    write_file(&script, ESCAPING);
    json!(["/bin/sh", script, out, binary()]).to_string()
}

/// `run` with `given` as flags and the stand-in agent writing into `out`.
fn sandboxed(board: &Changes, given: &Value, out: &Path) -> Result<Value, Value> {
    let argv = escaping(board, out);
    board.invoke("run", given, false, &[("BOARD_AGENT", argv.as_str())])
}

/// A sandboxed run of the draft on /repos/x, with `fields`.
fn dry_run(fields: &Value) -> Value {
    let input = json!({"sandbox": true, "flow": "/changes/author/draft", "mount": "/repos/x",
                       "title": "Sample"});
    serde_json::from_str(&record(input, fields)).unwrap()
}

#[test]
fn a_sandboxed_run_leaves_the_real_board_and_repository_unchanged_and_copies_its_records_back() {
    let board = authoring();
    let repo = board.repo_root("repos/x");
    let branches = git(&repo, &["branch", "--list"]);
    let out = tempfile::tempdir().unwrap();
    let output = sandboxed(&board, &dry_run(&json!({"budget": 1})), out.path()).unwrap();
    let put = std::fs::read_to_string(out.path().join("put")).unwrap();
    let sandbox = PathBuf::from(output["sandbox"].as_str().unwrap());
    assert_eq!(
        (&output["path"], &output["reason"]),
        (&json!("/changes/sample"), &json!("waiting"))
    );
    // The agent's writes landed in the copies: from its directory, in the
    // worktree of the copied repository, which is a board too, and to its
    // personal board, in the copied board. None reached the real board,
    // repository or home.
    assert!(
        sandbox
            .join("repo.worktrees/sample/notes/escape.md")
            .exists(),
        "{put}"
    );
    assert!(!repo.join("notes/escape.md").exists());
    assert!(sandbox.join("board/notes/personal.md").exists(), "{put}");
    assert!(sandbox.join("board/changes/sample.json").exists());
    for absent in [
        "notes/escape.md",
        "notes/personal.md",
        "changes/sample.json",
    ] {
        assert!(!board.exists(absent), "{absent}");
    }
    assert_eq!(git(&repo, &["branch", "--list"]), branches);
    assert!(!PathBuf::from(format!("{}.worktrees", repo.display())).exists());
    let ran_in = std::fs::read_to_string(out.path().join("pwd")).unwrap();
    assert!(
        Path::new(ran_in.trim()).starts_with(sandbox.canonicalize().unwrap()),
        "{ran_in}"
    );
    let home = std::fs::read_to_string(out.path().join("home")).unwrap();
    assert!(Path::new(&home).starts_with(&sandbox), "{home}");
    let worktree = sandbox.join("repo.worktrees/sample");
    assert_eq!(git(&worktree, &["log", "-1", "--format=%s"]), "agent");
    // The run and its stop are copied under the asking change, and the run
    // is attached to it.
    let run = output["run"].as_str().unwrap();
    assert!(run.starts_with("/changes/author/runs/"), "{run}");
    let written = board.read(&format!("{}.json", run.trim_start_matches('/')));
    assert_eq!(
        (
            &written["sandbox"],
            &written["change"],
            &written["reason"],
            &written["stopped"],
            &written["budget_usd"]
        ),
        (
            &json!(true),
            &json!("sample"),
            &json!("waiting"),
            &output["stopped"],
            &json!(1.0)
        )
    );
    // The run names the version of the flow the sandboxed change followed,
    // which is copied back with it, so the reference resolves here.
    let version = written["flow"].as_str().unwrap();
    assert!(version.starts_with("/flows/trial/"), "{version}");
    assert_eq!(
        board.read(&format!("{}.json", version.trim_start_matches('/')))["name"],
        "trial"
    );
    let stopped = output["stopped"].as_str().unwrap().trim_start_matches('/');
    assert_eq!(board.read(&format!("{stopped}.json"))["reason"], "waiting");
    assert_eq!(board.code(run), None);
    move_to(&board, "author", "dry-run");
    let gate = gate_of(&board, AUTHOR);
    assert_eq!(
        (gate[0].0.as_str(), gate[0].1.as_str()),
        ("dry-run", "pass")
    );
}

/// The stand-in agent: it tries a write-through put into the mount
/// /repos/y with the `board` its second argument names, keeping what
/// `board` said in the directory its first argument names.
const WRITING_THROUGH: &str = r#"out="$1"; board="$2"
echo '{"$type": "notes/note/v1"}' | "$board" put /repos/y/notes/escape > "$out/put" 2>&1
exit 0
"#;

#[test]
fn a_sandboxed_run_mounts_no_other_repository_so_a_write_through_put_never_reaches_it() {
    let board = authoring();
    // /repos/y is a board that commits its puts, checked out on the branch
    // its mount follows, so a put writes through into it.
    let config = r#"{"$type": "board/config/v1", "commit_on_put": true}"#;
    let schema = (
        ".board/types/notes/note/v1.schema.json",
        r#"{"type": "object"}"#,
    );
    board.mount_repo(&[(".board/board.json", config), schema], "repos/y", "main");
    board.write(
        "flows/repos/x.json",
        &json!({"$type": "board/mount/v1", "path": "/changes/author/draft"}).to_string(),
    );
    let y = board.repo_root("repos/y");
    // The put commits in /repos/y, so its repository names who commits,
    // and the machine's own git identity never answers for it.
    git(&y, &["config", "user.name", "t"]);
    git(&y, &["config", "user.email", "t@t"]);
    // The twin: the real board writes through into /repos/y.
    let real = board.run(
        &["put", "/repos/y/notes/real"],
        r#"{"$type": "notes/note/v1"}"#,
    );
    assert!(
        y.join("notes/real.json").exists(),
        "{} {}",
        text(&real.stdout),
        text(&real.stderr)
    );
    let before = git(&y, &["rev-parse", "HEAD"]);
    let script = board.root().join("writing.sh");
    write_file(&script, WRITING_THROUGH);
    let out = tempfile::tempdir().unwrap();
    let argv = json!(["/bin/sh", script, out.path(), binary()]).to_string();
    let output = board
        .invoke(
            "run",
            &dry_run(&json!({})),
            false,
            &[("BOARD_AGENT", argv.as_str())],
        )
        .unwrap();
    let sandbox = PathBuf::from(output["sandbox"].as_str().unwrap());
    let put = std::fs::read_to_string(out.path().join("put")).unwrap();
    assert!(!y.join("notes/escape.json").exists(), "{put}");
    assert_eq!(git(&y, &["rev-parse", "HEAD"]), before);
    assert!(!sandbox.join("board/repos/y.json").exists());
    assert!(sandbox.join("board/repos/x.json").exists());
    assert!(sandbox.join("board/flows/repos/x.json").exists());
}

#[test]
fn a_sandboxed_run_is_refused_inline_or_resuming_and_admitted_without_either() {
    let board = authoring();
    let out = tempfile::tempdir().unwrap();
    let inline = sandboxed(&board, &dry_run(&json!({"inline": true})), out.path()).unwrap_err();
    assert_eq!(inline["code"], "changes::sandbox::inline");
    let resuming = json!({"sandbox": true, "path": AUTHOR});
    let refused = sandboxed(&board, &resuming, out.path()).unwrap_err();
    assert_eq!(refused["code"], "changes::sandbox::resume");
    assert!(sandboxed(&board, &dry_run(&json!({})), out.path()).is_ok());
}

#[test]
fn a_sandboxed_run_of_a_named_flow_needs_the_change_it_is_for_and_runs_on_the_board_itself() {
    let board = authoring();
    board.write("flows/trial.json", &draft().to_string());
    let out = tempfile::tempdir().unwrap();
    let given = json!({"sandbox": true, "flow": "trial", "mount": "/", "title": "Sample"});
    let refused = sandboxed(&board, &given, out.path()).unwrap_err();
    assert_eq!(refused["code"], "changes::sandbox::no_change");
    let named = serde_json::from_str(&record(given, &json!({"for": AUTHOR}))).unwrap();
    let output = sandboxed(&board, &named, out.path()).unwrap();
    let sandbox = PathBuf::from(output["sandbox"].as_str().unwrap());
    assert_eq!(
        output["worktree"].as_str().unwrap().trim_end_matches('/'),
        sandbox
            .join("board")
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    );
    assert!(!sandbox.join("repo").exists());
    assert!(!board.exists("notes/escape.md"));
    assert!(
        output["run"]
            .as_str()
            .unwrap()
            .starts_with("/changes/author/runs/")
    );
}
