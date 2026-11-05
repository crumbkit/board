//! `tasks.available` through the binary: sections, scope by remote, sync,
//! degraded files and what blocks a task. These were `test_available.py`
//! and `test_blockers.py`. Dates are counted from today in UTC, at least
//! three days apart, so a call that crosses midnight answers the same.

use super::*;

/// The board with a task of its own, a personal notes repo, a work one
/// covering example-org/* and another covering acme/*.
fn estate() -> Tasks {
    let tasks = Tasks::new();
    let recent = json!({"reviewed": day(0)});
    tasks.write("tasks/board.md", &open("Board task", recent.clone()));
    let home = open("Home task", recent.clone());
    let personal = json!({"covers": [], "personal": true});
    tasks.notes_repo("personal", Some(personal), &[("home", &home)], true);
    let work = open("Work task", recent.clone());
    let covers = json!({"covers": ["example-org/*"]});
    tasks.notes_repo("work", Some(covers), &[("work", &work)], true);
    let acme = open("Acme task", recent);
    tasks.notes_repo(
        "other",
        Some(json!({"covers": ["acme/*"]})),
        &[("acme", &acme)],
        true,
    );
    tasks
}

/// `fields` with `reviewed` today.
fn reviewed(mut fields: Value) -> Value {
    fields["reviewed"] = json!(day(0));
    fields
}

/// Commits `files` in `clone` and pushes them.
fn push(clone: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        write_file(&clone.join(path), text);
    }
    git(clone, &["add", "."]);
    git(clone, &["commit", "-qm", "from the phone"]);
    git(clone, &["push", "-q"]);
}

#[test]
fn available_lists_open_tasks_neither_deferred_nor_blocked_by_title() {
    let tasks = Tasks::new();
    tasks.write("tasks/b.md", &open("B", reviewed(json!({}))));
    tasks.write(
        "tasks/a.md",
        &open("A", reviewed(json!({"defer": day(-3)}))),
    );
    tasks.write(
        "tasks/later.md",
        &open("Later", reviewed(json!({"defer": day(30)}))),
    );
    tasks.write(
        "tasks/soon.md",
        &open("Soon", reviewed(json!({"defer": "soon"}))),
    );
    tasks.write("tasks/done.md", &task("Done", "done", &json!({}), ""));
    tasks.write(
        "tasks/dropped.md",
        &task("Dropped", "dropped", &json!({}), ""),
    );
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(shown(&output), ["A", "B"]);
    assert_eq!(
        (&output["repos"], &output["degraded"], &output["unsynced"]),
        (&json!(["/"]), &json!(0), &json!([]))
    );
}

#[test]
fn a_task_waits_while_a_task_in_its_after_is_not_done_and_the_blocker_counts_it() {
    let tasks = Tasks::new();
    tasks.write("tasks/blocker.md", &open("Blocker", reviewed(json!({}))));
    let after = reviewed(json!({"after": ["/tasks/blocker.md", "/tasks/gone.md"]}));
    tasks.write("tasks/waiter.md", &open("Waiter", after));
    tasks.write(
        "tasks/finished.md",
        &task("Finished", "done", &json!({}), ""),
    );
    let released = reviewed(json!({"after": ["/tasks/finished.md"]}));
    tasks.write("tasks/released.md", &open("Released", released));
    let output = tasks.call("available", &[]).unwrap();
    let found: BTreeMap<String, (Value, Value)> =
        (output["available"]["tasks"].as_array().unwrap().iter())
            .map(|item| {
                let title = item["title"].as_str().unwrap().to_owned();
                (title, (item["waiting"].clone(), item["blocked_by"].clone()))
            })
            .collect();
    let expected = BTreeMap::from([
        ("Blocker".to_owned(), (json!(1), json!([]))),
        ("Released".to_owned(), (json!(0), json!([]))),
    ]);
    assert_eq!(found, expected);
    tasks.write("tasks/blocker.md", &task("Blocker", "done", &json!({}), ""));
    let after = reviewed(json!({"after": ["/tasks/blocker.md"]}));
    tasks.write("tasks/waiter.md", &open("Waiter", after));
    assert_eq!(
        shown(&tasks.call("available", &[]).unwrap()),
        ["Released", "Waiter"]
    );
}

