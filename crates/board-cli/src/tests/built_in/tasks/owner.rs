//! `--owner`, the one owner flag a released Board's `claim` and
//! `available` took: a session's id or a person's handle, which each
//! still accept beside `--session`, `--program`, `--person` and `--agent`.

use super::*;

/// A released caller's `--owner local_…` names a session and `--owner
/// luke` a person; any other text names an agent.
#[test]
fn owner_names_a_session_by_its_id_a_person_by_a_handle_and_otherwise_an_agent() {
    let tasks = Tasks::new();
    let cases = [
        ("local_0a1b", json!({"session": "local_0a1b"})),
        ("lead-1", json!({"person": "lead-1"})),
        ("owner@example.com", json!({"person": "owner@example.com"})),
        ("Inbox builder", json!({"agent": "Inbox builder"})),
    ];
    for (at, (owner, expected)) in cases.into_iter().enumerate() {
        let path = format!("/tasks/{at}.md");
        tasks.write(&path[1..], &open("A", json!({})));
        let output = tasks.call("claim", &["--path", path.as_str(), "--owner", owner]);
        assert_eq!(output.unwrap()["owner"], expected, "{owner}");
    }
}

/// `available --owner` lists what that owner holds, as it did before
/// owners were actors.
#[test]
fn available_by_owner_lists_the_tasks_that_session_holds_and_leaves_out_another_sessions() {
    let tasks = Tasks::new();
    let mine = json!({"session": "local_0a1b", "program": "claude-code"});
    tasks.write("tasks/mine.md", &open("Mine", json!({"owner": mine})));
    let theirs = json!({"session": "local_0c"});
    tasks.write("tasks/theirs.md", &open("Theirs", json!({"owner": theirs})));
    let listed = tasks.call("available", &["--owner", "local_0a1b"]).unwrap();
    let titles: Vec<&Value> = (listed["available"]["tasks"].as_array().unwrap().iter())
        .map(|task| &task["title"])
        .collect();
    assert_eq!(titles, [&json!("Mine")]);
}

/// `--owner` and the parts it stands for name the owner twice, so the two
/// together are refused.
#[test]
fn owner_with_another_owner_flag_is_refused_by_claim_and_available() {
    let tasks = Tasks::new();
    tasks.write("tasks/a.md", &open("A", json!({})));
    let both = ["--owner", "local_0a1b", "--program", "codex"];
    let claim = ["--path", "/tasks/a.md"];
    for (capability, flags) in [("available", &[][..]), ("claim", &claim[..])] {
        let refused = tasks.refused(capability, &[flags, &both].concat());
        assert_eq!(refused["code"], "tasks::owner::twice", "{capability}");
    }
}
