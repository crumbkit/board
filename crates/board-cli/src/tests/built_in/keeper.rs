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

/// The input of `keeper.map` for the open changes `a` and `b`, with `fields`
/// replacing its own.
fn map_input(fields: &Value) -> String {
    record(
        json!({
            "source": "/curation.md",
            "realms": [{"id": "alpha", "title": "Alpha"}, {"id": "beta", "title": "Beta"}],
            "projects": [
                {"id": "speed", "realm": "alpha", "title": "Speed", "state": "active",
                 "task": null, "changes": ["/changes/a", "/changes/b"]},
                {"id": "docs", "realm": "beta", "title": "Docs", "state": "queued",
                 "task": null, "changes": []},
            ],
            "findings": [{"kind": "stale", "paths": ["/changes/a"], "evidence": "No commit.",
                          "fix": "changes.pause"}],
        }),
        fields,
    )
}

/// A board with changes `a` and `b` and a curation record.
fn mapped() -> Fixture {
    let board = board();
    board
        .write("curation.md", "# Pass\n")
        .write("changes/a.json", &change("Add a timeout", "build", 0))
        .write("changes/b.json", &change("Cache results", "build", 0));
    board
}

/// The record at the logical `path`, or none.
fn stored(board: &Fixture, path: &str) -> Option<Value> {
    let file = board
        .board
        .workdir()
        .join(format!("{}.json", path.trim_start_matches('/')));
    Some(serde_json::from_str(&std::fs::read_to_string(file).ok()?).unwrap())
}

fn run_map(board: &Fixture, input: &str) -> Output {
    board.run_with(&["--json", "call", "keeper.map"], input)
}

#[test]
fn map_writes_a_timestamped_record_and_latest_in_one_commit_with_computed_fields() {
    let board = mapped();
    board.write(
        "projects/p/questions/q.json",
        &open_question("Should cache wait?"),
    );
    let input = map_input(&json!({"question_rules": {
        "mount_realms": {}, "localRealm": {}, "localDefault": "beta",
        "tasks": [], "names": {"p": "speed"},
    }}));
    let output = run_map(&board, &input);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let said = json(&output)["data"]["output"].clone();
    let path = said["path"].as_str().unwrap();
    assert!(path.starts_with("/projects/keeper/map/2"), "{said}");
    assert_eq!(said["latest"], "/projects/keeper/map/latest");
    let held = stored(&board, path).expect("the timestamped record is written");
    let latest = stored(&board, "/projects/keeper/map/latest").expect("latest is written");
    assert_eq!(held, latest);
    assert_eq!(held["$type"], "keeper/map/v1");
    assert_eq!(held["date"], days_ago(0));
    let id = held["findings"][0]["id"].as_str().unwrap();
    assert!(id.starts_with("stale-") && id.len() == 14, "{id}");
    assert_eq!(held["questions_waiting"], json!({"alpha": 1, "beta": 0}));
    assert!(held.get("dedupe").is_none(), "{held}");
    let checked = board.checked("/projects/keeper/map/");
    assert!(checked.values().all(Option::is_none), "{checked:?}");
}

#[test]
fn a_second_map_replaces_latest_and_leaves_the_first_where_it_was() {
    let board = mapped();
    let first = run_map(&board, &map_input(&json!({})));
    assert_eq!(first.code, 0, "{}", text(&first.stderr));
    // The record's path has the second in it.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = run_map(
        &board,
        &map_input(&json!({"projects": [
            {"id": "speed", "realm": "alpha", "title": "Speed 2", "state": "active",
             "task": null, "changes": ["/changes/a", "/changes/b"]},
        ]})),
    );
    assert_eq!(second.code, 0, "{}", text(&second.stderr));
    let (first, second) = (
        json(&first)["data"]["output"].clone(),
        json(&second)["data"]["output"].clone(),
    );
    assert_ne!(first["path"], second["path"]);
    let title = |path: &str| stored(&board, path).unwrap()["projects"][0]["title"].clone();
    assert_eq!(title(first["path"].as_str().unwrap()), "Speed");
    assert_eq!(title(second["path"].as_str().unwrap()), "Speed 2");
    assert_eq!(title("/projects/keeper/map/latest"), "Speed 2");
}