#[test]
fn due_soon_and_stale_are_sections_of_their_own() {
    let tasks = Tasks::new();
    let due = reviewed(json!({"due": day(3), "defer": day(30)}));
    tasks.write("tasks/due.md", &open("Due", due));
    tasks.write(
        "tasks/far.md",
        &open("Far", reviewed(json!({"due": day(30)}))),
    );
    let unreviewed = json!({"after": ["/tasks/far.md"]});
    tasks.write("tasks/unreviewed.md", &open("Unreviewed", unreviewed));
    tasks.write(
        "tasks/deferred.md",
        &open("Deferred", json!({"defer": day(30)})),
    );
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(
        (
            shown(&output),
            titles(&output, "due_soon"),
            titles(&output, "stale")
        ),
        (
            titles_of(&["Far"]),
            titles_of(&["Due"]),
            titles_of(&["Unreviewed"])
        )
    );
}

fn titles_of(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn context_narrows_every_section_to_tasks_with_that_tag() {
    let tasks = Tasks::new();
    let milk = reviewed(json!({"contexts": ["errands"], "due": day(3)}));
    tasks.write("tasks/milk.md", &open("Milk", milk));
    let call = reviewed(json!({"contexts": ["phone"], "due": day(3)}));
    tasks.write("tasks/call.md", &open("Call", call));
    tasks.write("tasks/think.md", &open("Think", reviewed(json!({}))));
    let output = tasks.call("available", &["--context", "errands"]).unwrap();
    assert_eq!(
        (shown(&output), titles(&output, "due_soon")),
        (titles_of(&["Milk"]), titles_of(&["Milk"]))
    );
    assert_eq!(
        shown(&tasks.call("available", &[]).unwrap()),
        ["Call", "Milk", "Think"]
    );
}

#[test]
fn a_task_file_that_is_not_a_readable_task_is_counted_as_degraded() {
    let tasks = Tasks::new();
    tasks.write("tasks/good.md", &open("Good", reviewed(json!({}))));
    assert_eq!(tasks.call("available", &[]).unwrap()["degraded"], 0);
    tasks.write(
        "tasks/broken.md",
        "---\n$type: tasks/task/v2\ntitle: [Broken\n---\n",
    );
    tasks.write(
        "tasks/invalid.md",
        &task("Invalid", "waiting", &json!({}), ""),
    );
    tasks.write("tasks/untyped.md", "# A note, not a task\n");
    let dangling = reviewed(json!({"after": ["/tasks/gone.md"]}));
    tasks.write("tasks/dangling.md", &open("Dangling", dangling));
    tasks.write(
        "notes/elsewhere.md",
        &open("Elsewhere", reviewed(json!({}))),
    );
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(
        (&output["degraded"], shown(&output)),
        (&json!(3), titles_of(&["Good"]))
    );
}

#[test]
fn a_done_task_check_degrades_counts_as_degraded_and_its_valid_done_twin_neither_shows_nor_counts()
{
    let tasks = Tasks::new();
    tasks.write("tasks/good.md", &open("Good", reviewed(json!({}))));
    let home = json!({"contexts": ["home"]});
    tasks.write("tasks/finished.md", &task("Finished", "done", &home, ""));
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(
        (&output["degraded"], shown(&output)),
        (&json!(0), titles_of(&["Good"]))
    );
    let bad = json!({"contexts": ["Not A Tag"]});
    tasks.write("tasks/finished.md", &task("Finished", "done", &bad, ""));
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(
        (&output["degraded"], shown(&output)),
        (&json!(1), titles_of(&["Good"]))
    );
}

#[test]
fn a_done_task_the_schema_admits_and_the_model_refuses_counts_as_degraded_and_its_valid_twin_does_not()
 {
    // JSON Schema's date-time admits a leap second, which Python's
    // datetime, and so the plugin, refuses.
    let tasks = Tasks::new();
    tasks.write("tasks/good.md", &open("Good", reviewed(json!({}))));
    let valid = json!({"defer": "2026-06-30T23:59:59Z"});
    tasks.write("tasks/finished.md", &task("Finished", "done", &valid, ""));
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(
        (&output["degraded"], shown(&output)),
        (&json!(0), titles_of(&["Good"]))
    );
    let leap = json!({"defer": "2026-06-30T23:59:60Z"});
    tasks.write("tasks/finished.md", &task("Finished", "done", &leap, ""));
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(
        (&output["degraded"], shown(&output)),
        (&json!(1), titles_of(&["Good"]))
    );
}

#[test]
fn the_remote_chooses_its_covering_repo_with_the_personal_repo_and_the_boards_own_tree() {
    let tasks = estate();
    let output = tasks
        .call(
            "available",
            &["--remote", "git@github.com:example-org/home.git"],
        )
        .unwrap();
    assert_eq!(
        output["repos"],
        json!(["/", "/repos/personal", "/repos/work"])
    );
    assert_eq!(shown(&output), ["Board task", "Home task", "Work task"]);
    let repos: Vec<&Value> = (output["available"]["tasks"].as_array().unwrap().iter())
        .map(|item| &item["repo"])
        .collect();
    assert_eq!(
        repos,
        [
            &json!("/"),
            &json!("/repos/personal"),
            &json!("/repos/work")
        ]
    );
    let output = tasks
        .call("available", &["--remote", "crumbkit/board"])
        .unwrap();
    assert_eq!(output["repos"], json!(["/", "/repos/personal"]));
}

#[test]
fn without_a_remote_every_repo_is_read_and_repo_names_one() {
    let tasks = estate();
    let every = json!(["/", "/repos/other", "/repos/personal", "/repos/work"]);
    assert_eq!(tasks.call("available", &[]).unwrap()["repos"], every);
    let flags = ["--repo", "/repos/work/", "--remote", "acme/x"];
    let output = tasks.call("available", &flags).unwrap();
    assert_eq!(
        (&output["repos"], shown(&output)),
        (&json!(["/repos/work"]), titles_of(&["Work task"]))
    );
    let refused = tasks.refused("available", &["--repo", "/repos/elsewhere"]);
    assert_eq!(refused["code"], "tasks::repo::not_found");
    assert_eq!(refused["fields"]["repos"], every);
}

#[test]
fn a_call_syncs_the_repos_in_scope_and_an_after_in_a_mount_resolves_in_that_mount() {
    let tasks = Tasks::new();
    let personal = json!({"covers": [], "personal": true});
    let clone = tasks.notes_repo("personal", Some(personal), &[], true);
    tasks.write("tasks/first.md", &open("Board first", reviewed(json!({}))));
    let second = open("Second", reviewed(json!({"after": ["/tasks/first.md"]})));
    let mobile = tasks.phone("personal");
    let first = open("First", reviewed(json!({})));
    push(
        &mobile,
        &[("tasks/first.md", &first), ("tasks/second.md", &second)],
    );
    let output = tasks
        .call("available", &["--repo", "/repos/personal"])
        .unwrap();
    assert_eq!(
        (&output["unsynced"], shown(&output)),
        (&json!([]), titles_of(&["First"]))
    );
    let listed = &output["available"]["tasks"][0];
    assert_eq!(
        (&listed["path"], &listed["waiting"]),
        (&json!("/repos/personal/tasks/first.md"), &json!(1))
    );
    assert!(clone.join("tasks/second.md").exists(), "the clone synced");
    push(
        &mobile,
        &[("tasks/first.md", &task("First", "done", &json!({}), ""))],
    );
    let output = tasks
        .call("available", &["--repo", "/repos/personal"])
        .unwrap();
    assert_eq!(
        shown(&output),
        ["Second"],
        "the open /tasks/first.md is the board's"
    );
}

#[test]
fn a_pin_that_cannot_move_lists_its_repo_as_unsynced_with_the_code_and_the_call_still_answers() {
    use std::os::unix::fs::PermissionsExt;
    let tasks = estate();
    let more = open("More", reviewed(json!({})));
    push(&tasks.phone("work"), &[("tasks/more.md", &more)]);
    let repos = tasks.board.workdir().join("repos");
    let permit = |mode| std::fs::set_permissions(&repos, std::fs::Permissions::from_mode(mode));
    permit(0o555).unwrap();
    let output = tasks.call("available", &["--repo", "/repos/work"]);
    permit(0o755).unwrap();
    let output = output.unwrap();
    let unsynced = output["unsynced"].as_array().unwrap();
    assert_eq!(unsynced.len(), 1, "{output}");
    assert_eq!(unsynced[0]["repo"], "/repos/work");
    assert!(
        unsynced[0]["code"].as_str().unwrap().starts_with("board::"),
        "{output}"
    );
    assert_eq!(shown(&output), ["Work task"], "read at the pin");
    let output = tasks.call("available", &["--repo", "/repos/work"]).unwrap();
    assert_eq!(
        (&output["unsynced"], shown(&output)),
        (&json!([]), titles_of(&["More", "Work task"]))
    );
}

/// Python left a sync past its deadline running; Board stops git at the
/// deadline, so the clone is left as it was and its lock released.
#[test]
fn a_sync_past_its_deadline_is_stopped_and_its_repo_read_at_its_pin() {
    let tasks = estate();
    let clone = tasks.clones.borrow()[1].1.workdir().to_path_buf();
    git(&clone, &["config", "core.sshCommand", "sleep 6 #"]);
    let unreachable = "ssh://git@example.invalid/work.git";
    git(&clone, &["remote", "set-url", "origin", unreachable]);
    let output = tasks.call("available", &["--repo", "/repos/work"]).unwrap();
    let timed_out = json!([{"repo": "/repos/work", "code": "tasks::repo::sync_timeout"}]);
    assert_eq!(output["unsynced"], timed_out);
    assert_eq!(shown(&output), ["Work task"]);
    let remote = tasks.beside.path().join("work.git");
    git(
        &clone,
        &["remote", "set-url", "origin", remote.to_str().unwrap()],
    );
    assert!(
        !clone.join(".git/board.lock").exists(),
        "the stopped sync released Board's lock"
    );
    let output = tasks.call("available", &["--repo", "/repos/work"]).unwrap();
    assert_eq!(output["unsynced"], json!([]));
}

#[test]
fn a_repo_that_cannot_sync_is_listed_with_its_code_and_read_as_its_clone_holds_it() {
    let tasks = estate();
    let remote = tasks.beside.path().join("work.git");
    let away = tasks.beside.path().join("away.git");
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture takes the remote away, as a network outage would"
    )]
    std::fs::rename(&remote, &away).unwrap();
    let output = tasks
        .call("available", &["--remote", "example-org/home"])
        .unwrap();
    let failed = json!([{"repo": "/repos/work", "code": "board::sync::failed"}]);
    assert_eq!(output["unsynced"], failed);
    assert_eq!(shown(&output), ["Board task", "Home task", "Work task"]);
}

