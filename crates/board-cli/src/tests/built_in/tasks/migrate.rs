//! `migrate`: the v2 tasks that are not projects, listed and then rewritten
//! as v3, one bulk commit per repo.

use super::*;

/// The board committing puts, holding `files`, committed.
fn committed(files: &[(&str, String)]) -> Tasks {
    let tasks = Tasks::new();
    tasks.configure(true);
    for (path, text) in files {
        tasks.write(path, text);
    }
    git(tasks.board.workdir(), &["add", "."]);
    git(tasks.board.workdir(), &["commit", "-qm", "fixture"]);
    tasks
}

/// How many commits the board's HEAD holds.
fn commits(tasks: &Tasks) -> String {
    git(tasks.board.workdir(), &["rev-list", "--count", "HEAD"])
}

#[test]
fn migrate_lists_then_rewrites_each_v2_task_in_one_bulk_commit_and_a_second_run_finds_nothing() {
    let typed = task(
        "Typed",
        "open",
        &json!({"kind": "task"}),
        "\nKeep this body.\n",
    );
    let plain = task("Plain", "done", &json!({"due": "2026-12-01"}), "");
    let tasks = committed(&[("tasks/typed.md", typed.clone()), ("tasks/plain.md", plain)]);
    let before = commits(&tasks);
    let listed = tasks.call("migrate", &[]).unwrap();
    let root = &listed["repos"][0];
    assert_eq!(
        (
            &root["repo"],
            &root["total"],
            &listed["wrote"],
            &listed["unchanged"]
        ),
        (&json!("/"), &json!(2), &json!(false), &json!(false))
    );
    let paths: Vec<&Value> = (root["tasks"].as_array().unwrap().iter())
        .map(|each| &each["path"])
        .collect();
    assert_eq!(
        paths,
        [&json!("/tasks/plain.md"), &json!("/tasks/typed.md")]
    );
    assert_eq!(root["tasks"][1]["record"]["$type"], "tasks/task/v3");
    assert_eq!(root["tasks"][1]["record"].get("kind"), None);
    assert_eq!(commits(&tasks), before);
    assert_eq!(tasks.read("tasks/typed.md"), typed);

    let wrote = tasks.call("migrate", &["--yes"]).unwrap();
    assert_eq!(wrote["wrote"], true);
    let after: u32 = commits(&tasks).parse().unwrap();
    assert_eq!(after, before.parse::<u32>().unwrap() + 1);
    let message = git(tasks.board.workdir(), &["log", "-1", "--format=%B"]);
    assert!(
        message.contains("Board-Bulk: tasks migrate to tasks/task/v3"),
        "{message}"
    );
    assert_eq!(
        tasks.read("tasks/typed.md"),
        "---\n$type: \"tasks/task/v3\"\ntitle: \"Typed\"\nstatus: \"open\"\n---\n\nKeep this body.\n"
    );
    assert!(
        tasks
            .read("tasks/plain.md")
            .contains("$type: \"tasks/task/v3\"\n")
    );
    let checked = tasks.run(&["check"], 0, None, "");
    assert_eq!(checked.code, 0, "{}", text(&checked.stdout));

    let again = tasks.call("migrate", &["--yes"]).unwrap();
    assert_eq!(
        (
            &again["unchanged"],
            &again["wrote"],
            &again["repos"][0]["total"]
        ),
        (&json!(true), &json!(false), &json!(0))
    );
    assert_eq!(commits(&tasks), after.to_string());
}

#[test]
fn migrate_yes_refuses_while_a_project_task_remains_and_writes_nothing() {
    let project = task("Ship", "open", &json!({"kind": "project"}), "");
    let child = open("Part", json!({"parent": "/tasks/ship.md"}));
    let tasks = committed(&[("tasks/ship.md", project), ("tasks/part.md", child.clone())]);
    let before = commits(&tasks);
    let listed = tasks.call("migrate", &[]).unwrap();
    assert_eq!(
        (
            &listed["projects"],
            &listed["projects_total"],
            &listed["repos"][0]["total"]
        ),
        (&json!(["/tasks/ship.md"]), &json!(1), &json!(1))
    );
    let refused = tasks.refused("migrate", &["--yes"]);
    assert_eq!(refused["code"], "tasks::migrate::project_task", "{refused}");
    assert_eq!(commits(&tasks), before);
    assert_eq!(tasks.read("tasks/part.md"), child);
}

#[test]
fn migrate_yes_rewrites_a_notes_repos_tasks_and_pushes_them() {
    let tasks = Tasks::new();
    let clone = tasks.mounted();
    let wrote = tasks.call("migrate", &["--yes"]).unwrap();
    let work = (wrote["repos"].as_array().unwrap().iter())
        .find(|repo| repo["repo"] == "/repos/work")
        .unwrap();
    assert_eq!(
        (&work["total"], &work["unpushed"]),
        (&json!(1), &Value::Null),
        "{wrote}"
    );
    let remote = tasks.beside.path().join("work.git");
    let message = git(&remote, &["log", "-1", "--format=%B"]);
    assert!(message.contains("Board-Bulk:"), "{message}");
    let shown = git(&remote, &["show", "main:tasks/report.md"]);
    assert!(shown.contains("$type: \"tasks/task/v3\""), "{shown}");
    assert_eq!(
        git(&clone, &["rev-parse", "HEAD"]),
        git(&remote, &["rev-parse", "main"])
    );
}

