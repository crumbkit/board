//! The write calls through the binary: add, claim, done, defer, drop,
//! reviewed and move, in the board's own tree and in a mount. These were
//! `test_write.py`; its unit test of the write's retries is the crate's,
//! against a real mount. Dates are counted from today in UTC.

use super::*;

const NOT_SYNCED: &str = "tasks::repo::not_synced";
const EXAMPLE_ORG: &str = "git@github.com:example-org/home.git";

#[test]
fn add_writes_an_open_task_named_by_its_title_reviewed_today_and_a_second_one_beside_it() {
    let tasks = Tasks::new();
    let flags = [
        "--title",
        "Call the bank",
        "--defer",
        "soon",
        "--text",
        "Ask about fees.",
    ];
    let output = tasks.call("add", &flags).unwrap();
    let expected = json!({
        "repo": "/",
        "path": "/tasks/call-the-bank.md",
        "status": "open",
        "defer": day(20),
        "owner": null,
        "reviewed": day(0),
        "commit": null,
        "unsynced": NOT_SYNCED,
        "unpushed": NOT_SYNCED,
    });
    assert_eq!(output, expected);
    assert_eq!(
        tasks.read("tasks/call-the-bank.md"),
        format!(
            "---\n$type: \"tasks/task/v2\"\ntitle: \"Call the bank\"\nstatus: \"open\"\ndefer: \"{}\"\n\
             reviewed: \"{}\"\n---\n\nAsk about fees.\n",
            day(20),
            day(0)
        )
    );
    let stdin = json!({"title": "Call the bank", "contexts": ["phone"]}).to_string();
    let again = (tasks.call_with("add", &[], 0, tasks.outside.path(), &stdin)).unwrap();
    assert_eq!(
        (&again["path"], &again["defer"]),
        (&json!("/tasks/call-the-bank-2.md"), &Value::Null)
    );
    assert!(
        tasks
            .read("tasks/call-the-bank-2.md")
            .contains("contexts: [\"phone\"]\n")
    );
    let listed = tasks.call("available", &[]).unwrap();
    let paths: Vec<&Value> = (listed["available"]["tasks"].as_array().unwrap().iter())
        .map(|item| &item["path"])
        .collect();
    assert_eq!(paths, [&json!("/tasks/call-the-bank-2.md")]);
}

#[test]
fn add_goes_to_the_repo_covering_the_remote_and_pushes_and_outside_one_to_the_personal_repo() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    let output = tasks
        .call("add", &["--title", "Ship it", "--remote", EXAMPLE_ORG])
        .unwrap();
    assert_eq!(
        (&output["repo"], &output["path"]),
        (
            &json!("/repos/work"),
            &json!("/repos/work/tasks/ship-it.md")
        )
    );
    assert_eq!(
        (&output["unsynced"], &output["unpushed"]),
        (&Value::Null, &Value::Null)
    );
    let head = git(&work, &["rev-parse", "HEAD"]);
    assert_eq!(output["commit"], json!(head));
    assert_eq!(git(&work, &["rev-parse", "origin/main"]), head);
    let tracked = git(&work, &["ls-tree", "-r", "--name-only", "origin/main"]);
    assert!(tracked.contains("tasks/ship-it.md"), "{tracked}");
    let personal = tasks.call("add", &["--title", "Buy milk"]).unwrap();
    assert_eq!(personal["path"], "/repos/personal/tasks/buy-milk.md");
}