#[test]
fn available_answers_the_same_from_a_cold_cache() {
    let tasks = estate();
    let warm = tasks.call("available", &[]).unwrap();
    let cache = tasks.cache.path().join("board");
    assert!(cache.exists(), "the index is cached");
    #[expect(clippy::disallowed_methods, reason = "the test empties the cache")]
    std::fs::remove_dir_all(&cache).unwrap();
    assert_eq!(tasks.call("available", &[]).unwrap(), warm);
}

#[test]
fn a_board_with_no_task_answers_with_empty_sections() {
    let tasks = Tasks::new();
    let output = tasks.call("available", &[]).unwrap();
    let totals: Vec<&Value> = ["available", "due_soon", "stale", "held"]
        .iter()
        .map(|name| &output[name]["total"])
        .collect();
    assert_eq!(totals, [&json!(0); 4]);
    tasks.write("tasks/first.md", &open("First", reviewed(json!({}))));
    assert_eq!(shown(&tasks.call("available", &[]).unwrap()), ["First"]);
}

#[test]
fn a_mount_whose_clone_has_not_moved_keeps_its_pin_unwritten() {
    let tasks = estate();
    let pointer = tasks.board.workdir().join("repos/work.json");
    let modified = || std::fs::metadata(&pointer).unwrap().modified().unwrap();
    let written = modified();
    let output = tasks.call("available", &["--repo", "/repos/work"]).unwrap();
    assert_eq!(shown(&output), ["Work task"]);
    assert_eq!(modified(), written);
    let clone = tasks.clones.borrow()[1].1.workdir().to_path_buf();
    write_file(
        &clone.join("tasks/more.md"),
        &open("More", reviewed(json!({}))),
    );
    git(&clone, &["add", "."]);
    git(&clone, &["commit", "-qm", "more"]);
    let output = tasks.call("available", &["--repo", "/repos/work"]).unwrap();
    assert_eq!(shown(&output), ["More", "Work task"]);
    assert_ne!(modified(), written);
}

