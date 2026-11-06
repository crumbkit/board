//! `changes.run`: it records where a change's run stops, `next` and
//! `advance` hold the change there, `--inline` returns the run brief, and
//! otherwise a stand-in agent runs in a worktree for the change and `run`
//! records the run and why it stopped. No test starts a real agent.

use super::*;

const CHANGE: &str = "/changes/quick-fix";
/// The stand-in agent: it keeps its stdin, `BOARD_CHANGE` and directory in
/// the directory its first argument names, prints a line and then the
/// report Claude Code's JSON output ends with, copies the file its fourth
/// argument names to its third when given, and exits with its second.
const AGENT: &str = r#"out="$1"
cat > "$out/stdin"
printf '%s' "$BOARD_CHANGE" > "$out/change"
pwd -P > "$out/pwd"
if [ -n "$3" ]; then cp "$4" "$3"; fi
echo working
echo '{"total_cost_usd": 0.5, "duration_ms": 1200}'
exit "$2"
"#;

/// The fixture board with /repos/x mounted and the flow `quick`: draft,
/// then land, neither with a check.
fn quick() -> Changes {
    quick_checking(&json!([]))
}

/// [`quick`], with `checks` on its land phase.
fn quick_checking(checks: &Value) -> Changes {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    let phases: Vec<Value> = (["draft", "land"].iter())
        .map(|phase| {
            let checks = if *phase == "land" { checks.clone() } else { json!([]) };
            json!({"name": phase, "agent": "lead", "instruction": format!("Do {phase}."), "checks": checks})
        })
        .collect();
    let flow = json!({
        "$type": "changes/flow/v1",
        "name": "quick",
        "summary": "A flow for a test.",
        "stop": {"until": "land"},
        "phases": phases,
    });
    board.write("flows/quick.json", &flow.to_string());
    board
}

/// The input of `run` that starts the change "Quick fix" on the flow
/// `quick`, with `fields`.
fn starting(fields: &Value) -> Value {
    let input = json!({"mount": "/repos/x", "task": TASK, "title": "Quick fix", "flow": "quick"});
    serde_json::from_str(&record(input, fields)).unwrap()
}

/// The input of `run` that resumes the change, with `fields`.
fn resuming(fields: &Value) -> Value {
    serde_json::from_str(&record(json!({"path": CHANGE}), fields)).unwrap()
}

fn next(board: &Changes) -> Value {
    board.call("next", &json!({"path": CHANGE}))
}

/// The stand-in agent's argv as `BOARD_AGENT` holds it: it writes into
/// `out` and exits `code`, after copying `source` to `target` when given.
fn agent(board: &Changes, out: &Path, code: i32, copy: Option<(&Path, &Path)>) -> String {
    let script = board.root().join("agent.sh");
    write_file(&script, AGENT);
    let mut argv = vec![
        "/bin/sh".to_owned(),
        script.display().to_string(),
        out.display().to_string(),
        code.to_string(),
    ];
    if let Some((target, source)) = copy {
        argv.extend([target.display().to_string(), source.display().to_string()]);
    }
    json!(argv).to_string()
}

/// `run` with `given` as flags and `BOARD_AGENT` set to `argv`.
fn spawned(board: &Changes, given: &Value, argv: &str) -> Result<Value, Value> {
    board.invoke("run", given, false, &[("BOARD_AGENT", argv)])
}

#[test]
fn run_inline_starts_the_change_records_its_stop_and_returns_the_brief_up_to_it() {
    let board = quick();
    let output = board.call("run", &starting(&json!({"inline": true})));
    assert_eq!(output["path"], CHANGE);
    assert_eq!(output["stop"], json!({"until": "land", "reached": false}));
    let brief = &output["brief"];
    let steps: Vec<&Value> = (brief["steps"].as_array().unwrap().iter())
        .map(|step| &step["phase"])
        .collect();
    assert_eq!(steps, [&json!("draft")]);
    assert!(
        brief["stop_rule"]
            .as_str()
            .unwrap()
            .contains("enters the land phase")
    );
    assert!(brief["stopping"].as_str().unwrap().contains("changes.stop"));
    assert!(brief.get("budget_usd").is_none());
    assert_eq!(
        board.read("changes/quick-fix.json")["stop"],
        json!({"until": "land"})
    );
    let through = board.call(
        "run",
        &resuming(&json!({"inline": true, "through": "land", "budget": 2})),
    );
    let steps: Vec<&Value> = (through["brief"]["steps"].as_array().unwrap().iter())
        .map(|step| &step["phase"])
        .collect();
    assert_eq!(steps, [&json!("draft"), &json!("land")]);
    assert_eq!(through["brief"]["budget_usd"], 2.0);
    assert_eq!(
        board.read("changes/quick-fix.json")["stop"],
        json!({"through": "land"})
    );
}