#[test]
fn add_refuses_a_remote_two_repos_cover_and_writes_to_one_named_with_repo() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    let covers = json!({"covers": ["example-org/home"]});
    tasks.notes_repo("more", Some(covers), &[], true);
    let refused = tasks.refused("add", &["--title", "Ship it", "--remote", EXAMPLE_ORG]);
    assert_eq!(refused["code"], "tasks::repo::ambiguous");
    assert_eq!(
        refused["fields"]["candidates"],
        json!(["/repos/more", "/repos/work"])
    );
    assert!(!work.join("tasks/ship-it.md").exists());
    let flags = [
        "--title",
        "Ship it",
        "--remote",
        EXAMPLE_ORG,
        "--repo",
        "/repos/work",
    ];
    let output = tasks.call("add", &flags).unwrap();
    assert_eq!(output["path"], "/repos/work/tasks/ship-it.md");
    let refused = tasks.refused("add", &["--title", "Ship it", "--repo", "/repos/elsewhere"]);
    assert_eq!(refused["code"], "tasks::repo::not_found");
}

#[test]
fn done_sets_status_and_reviewed_and_keeps_every_other_byte() {
    let tasks = Tasks::new();
    let text = "---\n$type: tasks/task/v2\ntitle: Pay rent  # monthly\nstatus: open\nid: \"r1\"\n---\n\nBody.\n";
    tasks.write("tasks/rent.md", text);
    let output = tasks.call("done", &["--path", "/tasks/rent.md"]).unwrap();
    assert_eq!(
        (&output["status"], &output["reviewed"], &output["commit"]),
        (&json!("done"), &json!(day(0)), &Value::Null)
    );
    let done = format!("status: \"done\"\nid: \"r1\"\nreviewed: \"{}\"\n", day(0));
    assert_eq!(
        tasks.read("tasks/rent.md"),
        text.replace("status: open\nid: \"r1\"\n", &done)
    );
}

#[test]
fn done_defers_a_repeating_task_to_its_next_occurrence_and_it_stays_open() {
    let tasks = Tasks::new();
    let water = json!({"repeat": "daily", "defer": day(-3)});
    tasks.write("tasks/water.md", &open("Water the plants", water));
    let stretch = json!({"repeat": "daily", "ephemeral": true, "defer": day(-3)});
    tasks.write("tasks/stretch.md", &open("Stretch", stretch));
    let output = tasks.call("done", &["--path", "/tasks/water.md"]).unwrap();
    assert_eq!(
        (&output["status"], &output["defer"]),
        (&json!("open"), &json!(day(-2)))
    );
    let output = tasks
        .call("done", &["--path", "/tasks/stretch.md"])
        .unwrap();
    assert_eq!(
        (&output["status"], &output["defer"]),
        (&json!("open"), &json!(day(1)))
    );
}

#[test]
fn a_finished_task_is_refused_by_done_defer_and_drop_and_admitted_by_reviewed() {
    let tasks = Tasks::new();
    tasks.write("tasks/old.md", &task("Old", "done", &json!({}), ""));
    let cases: [(&str, &[&str]); 3] = [
        ("done", &[]),
        ("drop", &[]),
        ("defer", &["--until", "soon"]),
    ];
    for (capability, flags) in cases {
        let mut every = vec!["--path", "/tasks/old.md"];
        every.extend(flags);
        let refused = tasks.refused(capability, &every);
        assert_eq!(refused["code"], "tasks::task::finished", "{capability}");
        assert_eq!(
            (&refused["fields"]["path"], &refused["fields"]["status"]),
            (&json!("/tasks/old.md"), &json!("done"))
        );
    }
    let output = tasks
        .call("reviewed", &["--path", "/tasks/old.md"])
        .unwrap();
    assert_eq!(
        (&output["status"], &output["reviewed"]),
        (&json!("done"), &json!(day(0)))
    );
}

#[test]
fn defer_writes_a_horizon_as_its_date_and_a_date_as_it_is_and_drop_drops() {
    let tasks = Tasks::new();
    tasks.write("tasks/a.md", &open("A", json!({})));
    let path = ["--path", "/tasks/a.md"];
    let deferred = tasks.call(
        "defer",
        &[&path[..], &["--until", "not-this-week"]].concat(),
    );
    assert_eq!(deferred.unwrap()["defer"], json!(day(6)));
    let later = day(40);
    let deferred = tasks.call("defer", &[&path[..], &["--until", &later]].concat());
    assert_eq!(deferred.unwrap()["defer"], json!(later));
    assert!(
        tasks
            .read("tasks/a.md")
            .contains(&format!("defer: \"{later}\""))
    );
    let output = tasks.call("drop", &path).unwrap();
    assert_eq!(output["status"], "dropped");
}