#[test]
fn inside_a_git_repository_without_origin_only_the_boards_tree_and_the_personal_repo_are_read_and_why()
 {
    let tasks = estate();
    let checkout = tasks.checkout();
    let output = tasks.call_with("available", &[], 0, &checkout, "").unwrap();
    assert_eq!(output["repos"], json!(["/", "/repos/personal"]));
    let why = output["origin_unread"].as_str().unwrap();
    let resolved = checkout.canonicalize().unwrap();
    assert!(
        why.starts_with(&format!("git gave no origin for {}: ", resolved.display()))
            || why.starts_with(&format!("git gave no origin for {}: ", checkout.display())),
        "{why}"
    );
    let origin = "git@github.com:example-org/home.git";
    git(&checkout, &["remote", "add", "origin", origin]);
    let output = tasks.call_with("available", &[], 0, &checkout, "").unwrap();
    assert_eq!(
        (&output["repos"], &output["origin_unread"]),
        (
            &json!(["/", "/repos/personal", "/repos/work"]),
            &Value::Null
        )
    );
    let output = tasks.call("available", &[]).unwrap();
    let every = json!(["/", "/repos/other", "/repos/personal", "/repos/work"]);
    assert_eq!(
        (&output["repos"], &output["origin_unread"]),
        (&every, &Value::Null)
    );
}