#[test]
fn map_refuses_an_unplaced_open_change_and_writes_nothing() {
    let board = mapped();
    let input = map_input(&json!({"projects": [
        {"id": "speed", "realm": "alpha", "title": "Speed", "state": "active",
         "task": null, "changes": ["/changes/a"]},
    ]}));
    let output = run_map(&board, &input);
    assert_ne!(output.code, 0);
    let failed = json(&output);
    assert_eq!(failed["code"], "keeper::map::unplaced", "{failed}");
    assert!(stored(&board, "/projects/keeper/map/latest").is_none());
    let placed = run_map(&board, &map_input(&json!({})));
    assert_eq!(placed.code, 0, "{}", text(&placed.stderr));
}

#[test]
fn render_refuses_without_a_map_and_with_one_that_does_not_read_and_writes_a_page_with_one() {
    let board = mapped();
    let none = board.refusal("keeper.render", &[]);
    assert_eq!(none["code"], "keeper::render::no_map", "{none}");
    board.write("projects/keeper/map/latest.json", "{\"date\": 3}");
    let bad = board.refusal("keeper.render", &[]);
    assert_eq!(bad["code"], "keeper::render::bad_map", "{bad}");
    board.write("projects/keeper/map/latest.json", &map(&json!({})));
    let said = board.call("keeper.render", &[]);
    assert_eq!(said["path"], "/projects/keeper/work-view/index.html");
    assert_eq!(
        (said["open"].clone(), said["missing"].clone()),
        (json!(2), json!(1)),
        "{said}"
    );
    let page = board.read("projects/keeper/work-view/index.html");
    for id in ["data", "systems", "questions"] {
        assert!(page.contains(&format!("id=\"{id}\"")), "{page}");
    }
    assert!(page.contains("Add a timeout"));
    let elsewhere = board.call("keeper.render", &["--path", "/projects/keeper/other.html"]);
    assert_eq!(elsewhere["path"], "/projects/keeper/other.html");
    assert_eq!(board.read("projects/keeper/other.html"), page);
}

/// A map whose one system reads the Cargo workspace mounted at `/repos/lib`.
fn surveyed_map() -> String {
    map(&json!({"systems": [{
        "id": "lib", "realm": "alpha", "title": "Lib", "language": "Rust",
        "crates_from": "/repos/lib",
    }]}))
}

#[test]
fn render_reads_the_crates_of_a_mounted_cargo_workspace_and_says_so_when_there_are_none() {
    let board = mapped();
    board.write("projects/keeper/map/latest.json", &surveyed_map());
    let (library, commit) = repository(
        &[],
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/*\"]\n".to_owned(),
            ),
            (
                "crates/one/Cargo.toml",
                "[package]\nname = \"lib-one\"\n[dependencies]\nlib-two = \"*\"\n".to_owned(),
            ),
            ("crates/one/src/lib.rs", "a\nb\n".to_owned()),
            ("crates/one/src/run/mod.rs", "c\n".to_owned()),
            (
                "crates/two/Cargo.toml",
                "[package]\nname = \"lib-two\"\n".to_owned(),
            ),
            ("crates/two/src/lib.rs", "d\n".to_owned()),
        ],
    );
    mount(&board, "repos/lib.json", &library, commit);
    let said = board.call("keeper.render", &[]);
    assert_eq!(
        said["surveys"],
        json!([{"system": "lib", "crates": 2}]),
        "{said}"
    );
    let page = board.read("projects/keeper/work-view/index.html");
    let data = page
        .split("id=\"data\">")
        .nth(1)
        .and_then(|rest| rest.split("</script>").next())
        .unwrap();
    let data: Value = serde_json::from_str(data).unwrap();
    let crates = &data["workspaces"][0]["crates"];
    assert_eq!(crates[0]["id"], "lib-one", "{data}");
    assert_eq!(crates[0]["deps"], json!(["lib-two"]));
    assert_eq!(crates[0]["loc"], 3);
    assert_eq!(
        crates[0]["modules"],
        json!([{"id": "lib", "loc": 2}, {"id": "run", "loc": 1}])
    );
    let (plain, commit) = repository(&[], &[("README.md", "# Plain\n".to_owned())]);
    let other = mapped();
    other.write("projects/keeper/map/latest.json", &surveyed_map());
    mount(&other, "repos/lib.json", &plain, commit);
    let said = other.call("keeper.render", &[]);
    assert_eq!(said["surveys"][0]["crates"], 0, "{said}");
    assert!(
        said["surveys"][0]["problem"]
            .as_str()
            .unwrap()
            .contains("no Cargo.toml"),
        "{said}"
    );
}
