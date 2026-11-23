//! `refs` on a `tasks/task/v3` task: `add` writes them, `ref` adds and
//! removes one, and `board check` holds them to the task's own board.

use super::*;

/// A v3 task titled `title`, its frontmatter's `fields` as written, then
/// `body`.
fn v3(title: &str, fields: &str, body: &str) -> String {
    format!("---\n$type: tasks/task/v3\ntitle: {title}\nstatus: open\n{fields}---\n{body}")
}

/// The board's HEAD.
fn head(tasks: &Tasks) -> String {
    git(tasks.board.workdir(), &["rev-parse", "HEAD"])
}

/// The board committing puts, holding the files `paths`, committed.
fn committing(paths: &[&str]) -> Tasks {
    let tasks = Tasks::new();
    tasks.configure(true);
    for path in paths {
        tasks.write(path, "# A message\n");
    }
    git(tasks.board.workdir(), &["add", "."]);
    git(tasks.board.workdir(), &["commit", "-qm", "fixture"]);
    tasks
}

#[test]
fn add_with_refs_writes_a_v3_task_whose_refs_board_refs_lists() {
    let tasks = committing(&["exchange/sent/a.md"]);
    let stdin = json!({"title": "Wait on the lead", "refs": ["/exchange/sent/a.md"]}).to_string();
    let output = (tasks.call_with("add", &[], 0, tasks.outside.path(), &stdin)).unwrap();
    assert_eq!(output["path"], "/tasks/wait-on-the-lead.md");
    let written = tasks.read("tasks/wait-on-the-lead.md");
    assert!(
        written.starts_with("---\n$type: \"tasks/task/v3\"\n"),
        "{written}"
    );
    assert!(
        written.contains("\nrefs: [\"/exchange/sent/a.md\"]\n"),
        "{written}"
    );
    let refs = json(&tasks.run(
        &["--json", "refs", "/tasks/wait-on-the-lead.md"],
        0,
        None,
        "",
    ));
    let from = &refs["data"][0]["from"];
    assert_eq!(from[0]["pointer"], "/refs/0", "{refs}");
    assert_eq!(from[0]["target"], "/exchange/sent/a.md", "{refs}");
    let listed = tasks.call("available", &["--repo", "/"]).unwrap();
    let row = &listed["available"]["tasks"][0];
    assert_eq!(
        (&row["refs"], &row["refs_total"]),
        (&json!(["/exchange/sent/a.md"]), &json!(1))
    );
}

#[test]
fn ref_add_appends_once_keeps_the_body_and_remove_drops_it() {
    let tasks = committing(&["exchange/sent/a.md", "exchange/sent/b.md"]);
    let body = "\nWait for the reply.\n\n- keep this line\n";
    tasks.write("tasks/wait.md", &v3("Wait", "", body));
    git(tasks.board.workdir(), &["add", "."]);
    git(tasks.board.workdir(), &["commit", "-qm", "task"]);
    let call = |flags: &[&str]| {
        let mut every = vec!["--path", "/tasks/wait.md"];
        every.extend(flags);
        tasks.call("ref", &every).unwrap()
    };
    let first = call(&["--add", "/exchange/sent/a.md"]);
    assert_eq!(
        (&first["refs"], &first["unchanged"]),
        (&json!(["/exchange/sent/a.md"]), &json!(false))
    );
    let second = call(&["--add", "/exchange/sent/b.md"]);
    assert_eq!(
        second["refs"],
        json!(["/exchange/sent/a.md", "/exchange/sent/b.md"])
    );
    let text = tasks.read("tasks/wait.md");
    assert!(text.ends_with(&format!("---\n{body}")), "{text}");
    assert!(text.contains("\nreviewed: "), "{text}");
    let before = head(&tasks);
    let again = call(&["--add", "/exchange/sent/b.md"]);
    assert_eq!(
        (&again["unchanged"], &again["commit"]),
        (&json!(true), &Value::Null)
    );
    assert_eq!(head(&tasks), before);
    assert_eq!(tasks.read("tasks/wait.md"), text);
    let removed = call(&["--remove", "/exchange/sent/a.md"]);
    assert_eq!(removed["refs"], json!(["/exchange/sent/b.md"]));
    let emptied = call(&["--remove", "/exchange/sent/b.md"]);
    assert_eq!(emptied["refs"], json!([]));
    let text = tasks.read("tasks/wait.md");
    assert!(!text.contains("refs:"), "{text}");
    assert!(text.ends_with(&format!("---\n{body}")), "{text}");
}

#[test]
fn ref_refuses_a_missing_path_a_path_through_a_mount_and_a_v2_task_writing_nothing() {
    let tasks = committing(&["exchange/sent/a.md"]);
    tasks.notes_repo(
        "personal",
        Some(json!({"covers": [], "personal": true})),
        &[],
        true,
    );
    tasks.write("tasks/wait.md", &v3("Wait", "", ""));
    tasks.write("tasks/old.md", &open("Old", json!({})));
    git(tasks.board.workdir(), &["add", "."]);
    git(tasks.board.workdir(), &["commit", "-qm", "tasks"]);
    let before = head(&tasks);
    let cases = [
        (
            "/tasks/wait.md",
            "/exchange/sent/none.md",
            "tasks::ref::missing",
        ),
        (
            "/tasks/wait.md",
            "/repos/personal/repo.txt",
            "tasks::ref::through_mount",
        ),
        ("/tasks/old.md", "/exchange/sent/a.md", "tasks::ref::not_v3"),
    ];
    for (task, add, code) in cases {
        let refused = tasks.refused("ref", &["--path", task, "--add", add]);
        assert_eq!(refused["code"], code, "{refused}");
        assert_eq!(head(&tasks), before, "{code}");
    }
    let admitted = tasks.call(
        "ref",
        &["--path", "/tasks/wait.md", "--add", "/exchange/sent/a.md"],
    );
    assert_eq!(admitted.unwrap()["refs"], json!(["/exchange/sent/a.md"]));
}

#[test]
fn check_degrades_a_v3_ref_through_a_mount_and_still_lists_the_task_while_a_v2_brief_stays_ok() {
    let tasks = Tasks::new();
    tasks.notes_repo(
        "personal",
        Some(json!({"covers": [], "personal": true})),
        &[],
        true,
    );
    tasks.write(
        "tasks/through.md",
        &v3("Through", "refs: [\"/repos/personal/repo.txt\"]\n", ""),
    );
    tasks.write(
        "tasks/brief.md",
        &open("Brief", json!({"brief": "/repos/personal/repo.txt"})),
    );
    let report = json(&tasks.run(&["--json", "check", "--healthy", "--all"], 0, None, ""));
    let code = |path: &str| {
        (report["data"].as_array().unwrap().iter())
            .find(|entry| entry["logical"] == path)
            .map(|entry| entry.get("code").cloned().unwrap_or(Value::Null))
            .unwrap()
    };
    assert_eq!(code("/tasks/through.md"), "board::ref::through_mount");
    assert_eq!(code("/tasks/brief.md"), Value::Null);
    let listed = tasks.call("available", &["--repo", "/"]).unwrap();
    assert_eq!(shown(&listed), ["Brief", "Through"]);
    assert_eq!(listed["degraded"], 0);
}
