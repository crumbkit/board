//! `--owner`, the one owner flag a released Board's `claim` and
//! `available` took, which each still accept beside `--session`,
//! `--program`, `--person` and `--agent`. It reads its text as the shapes
//! migration reads an old owner, so the two agree.

use super::*;

/// `--owner` names the owner the shapes migration makes of the same text: a
/// `local_` id is the session, Luke is the person, and any other words are
/// the agent's name, kept whole.
#[test]
fn owner_names_the_owner_the_shapes_migration_makes_of_the_same_text() {
    let tasks = Tasks::new();
    let cases = [
        ("local_0a1b", json!({"session": "local_0a1b"})),
        ("luke", json!({"person": "luke"})),
        ("lead-1", json!({"agent": "lead-1"})),
        (
            "Codex recording builder",
            json!({"program": "codex", "agent": "Codex recording builder"}),
        ),
    ];
    for (at, (owner, expected)) in cases.into_iter().enumerate() {
        let path = format!("/tasks/{at}.md");
        tasks.write(&path[1..], &open("A", json!({})));
        let output = tasks.call("claim", &["--path", path.as_str(), "--owner", owner]);
        assert_eq!(output.unwrap()["owner"], expected, "{owner}");
    }
}

/// A task a released Board's `claim --owner lead-1` wrote holds the owner
/// the migration made of it, and the same `--owner` claims it again and
/// lists it, while another name is refused.
#[test]
fn a_migrated_owner_is_claimed_and_listed_by_the_same_owner_text_and_refused_to_another() {
    let tasks = Tasks::new();
    let migrated = json!({"agent": "lead-1"});
    tasks.write("tasks/a.md", &open("A", json!({"owner": migrated})));
    let path = ["--path", "/tasks/a.md"];
    let output = tasks.call("claim", &[&path[..], &["--owner", "lead-1"]].concat());
    assert_eq!(output.unwrap()["owner"], migrated);
    let listed = tasks.call("available", &["--owner", "lead-1"]).unwrap();
    assert_eq!(listed["available"]["tasks"][0]["title"], json!("A"));
    let refused = tasks.refused("claim", &[&path[..], &["--owner", "lead-2"]].concat());
    assert_eq!(refused["code"], "tasks::task::held");
}

/// `available --owner` lists what that session holds, as it did before
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
/// together are refused, and `--owner` alone is admitted.
#[test]
fn owner_with_another_owner_flag_is_refused_by_claim_and_available_and_owner_alone_is_admitted() {
    let tasks = Tasks::new();
    tasks.write("tasks/a.md", &open("A", json!({})));
    let both = ["--owner", "local_0a1b", "--program", "codex"];
    let claim = ["--path", "/tasks/a.md"];
    for (capability, flags) in [("available", &[][..]), ("claim", &claim[..])] {
        let refused = tasks.refused(capability, &[flags, &both].concat());
        assert_eq!(refused["code"], "tasks::owner::twice", "{capability}");
        let admitted = tasks.call(capability, &[flags, &both[..2]].concat());
        assert!(admitted.is_ok(), "{capability}: {admitted:?}");
    }
}

/// A personal notes repo holding one open task, committed and mounted, with
/// the tasks/task/v2 schema the plugin ships, or with the copy written
/// before the shapes migration, whose `owner` is text, when `stale`: the
/// board and the clone.
fn notes_with_task(stale: bool) -> (Tasks, PathBuf) {
    let tasks = Tasks::new();
    tasks.configure(true);
    let personal = json!({"covers": [], "personal": true});
    let task = open("Report", json!({}));
    let clone = tasks.notes_repo("personal", Some(personal), &[("report", &task)], true);
    if stale {
        let mut schema: Value = serde_json::from_str(SCHEMA).unwrap();
        schema["properties"]["owner"] = json!({"type": "string", "minLength": 1});
        let file = clone.join(".board/types/tasks/task/v2.schema.json");
        write_file(&file, &schema.to_string());
        git(
            &clone,
            &["commit", "-qam", "the schema before the shapes migration"],
        );
        git(&clone, &["push", "-q", "origin", "main"]);
        tasks.mount("personal", &clone);
    }
    (tasks, clone)
}

const REPORT: &str = "/repos/personal/tasks/report.md";
const SESSION: &str = "local_0a1b";

/// A notes repo that still stores the tasks/task/v2 schema from before the
/// shapes migration would refuse the Actor a claim writes as its owner, so
/// the claim is refused as stale before it syncs or writes, and says how to
/// migrate the clone. It proves the claim stops and names the remedy; the
/// migration itself is the xtask's own tests'.
#[test]
fn a_claim_in_a_mounted_notes_repo_with_the_old_task_schema_is_refused_as_stale_and_writes_nothing()
{
    let (tasks, clone) = notes_with_task(true);
    let head = git(&clone, &["rev-parse", "HEAD"]);
    let before = std::fs::read_to_string(clone.join("tasks/report.md")).unwrap();
    let refused = tasks.refused("claim", &["--path", REPORT, "--session", SESSION]);
    assert_eq!(refused["code"], "tasks::repo::schema_stale", "{refused}");
    assert_eq!(refused["fields"]["path"], REPORT);
    assert_eq!(
        refused["fields"]["clone"],
        json!(clone.canonicalize().unwrap())
    );
    let said = refused.to_string();
    assert!(said.contains("migrate-shapes"), "{said}");
    assert!(said.contains("plugin enable tasks"), "{said}");
    assert!(said.contains("mount update"), "{said}");
    assert_eq!(git(&clone, &["rev-parse", "HEAD"]), head);
    let after = std::fs::read_to_string(clone.join("tasks/report.md")).unwrap();
    assert_eq!(after, before);
}

/// The twin: with the schema the plugin ships, the claim is stored as the
/// Actor, `board check` finds the task valid and `available` lists it as
/// held by the session.
#[test]
fn a_claim_in_a_mounted_notes_repo_with_the_shipped_task_schema_is_stored_and_listed_as_held() {
    let (tasks, clone) = notes_with_task(false);
    let output = tasks.call("claim", &["--path", REPORT, "--session", SESSION]);
    assert_eq!(output.unwrap()["owner"], json!({"session": SESSION}));
    let stored = std::fs::read_to_string(clone.join("tasks/report.md")).unwrap();
    assert!(
        stored.contains("owner: {\"session\":\"local_0a1b\"}\n"),
        "{stored}"
    );
    let report = json(&tasks.run(&["--json", "check", "--healthy", "--all"], 0, None, ""));
    let entry = (report["data"].as_array().unwrap().iter())
        .find(|entry| entry["logical"] == REPORT)
        .unwrap();
    assert_eq!(entry.get("code"), None, "{entry}");
    let listed = tasks.call("available", &["--session", SESSION]).unwrap();
    assert_eq!(listed["available"]["tasks"][0]["path"], REPORT, "{listed}");
}
