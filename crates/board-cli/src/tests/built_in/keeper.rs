//! The keeper plugin through the binary, with changes and tasks enabled
//! beside it. These tests answer the questions that the plugin's design
//! rests on: it owns its namespace, it reaches other plugins' capabilities
//! through the host, and it writes a file.

use super::*;

/// A board that enables keeper and the plugins it reads.
fn board() -> Fixture {
    Fixture::enabling(&["keeper", "changes", "tasks", "plans"])
}

/// The UTC day `days` before today, as YYYY-MM-DD.
fn days_ago(days: i64) -> String {
    let today = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();
    today
        .checked_sub(jiff::Span::new().days(days))
        .unwrap()
        .to_string()
}

/// A `changes/change/v2` record with no mount, touched `touched` days ago.
fn change(title: &str, phase: &str, touched: i64) -> String {
    json!({
        "$type": "changes/change/v2", "title": title, "phase": phase,
        "touched": days_ago(touched),
    })
    .to_string()
}

/// An open task that was last looked at `reviewed` days ago.
fn task(title: &str, reviewed: Option<i64>) -> String {
    let day = reviewed.map_or(String::new(), |days| {
        format!("reviewed: {}\n", days_ago(days))
    });
    format!("---\n$type: tasks/task/v2\ntitle: {title}\nstatus: open\n{day}---\n")
}

/// A `plans/question/v1` record, open.
fn open_question(text: &str) -> String {
    json!({
        "$type": "plans/question/v1", "question": text, "by": {"session": "local_0a1b"},
        "asked": "2026-10-01T09:00:00Z", "status": "open",
    })
    .to_string()
}

/// A `plans/answer/v1` record that answers `question`.
fn answer(question: &str) -> String {
    json!({
        "$type": "plans/answer/v1", "question": question, "words": "Do it.",
        "answered": "2026-10-02T09:00:00Z", "by": {"session": "local_0a1b"},
    })
    .to_string()
}

#[test]
fn a_plugin_may_own_the_namespace_keeper() {
    let board = board();
    let listed = board.json(&["plugin", "list"]);
    let names: Vec<&str> = (listed["data"].as_array().unwrap().iter())
        .filter_map(|plugin| plugin["name"].as_str())
        .collect();
    assert!(names.contains(&"keeper"), "{listed}");
}

#[test]
fn gather_reaches_changes_open_and_tasks_tree_through_the_host() {
    let board = board();
    board.write("changes/a.json", &change("Add a timeout", "build", 0));
    board.write("tasks/one.md", &task("One", Some(0)));
    let output = board.call("keeper.gather", &[]);
    assert_eq!(output["changes"][0]["path"], "/changes/a", "{output}");
    assert_eq!(
        output["tasks"]["open"][0]["path"], "/tasks/one.md",
        "{output}"
    );
}

#[test]
fn gather_lists_each_stale_work_item_and_leaves_out_its_fresh_twin() {
    let board = board();
    board
        .write("changes/old.json", &change("Old build", "build", 4))
        .write("changes/new.json", &change("New build", "build", 3))
        .write("changes/rev.json", &change("Old review", "review", 40))
        .write("tasks/old.md", &task("Old", Some(15)))
        .write("tasks/new.md", &task("New", Some(14)))
        .write("tasks/never.md", &task("Never", None));
    let stale = board.call("keeper.gather", &[])["stale"].clone();
    let paths = |list: &str| -> Vec<String> {
        (stale[list].as_array().unwrap().iter())
            .map(|item| item["path"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(paths("changes"), ["/changes/old"], "{stale}");
    assert_eq!(
        paths("tasks"),
        ["/tasks/never.md", "/tasks/old.md"],
        "{stale}"
    );
    assert_eq!(paths("prototypes"), Vec::<String>::new());
}

#[test]
fn gather_counts_a_question_answered_under_a_mounted_boards_shorter_path_as_answered() {
    let board = board();
    board
        .write(
            "projects/p/questions/here.json",
            &open_question("Local open?"),
        )
        .write(
            "projects/p/questions/done.json",
            &open_question("Local done?"),
        )
        .write(
            "projects/p/answers/done.json",
            &answer("/projects/p/questions/done"),
        );
    let (library, commit) = repository(
        &["plans"],
        &[
            (
                "projects/q/questions/open.json",
                open_question("Mounted open?"),
            ),
            (
                "projects/q/questions/done.json",
                open_question("Mounted done?"),
            ),
            (
                "projects/q/answers/done.json",
                answer("/projects/q/questions/done"),
            ),
        ],
    );
    mount(&board, "repos/team.json", &library, commit);
    let questions = board.call("keeper.gather", &[])["questions"].clone();
    let mut waiting: Vec<&str> = (questions["waiting"].as_array().unwrap().iter())
        .map(|question| question["path"].as_str().unwrap())
        .collect();
    waiting.sort_unstable();
    assert_eq!(
        waiting,
        [
            "/projects/p/questions/here",
            "/repos/team/projects/q/questions/open"
        ],
        "{questions}"
    );
}

#[test]
fn render_writes_a_file_at_a_path_in_the_board() {
    let board = board();
    board.call("keeper.render", &[]);
    assert_eq!(
        board.read("projects/keeper/work-view/index.html"),
        "<!doctype html>\n"
    );
}

/// A map of one realm, one project and one finding, with `fields` replacing
/// its own.
fn map(fields: &Value) -> String {
    record(
        json!({
            "$type": "keeper/map/v1",
            "date": "2026-02-03",
            "source": "/curation.md",
            "realms": [{"id": "alpha", "title": "Alpha"}],
            "projects": [{
                "id": "speed", "realm": "alpha", "title": "Speed", "state": "active",
                "task": null, "changes": ["/changes/a"],
            }],
            "findings": [{
                "kind": "stale", "paths": ["/changes/a"], "evidence": "No commit.",
                "fix": "changes.pause", "id": "stale-0a1b2c3d",
            }],
        }),
        fields,
    )
}

#[test]
fn a_map_is_admitted_by_board_and_one_with_a_bad_finding_id_is_refused_with_its_code() {
    let board = board();
    board.write("curation.md", "# Pass\n");
    board.write("projects/keeper/map/ok.json", &map(&json!({})));
    let admitted = board.json(&["check", "--healthy", "--all"]);
    assert!(
        !admitted["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["logical"] == "/projects/keeper/map/ok" && !entry["code"].is_null()),
        "{admitted}"
    );
    let bad = map(&json!({"findings": [{
        "kind": "stale", "paths": [], "evidence": "x", "fix": "y", "id": "stale-xyz",
    }]}));
    let put = board.run_with(&["put", "/projects/keeper/map/bad"], &bad);
    assert_ne!(put.code, 0);
    assert!(
        text(&put.stderr).contains("board::record::invalid"),
        "{}",
        text(&put.stderr)
    );
    let good = board.run_with(&["put", "/projects/keeper/map/good"], &map(&json!({})));
    assert_eq!(good.code, 0, "{}", text(&good.stderr));
}

#[test]
fn gather_refuses_when_a_plugin_it_reads_is_not_enabled_and_names_the_capability() {
    let board = Fixture::enabling(&["keeper", "changes", "plans"]);
    let refused = board.refusal("keeper.gather", &[]);
    assert_eq!(
        refused["code"], "keeper::gather::source_failed",
        "{refused}"
    );
    assert_eq!(refused["fields"]["capability"], "tasks.tree", "{refused}");
    let enabled = Fixture::enabling(&["keeper", "changes", "plans", "tasks"]);
    enabled.call("keeper.gather", &[]);
}