#[test]
fn claim_refuses_a_task_another_owner_holds_and_admits_one_held_by_nobody_or_the_same_owner() {
    let tasks = Tasks::new();
    tasks.write("tasks/free.md", &open("Free", json!({})));
    tasks.write(
        "tasks/taken.md",
        &open("Taken", json!({"owner": {"person": "worker-2"}})),
    );
    let refused = tasks.refused(
        "claim",
        &["--path", "/tasks/taken.md", "--person", "lead-1"],
    );
    assert_eq!(refused["code"], "tasks::task::held");
    assert_eq!(
        (&refused["fields"]["path"], &refused["fields"]["owner"]),
        (&json!("/tasks/taken.md"), &json!({"person": "worker-2"}))
    );
    assert!(
        tasks
            .read("tasks/taken.md")
            .contains(r#"owner: {"person":"worker-2"}"#)
    );
    let output = tasks.call(
        "claim",
        &["--path", "/tasks/taken.md", "--person", "worker-2"],
    );
    let output = output.unwrap();
    assert_eq!(
        (&output["owner"], &output["reviewed"]),
        (&json!({"person": "worker-2"}), &json!(day(0)))
    );
    let output = tasks
        .call("claim", &["--path", "/tasks/free.md", "--person", "lead-1"])
        .unwrap();
    assert_eq!(
        (&output["owner"], &output["status"]),
        (&json!({"person": "lead-1"}), &json!("open"))
    );
    assert!(
        tasks
            .read("tasks/free.md")
            .contains(r#"owner: {"person":"lead-1"}"#)
    );
}

#[test]
fn an_owner_part_outside_its_grammar_is_refused_by_available_and_claim_and_its_twin_is_admitted() {
    let tasks = Tasks::new();
    let cases = [
        (["--person", ""], ["--person", "lead-1"]),
        (["--session", "lead-1"], ["--session", "local_0a1b"]),
        (["--program", "Claude Code"], ["--program", "claude-code"]),
    ];
    for (at, (refused, admitted)) in cases.into_iter().enumerate() {
        let path = format!("/tasks/{at}.md");
        tasks.write(&path[1..], &open("A", json!({})));
        let claim = ["--path", path.as_str()];
        for (capability, flags) in [("available", &[][..]), ("claim", &claim[..])] {
            let shown = tasks.refused(capability, &[flags, &refused].concat());
            assert_eq!(shown["code"], "board::plugin::bad_input", "{capability}");
            let answered = tasks.call(capability, &[flags, &admitted].concat());
            assert!(answered.is_ok(), "{capability}: {answered:?}");
        }
    }
}

#[test]
fn a_claim_that_names_nobody_is_refused_and_one_naming_a_program_is_admitted() {
    let tasks = Tasks::new();
    tasks.write("tasks/a.md", &open("A", json!({})));
    let refused = tasks.refused("claim", &["--path", "/tasks/a.md"]);
    assert_eq!(refused["code"], "tasks::claim::no_owner");
    let output = tasks.call("claim", &["--path", "/tasks/a.md", "--program", "codex"]);
    assert_eq!(output.unwrap()["owner"], json!({"program": "codex"}));
}

#[test]
fn done_defer_and_drop_leave_the_owner_and_claim_refuses_a_finished_task() {
    let tasks = Tasks::new();
    for name in ["a", "b"] {
        let text = open(&name.to_uppercase(), json!({"owner": {"person": "lead-1"}}));
        tasks.write(&format!("tasks/{name}.md"), &text);
    }
    let deferred = tasks.call("defer", &["--path", "/tasks/a.md", "--until", "soon"]);
    assert_eq!(deferred.unwrap()["owner"], json!({"person": "lead-1"}));
    assert_eq!(
        tasks.call("done", &["--path", "/tasks/a.md"]).unwrap()["owner"],
        json!({"person": "lead-1"})
    );
    assert_eq!(
        tasks.call("drop", &["--path", "/tasks/b.md"]).unwrap()["owner"],
        json!({"person": "lead-1"})
    );
    for name in ["a", "b"] {
        assert!(
            tasks
                .read(&format!("tasks/{name}.md"))
                .contains(r#"owner: {"person":"lead-1"}"#)
        );
    }
    let refused = tasks.refused("claim", &["--path", "/tasks/a.md", "--person", "lead-1"]);
    assert_eq!(refused["code"], "tasks::task::finished");
}

#[test]
fn a_path_holding_no_task_is_refused_and_a_task_beside_it_is_admitted() {
    let tasks = Tasks::new();
    tasks.write("tasks/note.md", "# A note, not a task\n");
    tasks.write("notes/elsewhere.md", &open("Elsewhere", json!({})));
    let fortnightly = open("Fortnightly", json!({"repeat": "fortnightly"}));
    tasks.write("tasks/fortnightly.md", &fortnightly);
    tasks.write(
        "tasks/real.md",
        &open("Real", json!({"repeat": "weekly-mon"})),
    );
    let mut last = Value::Null;
    for (path, code) in [
        ("/tasks/gone.md", "tasks::task::not_found"),
        ("/tasks/note.md", "tasks::task::not_task"),
        ("/notes/elsewhere.md", "tasks::task::not_task"),
        ("/tasks/fortnightly.md", "tasks::task::not_task"),
    ] {
        last = tasks.refused("done", &["--path", path]);
        assert_eq!(
            (&last["code"], &last["fields"]["path"]),
            (&json!(code), &json!(path))
        );
    }
    let message = last["message"].as_str().unwrap();
    assert!(
        message.contains("`board check /tasks/fortnightly.md`"),
        "{message}"
    );
    assert!(tasks.call("done", &["--path", "/tasks/real.md"]).is_ok());
}

#[test]
fn done_in_a_mount_pushes_the_commit_and_moves_the_pin() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    let output = tasks
        .call("done", &["--path", "/repos/work/tasks/report.md"])
        .unwrap();
    let head = git(&work, &["rev-parse", "HEAD"]);
    assert_eq!(
        (&output["repo"], &output["commit"], &output["unpushed"]),
        (&json!("/repos/work"), &json!(head), &Value::Null)
    );
    assert_eq!(git(&work, &["rev-parse", "origin/main"]), head);
    let pointer: Value = serde_json::from_str(&tasks.read("repos/work.json")).unwrap();
    assert_eq!(pointer["commit"], json!(head));
    assert!(git(&work, &["show", "HEAD:tasks/report.md"]).contains("status: \"done\""));
}

#[test]
fn a_write_after_a_sync_that_cannot_reach_the_remote_is_committed_and_the_next_call_pushes_it() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    let remote = tasks.beside.path().join("work.git");
    let away = tasks.beside.path().join("away.git");
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture takes the remote away"
    )]
    std::fs::rename(&remote, &away).unwrap();
    let output = tasks
        .call("done", &["--path", "/repos/work/tasks/report.md"])
        .unwrap();
    assert_eq!(output["unsynced"], "board::sync::failed");
    let head = git(&work, &["rev-parse", "HEAD"]);
    assert_eq!(output["commit"], json!(head));
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture brings the remote back"
    )]
    std::fs::rename(&away, &remote).unwrap();
    assert_ne!(git(&work, &["rev-parse", "origin/main"]), head);
    let output = tasks
        .call("reviewed", &["--path", "/repos/work/tasks/report.md"])
        .unwrap();
    assert_eq!(
        (&output["unsynced"], &output["unpushed"]),
        (&Value::Null, &Value::Null)
    );
    let pushed = git(&remote, &["show", "main:tasks/report.md"]);
    assert_eq!(pushed.matches("status: \"done\"").count(), 1);
}