#[test]
fn a_stop_in_a_phase_the_flow_lacks_is_refused_and_one_in_its_phases_or_done_is_admitted() {
    let board = quick();
    board.call("run", &starting(&json!({"inline": true})));
    let refused = board.refusal(
        "run",
        &resuming(&json!({"inline": true, "until": "review"})),
    );
    assert_eq!(refused["code"], "changes::run::no_phase");
    assert_eq!(
        refused["fields"]["phases"],
        json!(["draft", "land", "done"])
    );
    let admitted = board.call("run", &resuming(&json!({"inline": true, "until": "done"})));
    assert_eq!(admitted["stop"], json!({"until": "done", "reached": false}));
}

#[test]
fn a_run_that_names_no_one_change_or_two_stops_is_refused_and_its_twin_is_admitted() {
    let board = quick();
    let refused = board.refusal("run", &json!({"inline": true}));
    assert_eq!(refused["code"], "changes::run::which_change");
    let both = serde_json::from_str(&record(
        starting(&json!({"inline": true})),
        &json!({"path": CHANGE}),
    ))
    .unwrap();
    assert_eq!(
        board.refusal("run", &both)["code"],
        "changes::run::which_change"
    );
    board.call("run", &starting(&json!({"inline": true})));
    let two = board.refusal(
        "run",
        &resuming(&json!({"inline": true, "through": "draft", "until": "land"})),
    );
    assert_eq!(two["code"], "changes::run::two_stops");
    board.call(
        "run",
        &resuming(&json!({"inline": true, "through": "draft"})),
    );
}

#[test]
fn advance_is_refused_past_the_stop_with_how_to_record_it_and_admitted_once_the_stop_moves() {
    let board = quick();
    board.call("run", &starting(&json!({"inline": true})));
    assert_eq!(
        next(&board)["stop"],
        json!({"until": "land", "reached": false})
    );
    let into = board.call("advance", &json!({"path": CHANGE}));
    assert_eq!(into["to"], "land");
    let at = next(&board);
    assert_eq!(at["stop"], json!({"until": "land", "reached": true}));
    assert!(
        at["instruction"]
            .as_str()
            .unwrap()
            .contains("--reason reached")
    );
    let refused = board.refusal("advance", &json!({"path": CHANGE}));
    assert_eq!(refused["code"], "changes::run::stop_reached");
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains("board call changes.stop --path /changes/quick-fix --reason reached"),
        "{message}"
    );
    assert!(!message.contains("--until"), "{message}");
    board.call("run", &resuming(&json!({"inline": true, "until": "done"})));
    assert_eq!(
        board.call("advance", &json!({"path": CHANGE}))["to"],
        "done"
    );
}

#[test]
fn advance_at_an_until_stop_whose_phase_has_an_unmet_check_is_refused_as_stop_reached() {
    let board = quick_checking(&json!([{"id": "head", "kind": "field", "field": "head"}]));
    board.call("run", &starting(&json!({"inline": true})));
    board.call("advance", &json!({"path": CHANGE}));
    let refused = board.refusal("advance", &json!({"path": CHANGE}));
    assert_eq!(refused["code"], "changes::run::stop_reached");
}

#[test]
fn advance_with_the_stop_moved_past_a_phase_with_an_unmet_check_is_refused_as_unmet() {
    let board = quick_checking(&json!([{"id": "head", "kind": "field", "field": "head"}]));
    board.call("run", &starting(&json!({"inline": true})));
    board.call("advance", &json!({"path": CHANGE}));
    board.call("run", &resuming(&json!({"inline": true, "until": "done"})));
    let refused = board.refusal("advance", &json!({"path": CHANGE}));
    assert_eq!(refused["code"], "changes::gate::unmet");
    assert_eq!(refused["fields"]["missing"], json!(["head"]));
}

/// [`quick`], with its draft phase relying on the skill `drafting`, and a
/// home whose skills directory holds `installed`.
fn quick_with_a_skill(installed: &[&str]) -> (Changes, tempfile::TempDir) {
    let board = quick();
    let mut flow = board.read("flows/quick.json");
    flow["phases"][0]["skills"] = json!(["drafting"]);
    board.write("flows/quick.json", &flow.to_string());
    let home = tempfile::tempdir().unwrap();
    for skill in installed {
        write_file(
            &home.path().join(format!(".claude/skills/{skill}/SKILL.md")),
            "---\nname: s\n---\n",
        );
    }
    (board, home)
}