#[test]
fn migrate_lists_the_questions_and_artifacts_that_name_a_task_it_rewrites_and_not_another() {
    let tasks = Tasks::new();
    for plugin in ["plans", "artifacts"] {
        let enabled = tasks.run(&["plugin", "enable", plugin], 0, None, "");
        assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
    }
    tasks.write("tasks/typed.md", &open("Typed", json!({})));
    tasks.write(
        "tasks/ship.md",
        &task("Ship", "open", &json!({"kind": "project"}), ""),
    );
    let question = |blocks: &str| {
        json!({"$type": "plans/question/v1", "question": "Which?", "blocks": blocks}).to_string()
    };
    tasks.write("questions/typed.json", &question("/tasks/typed.md"));
    tasks.write("questions/ship.json", &question("/tasks/ship.md"));
    let artifact = json!({"$type": "artifacts/artifact/v1", "subject": ["/tasks/typed.md"]});
    tasks.write("artifacts/page.json", &artifact.to_string());
    let listed = tasks.call("migrate", &[]).unwrap();
    assert_eq!(
        (&listed["naming"], &listed["naming_total"]),
        (
            &json!([
                {"path": "/artifacts/page", "task": "/tasks/typed.md", "code": "board::ref::wrong_type"},
                {"path": "/questions/typed", "task": "/tasks/typed.md", "code": "board::ref::wrong_type"},
            ]),
            &json!(2)
        ),
        "{listed}"
    );
}

#[test]
fn migrate_yes_reports_a_repo_whose_rewrite_is_refused_and_still_rewrites_the_others() {
    let tasks = Tasks::new();
    let clone = tasks.mounted();
    let odd = task("Odd", "open", &json!({"colour": "red"}), "");
    tasks.write("tasks/odd.md", &odd);
    git(tasks.board.workdir(), &["add", "."]);
    git(tasks.board.workdir(), &["commit", "-qm", "odd"]);
    let wrote = tasks.call("migrate", &["--yes"]).unwrap();
    let repo = |root: &str| {
        (wrote["repos"].as_array().unwrap().iter())
            .find(|repo| repo["repo"] == root)
            .unwrap()
            .clone()
    };
    let own = repo("/");
    assert_eq!(own["refused"]["code"], "tasks::board::refused", "{wrote}");
    assert_eq!(own["commit"], Value::Null, "{wrote}");
    assert_eq!(tasks.read("tasks/odd.md"), odd);
    let work = repo("/repos/work");
    assert_eq!(
        (&work["refused"], &wrote["wrote"]),
        (&Value::Null, &json!(true)),
        "{wrote}"
    );
    let remote = tasks.beside.path().join("work.git");
    let shown = git(&remote, &["show", "main:tasks/report.md"]);
    assert!(shown.contains("$type: \"tasks/task/v3\""), "{shown}");
    assert_eq!(
        git(&clone, &["rev-parse", "HEAD"]),
        git(&remote, &["rev-parse", "main"])
    );
}

#[test]
fn migrate_yes_reports_a_notes_repo_one_mount_deeper_as_refused_and_rewrites_the_boards_own() {
    let tasks = committed(&[("tasks/own.md", open("Own", json!({})))]);
    let deeper = open("Deeper", json!({}));
    tasks.notes_repo("x", None, &[("deeper", &deeper)], true);
    let pointer = tasks.read("repos/x.json");
    git(tasks.board.workdir(), &["add", "repos/x.json"]);
    git(tasks.board.workdir(), &["rm", "-qf", "repos/x.json"]);
    let team = Scenario::new();
    git(team.workdir(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(team.workdir(), &["config", "user.name", "t"]);
    git(team.workdir(), &["config", "user.email", "t@t"]);
    team.write(
        ".board/board.json",
        Item::Fresh(r#"{"$type": "board/config/v1", "commit_on_put": true}"#),
    )
    .write("repos/x.json", Item::Fresh(&pointer));
    git(team.workdir(), &["add", "."]);
    git(team.workdir(), &["commit", "-qm", "team"]);
    tasks.mount("team", team.workdir());
    git(tasks.board.workdir(), &["add", "."]);
    git(tasks.board.workdir(), &["commit", "-qm", "mounts"]);
    let listed = tasks.call("migrate", &[]).unwrap();
    let roots: Vec<&Value> = (listed["repos"].as_array().unwrap().iter())
        .map(|repo| &repo["repo"])
        .collect();
    assert_eq!(roots, [&json!("/"), &json!("/repos/team/repos/x")]);
    let wrote = tasks.call("migrate", &["--yes"]).unwrap();
    assert_eq!(
        (&wrote["repos"][0]["refused"], &wrote["wrote"]),
        (&Value::Null, &json!(true)),
        "{wrote}"
    );
    let deeper_repo = &wrote["repos"][1];
    assert_eq!(
        deeper_repo["refused"]["code"], "tasks::board::refused",
        "{wrote}"
    );
    assert_eq!(deeper_repo["commit"], Value::Null, "{wrote}");
    assert!(tasks.read("tasks/own.md").contains("tasks/task/v3"));
}