#[test]
fn a_task_in_a_mount_that_is_not_a_notes_repo_is_refused_and_one_in_a_notes_repo_is_admitted() {
    let tasks = Tasks::new();
    tasks.mounted();
    let chore = open("Chore", json!({}));
    let plain = tasks.notes_repo("plain", None, &[("chore", &chore)], false);
    let refused = tasks.refused("done", &["--path", "/repos/plain/tasks/chore.md"]);
    assert_eq!(refused["code"], "tasks::repo::not_found");
    assert_eq!(refused["fields"]["path"], "/repos/plain/tasks/chore.md");
    assert_eq!(git(&plain, &["status", "--porcelain"]), "");
    let commits = git(&plain, &["rev-list", "--count", "HEAD"]);
    assert_eq!(commits, "1", "nothing was committed in the plain clone");
    let output = tasks
        .call("done", &["--path", "/repos/work/tasks/report.md"])
        .unwrap();
    assert_eq!(output["repo"], "/repos/work");
}

#[test]
fn a_push_that_fails_after_the_write_is_reported_and_the_write_stays_committed() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    let hook = tasks.beside.path().join("work.git/hooks/pre-receive");
    write_hook(&hook);
    let output = tasks
        .call("done", &["--path", "/repos/work/tasks/report.md"])
        .unwrap();
    assert!(!output["unpushed"].is_null(), "{output}");
    let head = git(&work, &["rev-parse", "HEAD"]);
    assert_eq!(output["commit"], json!(head));
    assert_ne!(git(&work, &["rev-parse", "origin/main"]), head);
    assert!(git(&work, &["show", "HEAD:tasks/report.md"]).contains("status: \"done\""));
}