#[test]
fn run_reports_a_skill_its_phases_rely_on_that_the_home_lacks() {
    let (board, home) = quick_with_a_skill(&[]);
    let home = home.path().to_str().unwrap();
    let output = board
        .invoke("run", &starting(&json!({"inline": true})), false, &[("HOME", home)])
        .unwrap();
    assert_eq!(output["skills_missing"]["names"], json!(["drafting"]));
    assert_eq!(
        output["skills_missing"]["directory"],
        json!(format!("{home}/.claude/skills"))
    );
}

#[test]
fn run_reports_no_skill_when_the_home_holds_every_one_its_phases_rely_on() {
    let (board, home) = quick_with_a_skill(&["drafting"]);
    let home = home.path().to_str().unwrap();
    let output = board
        .invoke("run", &starting(&json!({"inline": true})), false, &[("HOME", home)])
        .unwrap();
    assert_eq!(output.get("skills_missing"), None, "{output}");
}

#[test]
fn a_through_stop_is_reached_once_its_phases_gate_passes_and_holds_the_change_there() {
    let board = quick();
    let output = board.call(
        "run",
        &starting(&json!({"inline": true, "through": "draft"})),
    );
    assert_eq!(output["stop"], json!({"through": "draft", "reached": true}));
    let refused = board.refusal("advance", &json!({"path": CHANGE}));
    assert_eq!(refused["code"], "changes::run::stop_reached");
    board.call(
        "run",
        &resuming(&json!({"inline": true, "through": "land"})),
    );
    assert_eq!(
        board.call("advance", &json!({"path": CHANGE}))["to"],
        "land"
    );
}

#[test]
fn stop_records_why_a_run_stopped_and_refuses_a_question_of_another_type() {
    let board = quick();
    board.call("run", &starting(&json!({"inline": true})));
    board.write("questions/q.json", &typed("plans/decision/v1", &json!({})));
    let given = json!({"path": CHANGE, "reason": "waiting", "question": "/questions/q", "summary": "Waiting on the owner."});
    let refused = board.refusal("stop", &given);
    assert_eq!(refused["code"], "changes::ref::wrong_type");
    board.write(
        ".board/types/plans/question/v1.schema.json",
        r#"{"type": "object"}"#,
    );
    board.write("questions/q.json", &typed("plans/question/v1", &json!({})));
    let output = board.call("stop", &given);
    assert_eq!(
        (&output["phase"], &output["missing"]),
        (&json!("draft"), &json!([]))
    );
    let stop = output["stop"].as_str().unwrap().trim_start_matches('/');
    let written = board.read(&format!("{stop}.json"));
    assert_eq!(
        (&written["$type"], &written["reason"], &written["question"]),
        (
            &json!("changes/stop/v1"),
            &json!("waiting"),
            &json!("/questions/q")
        )
    );
}

#[test]
fn run_without_an_agent_is_refused_with_how_to_set_one_and_the_config_files_agent_is_admitted() {
    let mut board = quick();
    let refused = board.refusal("run", &starting(&json!({})));
    assert_eq!(refused["code"], "changes::agent::not_set");
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("--inline true")
    );
    let invalid = board.refusal_inheriting(
        "run",
        &starting(&json!({})),
        &[("BOARD_AGENT", "claude -p")],
    );
    assert_eq!(invalid["code"], "changes::agent::invalid");
    let out = tempfile::tempdir().unwrap();
    let argv = agent(&board, out.path(), 0, None);
    let config = board.cache.path().join("config.json");
    write_file(
        &config,
        &json!({"agent": serde_json::from_str::<Value>(&argv).unwrap()}).to_string(),
    );
    board.boards = Some(config);
    let output = board.call("run", &starting(&json!({})));
    assert_eq!(output["reason"], "error");
    assert_eq!(
        std::fs::read_to_string(out.path().join("change")).unwrap(),
        CHANGE
    );
}