#[test]
fn an_after_naming_a_task_in_another_repo_is_listed_and_never_blocks_and_one_in_its_own_repo_does()
{
    let tasks = estate();
    let after = json!({"after": ["/repos/work/tasks/work.md", "/tasks/board.md"]});
    tasks.write("tasks/crossing.md", &open("Crossing", after));
    let output = tasks.call("available", &[]).unwrap();
    let crossing = json!([{"path": "/tasks/crossing.md", "after": "/repos/work/tasks/work.md"}]);
    assert_eq!(output["cross_repo"], crossing);
    let stale = output["stale"]["tasks"].as_array().unwrap();
    let listed = stale
        .iter()
        .find(|item| item["title"] == "Crossing")
        .unwrap();
    assert_eq!(listed["blocked_by"], json!(["/tasks/board.md"]));
    tasks.write(
        "tasks/board.md",
        &task("Board task", "done", &json!({}), ""),
    );
    let output = tasks.call("available", &[]).unwrap();
    assert!(shown(&output).contains(&"Crossing".to_owned()));
    assert_eq!(output["cross_repo"], crossing);
}

#[test]
fn a_task_deferred_to_today_waits_before_the_configured_day_start_and_not_without_one() {
    let tasks = Tasks::new();
    let (offset, today) = one_in_the_morning();
    tasks.write(
        "tasks/today.md",
        &open("Today", reviewed(json!({"defer": today}))),
    );
    tasks.day_start(Some("07:00"));
    let outside = tasks.outside.path();
    let output = tasks
        .call_with("available", &[], offset, outside, "")
        .unwrap();
    assert_eq!(shown(&output), Vec::<String>::new());
    tasks.day_start(None);
    let output = tasks
        .call_with("available", &[], offset, outside, "")
        .unwrap();
    assert_eq!(shown(&output), ["Today"]);
}

