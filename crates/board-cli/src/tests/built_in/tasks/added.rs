//! What `add` says about where a task went: the remote its repo pushes
//! to, whether the call pushed it off this Mac, and the sentence that
//! says both; the reason `--repo /` needs; and the commands its routing
//! refusals give, each run as printed.

use super::write::write_hook;
use super::*;

const EXAMPLE_ORG: &str = "git@github.com:example-org/home.git";
/// A remote that holds a token, which no sentence may show.
const TOKENED: &str = "https://someone:not-a-real-token@github.com/example-org/home.git";

/// The remote of the notes repo `name`, as its clone's origin names it.
fn remote_of(tasks: &Tasks, name: &str) -> String {
    let remote = tasks.beside.path().join(format!("{name}.git"));
    remote.to_str().unwrap().to_owned()
}

/// The words of `command` as a POSIX shell splits them, for the plain and
/// single-quoted words a hint prints.
fn words(command: &str) -> Vec<String> {
    let (mut words, mut word, mut quoted, mut started) = (Vec::new(), String::new(), false, false);
    for character in command.chars() {
        match character {
            '\'' => (quoted, started) = (!quoted, true),
            ' ' if !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                }
                started = false;
            }
            other => (word, started) = (word + &other.to_string(), true),
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// The `board ...` commands a refusal's hint names, in backticks, each
/// as printed.
fn printed(refused: &Value) -> Vec<String> {
    let hint = refused["recovery"]["text"].as_str().unwrap();
    (hint.split('`').skip(1).step_by(2))
        .filter(|part| part.starts_with("board "))
        .map(str::to_owned)
        .collect()
}

/// The commands of [`printed`], each required to name `tasks`'s board with
/// `-C`, shown without it so a test can compare them.
fn commands(tasks: &Tasks, refused: &Value) -> Vec<String> {
    let board = tasks.board.workdir().canonicalize().unwrap();
    (printed(refused).iter())
        .map(|command| {
            let words = words(command);
            assert_eq!(words[..2], ["board", "-C"], "{command}");
            let named = Path::new(&words[2]).canonicalize().unwrap();
            assert_eq!(named, board, "{command}");
            let prefix = format!("board -C {} ", words[2]);
            let rest = command.strip_prefix(&prefix).unwrap();
            format!("board {rest}")
        })
        .collect()
}

/// Runs `command`, a hint's `board ...` line, word for word from `caller`
/// with nothing added, and gives what it printed, the call's output.
fn ran(tasks: &Tasks, command: &str, caller: &Path) -> Value {
    let words = words(command);
    let typed: Vec<&str> = words.iter().map(String::as_str).collect();
    let output = tasks.run_typed(&typed, 0, caller.to_path_buf(), "");
    assert_eq!(output.code, 0, "{command}: {}", text(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn a_task_pushed_to_a_notes_repo_names_the_remote_and_has_left_this_mac() {
    let tasks = Tasks::new();
    tasks.mounted();
    let output = tasks
        .call("add", &["--title", "Ship it", "--remote", EXAMPLE_ORG])
        .unwrap();
    let remote = remote_of(&tasks, "work");
    assert_eq!(
        (&output["remote"], &output["leaves_this_mac"]),
        (&json!(remote), &json!(true))
    );
    assert_eq!(
        output["about"],
        format!("Wrote /repos/work/tasks/ship-it.md to /repos/work and pushed it to {remote}.")
    );
}

#[test]
fn a_task_whose_push_fails_has_not_left_this_mac_and_the_sentence_gives_the_code() {
    let tasks = Tasks::new();
    tasks.mounted();
    let hook = tasks.beside.path().join("work.git/hooks/pre-receive");
    write_hook(&hook);
    let output = tasks
        .call("add", &["--title", "Ship it", "--remote", EXAMPLE_ORG])
        .unwrap();
    let code = output["unpushed"].as_str().unwrap();
    assert_eq!(
        (&output["remote"], &output["leaves_this_mac"]),
        (&json!(remote_of(&tasks, "work")), &json!(false))
    );
    assert_eq!(
        output["about"],
        format!(
            "Wrote /repos/work/tasks/ship-it.md to /repos/work, but the push failed with {code}, \
             so the task is only on this Mac until the next sync."
        )
    );
}

#[test]
fn repo_root_without_a_reason_is_refused_naming_the_personal_repo_and_with_one_is_written() {
    let tasks = Tasks::new();
    tasks.mounted();
    let refused = tasks.refused("add", &["--title", "Queue it", "--repo", "/"]);
    assert_eq!(refused["code"], "tasks::repo::reason_needed");
    let remote = remote_of(&tasks, "personal");
    assert_eq!(
        (&refused["fields"]["personal"], &refused["fields"]["remote"]),
        (&json!("/repos/personal"), &json!(remote))
    );
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.contains(&format!("/repos/personal, which pushes to {remote}")),
        "{message}"
    );
    let hinted = commands(&tasks, &refused);
    assert_eq!(
        hinted,
        [
            "board tasks add --title='Queue it' --repo /repos/personal",
            "board tasks add --title='Queue it' --repo / --reason '<why it belongs with this \
             board>'",
        ]
    );
    assert!(!tasks.board.workdir().join("tasks/queue-it.md").exists());
    let flags = [
        "--title",
        "Queue it",
        "--repo",
        "/",
        "--reason",
        "a queue item for the lead",
    ];
    let output = tasks.call("add", &flags).unwrap();
    assert_eq!(
        (
            &output["path"],
            &output["remote"],
            &output["leaves_this_mac"]
        ),
        (&json!("/tasks/queue-it.md"), &Value::Null, &json!(false))
    );
    assert_eq!(
        output["about"],
        "Wrote /tasks/queue-it.md to this board's own tree. tasks add does not push it, so it \
         stays on this Mac unless something else pushes this board. The reason given: a queue \
         item for the lead."
    );
    assert!(!tasks.read("tasks/queue-it.md").contains("queue item"));
}

#[test]
fn repo_root_without_a_reason_in_a_board_with_no_personal_repo_names_none() {
    let tasks = Tasks::new();
    let refused = tasks.refused("add", &["--title", "Queue it", "--repo", "/"]);
    assert_eq!(refused["code"], "tasks::repo::reason_needed");
    assert_eq!(
        (&refused["fields"]["personal"], &refused["fields"]["remote"]),
        (&Value::Null, &Value::Null)
    );
    let hinted = commands(&tasks, &refused);
    assert_eq!(hinted.len(), 1, "{hinted:?}");
    let output = ran(&tasks, &printed(&refused)[0], tasks.outside.path());
    assert_eq!(output["path"], "/tasks/queue-it.md");
}

#[test]
fn a_task_routed_to_the_boards_own_tree_needs_no_reason_and_says_no_notes_repo_took_it() {
    let tasks = Tasks::new();
    let output = tasks
        .call("add", &["--title", "Call the bank", "--remote", TOKENED])
        .unwrap();
    assert_eq!(
        (
            &output["path"],
            &output["remote"],
            &output["leaves_this_mac"]
        ),
        (
            &json!("/tasks/call-the-bank.md"),
            &Value::Null,
            &json!(false)
        )
    );
    assert_eq!(
        output["about"],
        "Wrote /tasks/call-the-bank.md to this board's own tree, because no notes repo covers \
         example-org/home and the board lists no personal notes repo. tasks add does not push \
         it, so it stays on this Mac unless something else pushes this board."
    );
    let outside = tasks.call("add", &["--title", "Buy milk"]).unwrap();
    assert!(
        (outside["about"].as_str().unwrap())
            .contains(", because the call gave no git remote and the board lists no personal"),
        "{outside}"
    );
}

#[test]
fn the_origin_unread_hint_names_the_personal_repo_first_and_each_command_runs_as_printed() {
    let tasks = Tasks::new();
    tasks.mounted();
    let checkout = tasks.checkout();
    let refused = tasks
        .call_with("add", &["--title", "Ship it"], 0, &checkout, "")
        .unwrap_err();
    assert_eq!(refused["code"], "tasks::repo::origin_unread");
    let hinted = commands(&tasks, &refused);
    assert_eq!(
        hinted,
        [
            "board tasks add --title='Ship it' --repo /repos/personal",
            "board tasks add --title='Ship it' --repo / --reason '<why it belongs with this \
             board>'",
        ]
    );
    let printed = printed(&refused);
    let first = ran(&tasks, &printed[0], &checkout);
    assert_eq!(first["path"], "/repos/personal/tasks/ship-it.md");
    let second = ran(&tasks, &printed[1], &checkout);
    assert_eq!(second["path"], "/tasks/ship-it.md");
}

#[test]
fn the_ambiguous_hint_gives_a_command_for_each_candidate_and_each_runs_as_printed() {
    let tasks = Tasks::new();
    tasks.mounted();
    let covers = json!({"covers": ["example-org/home"]});
    tasks.notes_repo("more", Some(covers), &[], true);
    let refused = tasks.refused("add", &["--title", "Ship it", "--remote", EXAMPLE_ORG]);
    assert_eq!(refused["code"], "tasks::repo::ambiguous");
    let hinted = commands(&tasks, &refused);
    assert_eq!(
        hinted,
        [
            "board tasks add --title='Ship it' --repo /repos/more",
            "board tasks add --title='Ship it' --repo /repos/work",
        ]
    );
    let paths: Vec<Value> = (printed(&refused).iter())
        .map(|command| ran(&tasks, command, tasks.outside.path())["path"].clone())
        .collect();
    assert_eq!(
        paths,
        [
            json!("/repos/more/tasks/ship-it.md"),
            json!("/repos/work/tasks/ship-it.md")
        ]
    );
}

#[test]
fn a_reason_that_is_only_spaces_is_missing_and_one_with_words_is_admitted() {
    let tasks = Tasks::new();
    let given = |reason: &str| {
        let flags = ["--title", "Queue it", "--repo", "/", "--reason", reason];
        tasks.call("add", &flags)
    };
    let refused = given("   ").unwrap_err();
    assert_eq!(refused["code"], "tasks::repo::reason_needed");
    assert!(!tasks.board.workdir().join("tasks/queue-it.md").exists());
    let output = given(" a queue item ").unwrap();
    assert!(
        (output["about"].as_str().unwrap()).ends_with(" The reason given: a queue item."),
        "{output}"
    );
}

#[test]
fn a_reason_given_for_a_notes_repo_is_ignored_and_one_for_the_boards_tree_is_printed() {
    let tasks = Tasks::new();
    tasks.mounted();
    let flags = [
        "--title",
        "Ship it",
        "--repo",
        "/repos/work",
        "--reason",
        "a queue item",
    ];
    let output = tasks.call("add", &flags).unwrap();
    assert_eq!(output["path"], "/repos/work/tasks/ship-it.md");
    let about = output["about"].as_str().unwrap();
    assert!(!about.contains("reason"), "{about}");
    let flags = [
        "--title",
        "Ship it",
        "--repo",
        "/",
        "--reason",
        "a queue item",
    ];
    let output = tasks.call("add", &flags).unwrap();
    let about = output["about"].as_str().unwrap();
    assert!(
        about.ends_with(" The reason given: a queue item."),
        "{about}"
    );
}

#[test]
fn every_routing_hint_says_to_add_the_calls_other_flags() {
    let tasks = Tasks::new();
    tasks.mounted();
    let checkout = tasks.checkout();
    let unread = tasks
        .call_with("add", &["--title", "Ship it"], 0, &checkout, "")
        .unwrap_err();
    let reason = tasks.refused("add", &["--title", "Ship it", "--repo", "/"]);
    tasks.notes_repo(
        "more",
        Some(json!({"covers": ["example-org/home"]})),
        &[],
        true,
    );
    let ambiguous = tasks.refused("add", &["--title", "Ship it", "--remote", EXAMPLE_ORG]);
    for refused in [unread, reason, ambiguous] {
        let hint = refused["recovery"]["text"].as_str().unwrap();
        assert!(hint.ends_with(", with the call's other flags"), "{hint}");
    }
}

#[test]
fn a_title_that_starts_with_a_dash_runs_as_printed_in_a_hint() {
    let tasks = Tasks::new();
    tasks.mounted();
    let checkout = tasks.checkout();
    let stdin = json!({"title": "-v is broken"}).to_string();
    let refused = tasks
        .call_with("add", &[], 0, &checkout, &stdin)
        .unwrap_err();
    assert_eq!(refused["code"], "tasks::repo::origin_unread");
    let printed = printed(&refused);
    assert!(
        printed[0].contains(" --title='-v is broken' "),
        "{printed:?}"
    );
    let output = ran(&tasks, &printed[0], &checkout);
    let path = output["path"].as_str().unwrap();
    assert!(path.starts_with("/repos/personal/tasks/"), "{path}");
}

/// Mounts a board at `/repos/team` that mounts each notes repo of `names`
/// again, at `/repos/team/repos/<name>`, pinned one commit behind the
/// board's own mount, so each is reached through two mounts that Board
/// lists apart. Board indexes a commit once, so two mounts pinning one
/// commit list once. Commits the board, and gives the team board's clone,
/// which the test keeps.
#[must_use]
fn mounted_twice(tasks: &Tasks, names: &[&str]) -> Scenario {
    let team = Scenario::new();
    git(team.workdir(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(team.workdir(), &["config", "user.name", "t"]);
    git(team.workdir(), &["config", "user.email", "t@t"]);
    team.write(
        ".board/board.json",
        Item::Fresh(r#"{"$type": "board/config/v1", "commit_on_put": true}"#),
    );
    for name in names {
        let pointer = tasks.read(&format!("repos/{name}.json"));
        team.write(&format!("repos/{name}.json"), Item::Fresh(&pointer));
        let pointer: Value = serde_json::from_str(&pointer).unwrap();
        let clone = Path::new(pointer["repo"].as_str().unwrap());
        write_file(&clone.join("later.txt"), "later\n");
        git(clone, &["add", "later.txt"]);
        git(clone, &["commit", "-qm", "later"]);
        git(clone, &["push", "-q", "origin", "main"]);
        tasks.mount(name, clone);
    }
    git(team.workdir(), &["add", "."]);
    git(team.workdir(), &["commit", "-qm", "team"]);
    tasks.mount("team", team.workdir());
    git(tasks.board.workdir(), &["add", "."]);
    git(tasks.board.workdir(), &["commit", "-qm", "mounts"]);
    team
}

#[test]
fn a_notes_repo_two_mounts_reach_takes_the_task_at_its_shorter_mount() {
    let tasks = Tasks::new();
    tasks.mounted();
    let _team = mounted_twice(&tasks, &["personal", "work"]);
    let routed = tasks
        .call("add", &["--title", "Ship it", "--remote", EXAMPLE_ORG])
        .unwrap();
    assert_eq!(routed["path"], "/repos/work/tasks/ship-it.md", "{routed}");
    let about = routed["about"].as_str().unwrap();
    assert!(
        about.starts_with("Wrote /repos/work/tasks/ship-it.md to /repos/work "),
        "{about}"
    );
    let personal = tasks.call("add", &["--title", "Call home"]).unwrap();
    assert_eq!(
        personal["path"], "/repos/personal/tasks/call-home.md",
        "{personal}"
    );
}

#[test]
fn two_different_notes_repos_covering_the_remote_still_refuse_when_one_is_mounted_twice() {
    let tasks = Tasks::new();
    tasks.mounted();
    let covers = json!({"covers": ["example-org/home"]});
    tasks.notes_repo("more", Some(covers), &[], true);
    let _team = mounted_twice(&tasks, &["work"]);
    let refused = tasks.refused("add", &["--title", "Ship it", "--remote", EXAMPLE_ORG]);
    assert_eq!(refused["code"], "tasks::repo::ambiguous");
    assert_eq!(
        refused["fields"]["candidates"],
        json!(["/repos/more", "/repos/work"]),
        "{refused}"
    );
}