#[test]
fn board_agent_wins_over_the_config_files_agent() {
    let mut board = quick();
    let config = board.cache.path().join("config.json");
    write_file(&config, r#"{"agent": ["/nonexistent/agent"]}"#);
    board.boards = Some(config);
    assert_eq!(
        board.refusal("run", &starting(&json!({})))["code"],
        "changes::agent::not_started"
    );
    let out = tempfile::tempdir().unwrap();
    let argv = agent(&board, out.path(), 0, None);
    // The refused run had started the change, so this one resumes it.
    assert!(spawned(&board, &resuming(&json!({})), &argv).is_ok());
}

#[test]
fn run_starts_the_agent_in_a_worktree_for_the_change_and_records_the_run_and_its_stop() {
    let board = quick();
    let out = tempfile::tempdir().unwrap();
    let argv = agent(&board, out.path(), 3, None);
    let output = spawned(&board, &starting(&json!({"budget": 1})), &argv).unwrap();
    let worktree = board.repo_root("repos/x").display().to_string() + ".worktrees/quick-fix";
    assert_eq!(
        (&output["reason"], &output["exit"], &output["stop"]),
        (
            &json!("error"),
            &json!(3),
            &json!({"until": "land", "reached": false})
        )
    );
    assert_eq!(
        output["worktree"].as_str().unwrap().trim_end_matches('/'),
        worktree
    );
    let ran_in = std::fs::read_to_string(out.path().join("pwd")).unwrap();
    assert_eq!(
        Path::new(ran_in.trim()),
        Path::new(&worktree).canonicalize().unwrap()
    );
    assert_eq!(
        git(Path::new(&worktree), &["branch", "--show-current"]),
        "quick-fix"
    );
    let stdin = std::fs::read_to_string(out.path().join("stdin")).unwrap();
    let (line, brief) = stdin.split_once("\n\n").unwrap();
    assert!(line == "board-brief: /changes/quick-fix", "{line}");
    let brief: Value = serde_json::from_str(brief).unwrap();
    assert_eq!(
        (&brief["path"], &brief["budget_usd"]),
        (&json!(CHANGE), &json!(1.0))
    );
    let run = output["run"].as_str().unwrap().trim_start_matches('/');
    let written = board.read(&format!("{run}.json"));
    assert_eq!(
        (
            &written["program"],
            &written["exit"],
            &written["cost_usd"],
            &written["duration_ms"]
        ),
        (&json!("sh"), &json!(3), &json!(0.5), &json!(1200))
    );
    assert_eq!(
        (
            &written["sandbox"],
            &written["phase"],
            &written["stopped"],
            &written["budget_usd"]
        ),
        (
            &json!(false),
            &json!("draft"),
            &output["stopped"],
            &json!(1.0)
        )
    );
    let stopped = output["stopped"].as_str().unwrap().trim_start_matches('/');
    let stop = board.read(&format!("{stopped}.json"));
    assert_eq!(
        stop["summary"],
        "The agent exited 3 before recording a stop."
    );
    // A resumed run reuses the worktree and the branch.
    let again = spawned(
        &board,
        &resuming(&json!({"budget": 0.25})),
        &agent(&board, out.path(), 0, None),
    )
    .unwrap();
    assert_eq!(again["worktree"], output["worktree"]);
    assert_eq!(again["reason"], "budget");
}

#[test]
fn a_stop_the_agent_wrote_is_the_runs_and_run_writes_none_of_its_own() {
    let board = quick();
    board.call("run", &starting(&json!({"inline": true})));
    let out = tempfile::tempdir().unwrap();
    let source = out.path().join("stop.json");
    let stop = json!({"$type": "changes/stop/v1", "reason": "waiting", "phase": "draft", "missing": [], "summary": "Waiting on the owner."});
    write_file(&source, &stop.to_string());
    let target = board
        .root()
        .join("changes/quick-fix/stops/29991231T000000000000Z.json");
    make_dir(target.parent().unwrap());
    let argv = agent(&board, out.path(), 0, Some((&target, &source)));
    let output = spawned(&board, &resuming(&json!({})), &argv).unwrap();
    assert_eq!(
        (&output["reason"], &output["stopped"]),
        (
            &json!("waiting"),
            &json!("/changes/quick-fix/stops/29991231T000000000000Z")
        )
    );
    let stops = std::fs::read_dir(target.parent().unwrap()).unwrap().count();
    assert_eq!(stops, 1);
}

#[test]
fn a_run_whose_stop_is_already_reached_starts_no_agent() {
    let board = quick();
    board.call(
        "run",
        &starting(&json!({"inline": true, "through": "draft"})),
    );
    let out = tempfile::tempdir().unwrap();
    let argv = agent(&board, out.path(), 0, None);
    let output = spawned(&board, &resuming(&json!({})), &argv).unwrap();
    assert_eq!(output["stop"], json!({"through": "draft", "reached": true}));
    assert!(output.get("run").is_none());
    assert!(!out.path().join("stdin").exists());
}