#[test]
fn owner_keeps_the_tasks_it_or_nobody_holds_and_lists_its_unavailable_ones_in_held() {
    let tasks = Tasks::new();
    tasks.write(
        "tasks/mine.md",
        &open("Mine", reviewed(json!({"owner": {"person": "lead-1"}}))),
    );
    tasks.write("tasks/free.md", &open("Free", reviewed(json!({}))));
    tasks.write(
        "tasks/theirs.md",
        &open("Theirs", reviewed(json!({"owner": {"person": "worker-2"}}))),
    );
    let parked = reviewed(json!({"owner": {"person": "lead-1"}, "defer": day(30)}));
    tasks.write("tasks/parked.md", &open("Parked", parked));
    let output = tasks.call("available", &["--person", "lead-1"]).unwrap();
    assert_eq!(
        (
            shown(&output),
            titles(&output, "held"),
            &output["held"]["total"]
        ),
        (
            titles_of(&["Free", "Mine"]),
            titles_of(&["Parked"]),
            &json!(1)
        )
    );
    let owners: Vec<&Value> = (output["available"]["tasks"].as_array().unwrap().iter())
        .map(|item| &item["owner"])
        .collect();
    assert_eq!(owners, [&Value::Null, &json!({"person": "lead-1"})]);
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(
        (shown(&output), titles(&output, "held")),
        (titles_of(&["Free", "Mine", "Theirs"]), Vec::new())
    );
}

/// What blocks each listed task, by its path, from `available --repo
/// <repo>`. A task without `reviewed` is listed whether or not it waits.
fn blocking(tasks: &Tasks, repo: &str) -> BTreeMap<String, Value> {
    let output = tasks.call("available", &["--repo", repo]).unwrap();
    (["available", "stale"].iter())
        .flat_map(|section| output[section]["tasks"].as_array().unwrap().clone())
        .map(|item| {
            (
                item["path"].as_str().unwrap().to_owned(),
                item["blocked_by"].clone(),
            )
        })
        .collect()
}

#[test]
fn a_task_is_blocked_while_a_task_in_its_after_is_not_done() {
    let tasks = Tasks::new();
    tasks.write("tasks/open.md", &open("Open", json!({})));
    tasks.write(
        "tasks/finished.md",
        &task("Finished", "done", &json!({}), ""),
    );
    tasks.write(
        "tasks/abandoned.md",
        &task("Abandoned", "dropped", &json!({}), ""),
    );
    let on_open = json!({"after": ["/tasks/open.md", "/tasks/finished.md", "/tasks/open.md"]});
    tasks.write("tasks/on-open.md", &open("On open", on_open));
    let on = |target: &str| json!({"after": [target]});
    tasks.write(
        "tasks/on-finished.md",
        &open("On finished", on("/tasks/finished.md")),
    );
    tasks.write(
        "tasks/on-dropped.md",
        &open("On dropped", on("/tasks/abandoned.md")),
    );
    tasks.write(
        "tasks/on-nothing.md",
        &open("On nothing", on("/tasks/missing.md")),
    );
    let expected: BTreeMap<String, Value> = [
        ("/tasks/on-dropped.md", json!(["/tasks/abandoned.md"])),
        ("/tasks/on-finished.md", json!([])),
        ("/tasks/on-nothing.md", json!(["/tasks/missing.md"])),
        ("/tasks/on-open.md", json!(["/tasks/open.md"])),
        ("/tasks/open.md", json!([])),
    ]
    .into_iter()
    .map(|(path, blocked)| (path.to_owned(), blocked))
    .collect();
    assert_eq!(blocking(&tasks, "/"), expected);
    tasks.write("tasks/open.md", &task("Open", "done", &json!({}), ""));
    assert_eq!(blocking(&tasks, "/")["/tasks/on-open.md"], json!([]));
}