/// A `pre-receive` hook at `hook` that refuses every push.
#[expect(
    clippy::disallowed_methods,
    reason = "the remote's hook is git's, which the fixture writes as its owner would"
)]
fn write_hook(hook: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(hook, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn add_inside_a_git_repository_without_origin_is_refused_and_admitted_with_remote() {
    let tasks = Tasks::new();
    tasks.mounted();
    let checkout = tasks.checkout();
    let refused = tasks
        .call_with("add", &["--title", "Ship it"], 0, &checkout, "")
        .unwrap_err();
    assert_eq!(refused["code"], "tasks::repo::origin_unread");
    let why = refused["fields"]["origin_unread"].as_str().unwrap();
    assert!(why.starts_with("git gave no origin for "), "{why}");
    let flags = ["--title", "Ship it", "--remote", EXAMPLE_ORG];
    let output = tasks.call_with("add", &flags, 0, &checkout, "").unwrap();
    assert_eq!(output["path"], "/repos/work/tasks/ship-it.md");
    let clones = tasks.clones.borrow();
    let personal = clones[0].1.workdir().join("tasks");
    assert!(!personal.exists(), "no task went to the personal repo");
}

#[test]
fn a_write_before_the_configured_day_start_is_reviewed_on_the_day_before() {
    let tasks = Tasks::new();
    let (offset, local) = one_in_the_morning();
    let yesterday = jiff::civil::Date::strptime("%Y-%m-%d", &local)
        .unwrap()
        .yesterday()
        .unwrap()
        .to_string();
    tasks.write("tasks/a.md", &open("A", json!({})));
    tasks.day_start(Some("07:00"));
    let outside = tasks.outside.path();
    let flags = ["--path", "/tasks/a.md", "--until", "not-today"];
    let output = tasks
        .call_with("defer", &flags, offset, outside, "")
        .unwrap();
    assert_eq!(
        (&output["reviewed"], &output["defer"]),
        (&json!(yesterday), &json!(local))
    );
    tasks.day_start(None);
    let flags = ["--path", "/tasks/a.md"];
    let output = tasks
        .call_with("reviewed", &flags, offset, outside, "")
        .unwrap();
    assert_eq!(output["reviewed"], json!(local));
}

#[test]
fn a_task_with_an_edit_not_committed_in_its_clone_is_busy_and_admitted_once_the_edit_is_gone() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    let report = work.join("tasks/report.md");
    let edited = std::fs::read_to_string(&report).unwrap() + "An edit on disk.\n";
    tasks.clones.borrow()[1]
        .1
        .write("tasks/report.md", Item::Fresh(&edited));
    let refused = tasks.refused("done", &["--path", "/repos/work/tasks/report.md"]);
    assert_eq!(refused["code"], "tasks::task::busy");
    assert_eq!(refused["fields"]["code"], "board::mount::stale");
    assert!(
        refused["message"].as_str().unwrap().contains("call again"),
        "{refused}"
    );
    assert!(
        std::fs::read_to_string(&report)
            .unwrap()
            .ends_with("An edit on disk.\n")
    );
    git(&work, &["checkout", "--", "tasks/report.md"]);
    let output = tasks
        .call("done", &["--path", "/repos/work/tasks/report.md"])
        .unwrap();
    assert_eq!(
        (&output["status"], &output["unsynced"]),
        (&json!("done"), &Value::Null)
    );
}

#[test]
fn add_writes_a_project_with_its_brief_under_a_parent() {
    let tasks = Tasks::new();
    tasks.write("tasks/flows.md", &open("Flows", json!({})));
    let flags = [
        "--title",
        "Flow document store",
        "--kind",
        "project",
        "--brief",
        "/projects/flows/README.md",
        "--parent",
        "/tasks/flows.md",
    ];
    let output = tasks.call("add", &flags).unwrap();
    assert_eq!(output["path"], "/tasks/flow-document-store.md");
    assert_eq!(
        tasks.read("tasks/flow-document-store.md"),
        format!(
            "---\n$type: \"tasks/task/v2\"\ntitle: \"Flow document store\"\nstatus: \"open\"\n\
             kind: \"project\"\nbrief: \"/projects/flows/README.md\"\nparent: \"/tasks/flows.md\"\n\
             reviewed: \"{}\"\n---\n",
            day(0)
        )
    );
    let refused = tasks.refused("add", &["--title", "Epic", "--kind", "epic"]);
    assert_eq!(refused["code"], "board::plugin::bad_input");
}

const PIECE: &str = "---\n$type: tasks/task/v2\ntitle: Piece  # of the plan\nstatus: done\nreviewed: \"2026-10-01\"\n---\n\nBody.\n";

/// The board with a plan, a finished piece and an open bit under the
/// piece, and another task: the plan's text.
fn plan(tasks: &Tasks) -> String {
    let plan = open("Plan", json!({"reviewed": "2026-10-01"}));
    tasks.write("tasks/plan.md", &plan);
    tasks.write("tasks/piece.md", PIECE);
    tasks.write(
        "tasks/bit.md",
        &open("Bit", json!({"parent": "/tasks/piece.md"})),
    );
    tasks.write("tasks/other.md", &open("Other", json!({})));
    plan
}

#[test]
fn move_sets_the_parent_of_any_task_and_keeps_every_other_byte_and_none_clears_it() {
    let tasks = Tasks::new();
    plan(&tasks);
    let output = tasks.call(
        "move",
        &["--path", "/tasks/piece.md", "--parent", "/tasks/plan.md"],
    );
    let expected = json!({
        "repo": "/",
        "path": "/tasks/piece.md",
        "parent": "/tasks/plan.md",
        "commit": null,
        "unsynced": NOT_SYNCED,
        "unpushed": NOT_SYNCED,
    });
    assert_eq!(output.unwrap(), expected);
    let moved = PIECE.replace(
        "\"2026-10-01\"\n",
        "\"2026-10-01\"\nparent: \"/tasks/plan.md\"\n",
    );
    assert_eq!(tasks.read("tasks/piece.md"), moved);
    let output = tasks
        .call("move", &["--path", "/tasks/piece.md", "--parent", "none"])
        .unwrap();
    assert_eq!(output["parent"], Value::Null);
    assert_eq!(tasks.read("tasks/piece.md"), PIECE);
}

#[test]
fn move_refuses_a_parent_that_is_the_task_or_under_it_and_admits_one_beside_it() {
    let tasks = Tasks::new();
    let text = plan(&tasks);
    let under = PIECE.replace(
        "status: done\n",
        "status: done\nparent: \"/tasks/plan.md\"\n",
    );
    tasks.write("tasks/piece.md", &under);
    for parent in ["/tasks/plan.md", "/tasks/piece.md", "/tasks/bit.md"] {
        let refused = tasks.refused("move", &["--path", "/tasks/plan.md", "--parent", parent]);
        assert_eq!(refused["code"], "tasks::move::cycle");
        assert_eq!(
            (&refused["fields"]["path"], &refused["fields"]["parent"]),
            (&json!("/tasks/plan.md"), &json!(parent))
        );
    }
    assert_eq!(tasks.read("tasks/plan.md"), text);
    let flags = ["--path", "/tasks/plan.md", "--parent", "/tasks/other.md"];
    assert!(tasks.call("move", &flags).is_ok());
}

#[test]
fn move_refuses_a_parent_that_holds_no_task_and_admits_a_task() {
    let tasks = Tasks::new();
    plan(&tasks);
    tasks.write("tasks/note.md", "# A note\n");
    for parent in ["/tasks/gone.md", "/tasks/note.md"] {
        let refused = tasks.refused("move", &["--path", "/tasks/other.md", "--parent", parent]);
        assert_eq!(
            (&refused["code"], &refused["fields"]["parent"]),
            (&json!("tasks::move::parent_not_found"), &json!(parent))
        );
    }
    let flags = ["--path", "/tasks/other.md", "--parent", "/tasks/plan.md"];
    assert!(tasks.call("move", &flags).is_ok());
}

#[test]
fn move_refuses_a_parent_in_another_repo_and_admits_one_in_the_tasks_own() {
    let tasks = Tasks::new();
    plan(&tasks);
    tasks.mounted();
    for (path, parent) in [
        ("/tasks/other.md", "/repos/work/tasks/report.md"),
        ("/repos/work/tasks/report.md", "/tasks/plan.md"),
    ] {
        let refused = tasks.refused("move", &["--path", path, "--parent", parent]);
        assert_eq!(refused["code"], "tasks::move::cross_repo");
        assert_eq!(
            (&refused["fields"]["path"], &refused["fields"]["parent"]),
            (&json!(path), &json!(parent))
        );
    }
    let flags = ["--path", "/tasks/other.md", "--parent", "/tasks/plan.md"];
    assert!(tasks.call("move", &flags).is_ok());
}

#[test]
fn add_takes_board_paths_for_parent_and_brief_and_writes_them_as_paths_in_the_tasks_repo() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    let flags = [
        "--title",
        "Child",
        "--repo",
        "/repos/work",
        "--parent",
        "/repos/work/tasks/report.md",
        "--brief",
        "/repos/work/projects/x/README.md",
    ];
    let output = tasks.call("add", &flags).unwrap();
    assert_eq!(output["path"], "/repos/work/tasks/child.md");
    let text = std::fs::read_to_string(work.join("tasks/child.md")).unwrap();
    assert!(
        text.contains("brief: \"/projects/x/README.md\"\nparent: \"/tasks/report.md\"\n"),
        "{text}"
    );
    let tree = tasks
        .call("tree", &["--path", "/repos/work/tasks/report.md"])
        .unwrap();
    let child = &tree["tree"]["children"][0];
    assert_eq!(
        (&child["path"], &child["tree_path"], &child["brief"]),
        (
            &json!("/repos/work/tasks/child.md"),
            &json!("report/child"),
            &json!("/repos/work/projects/x/README.md")
        )
    );
}

#[test]
fn add_refuses_a_parent_or_brief_in_another_repo_and_a_parent_that_holds_no_task() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    tasks.write("tasks/plan.md", &open("Plan", json!({})));
    for (flag, value, code) in [
        ("--parent", "/tasks/plan.md", "tasks::add::cross_repo"),
        ("--brief", "/projects/x/README.md", "tasks::add::cross_repo"),
        (
            "--parent",
            "/repos/work/tasks/gone.md",
            "tasks::add::parent_not_found",
        ),
    ] {
        let flags = ["--title", "Child", "--repo", "/repos/work", flag, value];
        let refused = tasks.refused("add", &flags);
        assert_eq!(refused["code"], code);
        assert_eq!(
            refused["fields"][flag.trim_start_matches("--")],
            json!(value)
        );
    }
    assert!(!work.join("tasks/child.md").exists());
    let flags = [
        "--title",
        "Child",
        "--repo",
        "/repos/work",
        "--parent",
        "/repos/work/tasks/report.md",
    ];
    assert!(tasks.call("add", &flags).is_ok());
}

#[test]
fn move_inside_a_mount_writes_the_parent_as_a_path_in_that_repo() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    assert!(
        tasks
            .call("add", &["--title", "Plan B", "--repo", "/repos/work"])
            .is_ok()
    );
    let flags = [
        "--path",
        "/repos/work/tasks/report.md",
        "--parent",
        "/repos/work/tasks/plan-b.md",
    ];
    let output = tasks.call("move", &flags).unwrap();
    assert_eq!(output["parent"], "/repos/work/tasks/plan-b.md");
    let report = || std::fs::read_to_string(work.join("tasks/report.md")).unwrap();
    assert!(report().contains("parent: \"/tasks/plan-b.md\"\n"));
    let tree = tasks
        .call("tree", &["--path", "/repos/work/tasks/plan-b.md"])
        .unwrap();
    assert_eq!(tree["tree"]["children"][0]["tree_path"], "plan-b/report");
    let clear = ["--path", "/repos/work/tasks/report.md", "--parent", "none"];
    assert!(tasks.call("move", &clear).is_ok());
    let cleared = report();
    assert!(!cleared.contains("parent:"));
    assert!(tasks.call("move", &clear).is_ok());
    assert_eq!(report(), cleared);
}

#[test]
fn add_finds_a_parent_its_sync_just_pulled_and_refuses_one_nobody_pushed() {
    let tasks = Tasks::new();
    let work = tasks.mounted();
    let phone = tasks.phone("work");
    write_file(&phone.join("tasks/plan.md"), &open("Plan", json!({})));
    git(&phone, &["add", "."]);
    git(&phone, &["commit", "-qm", "from the phone"]);
    git(&phone, &["push", "-q"]);
    let flags = ["--title", "Child", "--repo", "/repos/work"];
    let refused = tasks.refused(
        "add",
        &[&flags[..], &["--parent", "/repos/work/tasks/elsewhere.md"]].concat(),
    );
    assert_eq!(refused["code"], "tasks::add::parent_not_found");
    let output = tasks.call(
        "add",
        &[&flags[..], &["--parent", "/repos/work/tasks/plan.md"]].concat(),
    );
    assert_eq!(output.unwrap()["path"], "/repos/work/tasks/child.md");
    let text = std::fs::read_to_string(work.join("tasks/child.md")).unwrap();
    assert!(text.contains("parent: \"/tasks/plan.md\"\n"), "{text}");
}