#[test]
fn a_done_record_of_another_type_still_blocks_and_a_done_task_does_not() {
    let tasks = Tasks::new();
    tasks.write(
        ".board/types/x/note/v1.schema.json",
        r#"{"type": "object"}"#,
    );
    tasks.write(
        "tasks/note.md",
        "---\n$type: x/note/v1\nstatus: \"done\"\n---\n",
    );
    tasks.write(
        "tasks/finished.md",
        &task("Finished", "done", &json!({}), ""),
    );
    let after = json!({"after": ["/tasks/note.md", "/tasks/finished.md"]});
    tasks.write("tasks/after.md", &open("After", after));
    assert_eq!(
        blocking(&tasks, "/")["/tasks/after.md"],
        json!(["/tasks/note.md"])
    );
}

#[test]
fn a_done_task_check_refuses_still_blocks_and_one_degraded_only_by_the_tree_does_not() {
    let tasks = Tasks::new();
    let hourly = json!({"repeat": "hourly"});
    tasks.write("tasks/finished.md", &task("Finished", "done", &hourly, ""));
    tasks.write(
        "tasks/after.md",
        &open("After", json!({"after": ["/tasks/finished.md"]})),
    );
    let refs = || {
        let arguments = ["--json", "refs", "/tasks/after.md", "--field", "/status"];
        let report = json(&tasks.run(&arguments, 0, None, ""));
        report["data"][0]["from"][0]["target_code"].clone()
    };
    assert_eq!(refs(), "board::record::invalid");
    assert_eq!(
        blocking(&tasks, "/")["/tasks/after.md"],
        json!(["/tasks/finished.md"])
    );
    let dangling = json!({"after": ["/tasks/nowhere.md"]});
    tasks.write(
        "tasks/finished.md",
        &task("Finished", "done", &dangling, ""),
    );
    assert_eq!(refs(), "board::ref::dangling");
    assert_eq!(blocking(&tasks, "/")["/tasks/after.md"], json!([]));
}

#[test]
fn a_reference_through_a_link_reaches_the_task_it_names() {
    let tasks = Tasks::new();
    tasks.write(
        "tasks/finished.md",
        &task("Finished", "done", &json!({}), ""),
    );
    tasks.write(
        "alias.json",
        r#"{"$type": "board/mount/v1", "path": "/tasks/finished.md"}"#,
    );
    tasks.write(
        "tasks/after.md",
        &open("After", json!({"after": ["/alias"]})),
    );
    let expected = BTreeMap::from([("/tasks/after.md".to_owned(), json!([]))]);
    assert_eq!(blocking(&tasks, "/"), expected);
    tasks.write("tasks/finished.md", &open("Finished", json!({})));
    assert_eq!(
        blocking(&tasks, "/")["/tasks/after.md"],
        json!(["/tasks/finished.md"])
    );
}

#[test]
fn a_commit_mounted_twice_is_one_repo_whose_done_task_unblocks() {
    let tasks = Tasks::new();
    let finished = task("Finished", "done", &json!({}), "");
    let pending = open("Pending", json!({}));
    let after = open(
        "After",
        json!({"after": ["/tasks/finished.md", "/tasks/pending.md"]}),
    );
    let files = [
        ("finished", finished.as_str()),
        ("pending", &pending),
        ("after", &after),
    ];
    let clone = tasks.notes_repo("a", Some(json!({"covers": []})), &files, true);
    tasks.mount("z", &clone);
    let blocked = blocking(&tasks, "/repos/a");
    assert_eq!(
        blocked["/repos/a/tasks/after.md"],
        json!(["/repos/a/tasks/pending.md"])
    );
    let refused = tasks.refused("available", &["--repo", "/repos/z"]);
    assert_eq!(
        (&refused["code"], &refused["fields"]["repos"]),
        (&json!("tasks::repo::not_found"), &json!(["/", "/repos/a"]))
    );
}
