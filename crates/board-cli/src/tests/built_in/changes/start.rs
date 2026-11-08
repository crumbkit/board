//! `test_start.py`: `changes.start`, which writes a new change, its plan,
//! its route and the repository's brief.

use super::*;

const CONTRIBUTING: &str = "Run the checks.\n\nThe check suite is `/suites/checks`.\n";

/// A `runs/suite/v1` record of a suite of `x`.
fn suite() -> String {
    typed(
        "runs/suite/v1",
        &json!({"subjects": ["x"], "method": "Run it."}),
    )
}

#[test]
fn start_creates_the_change_in_plan_of_the_development_flow_with_a_plan_whose_sections_are_left_to_fill_in()
 {
    let board = Changes::new();
    let mount = board.mount_x(&[("README.md", "x")]);
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "Let a suite declare a timeout!"}),
    );
    let at = "/changes/let-a-suite-declare-a-timeout";
    assert_eq!(output["path"], at);
    let version = version_of(&board, at);
    let text = output["plan"].as_str().unwrap();
    let record = text.strip_suffix(".md").unwrap();
    assert!(record.starts_with(&format!("{at}/plans/")), "{output}");
    assert_eq!(
        board.read("changes/let-a-suite-declare-a-timeout.json"),
        json!({
            "$type": "changes/change/v2",
            "title": "Let a suite declare a timeout!",
            "mount": "/repos/x",
            "flow": version,
            "phase": "plan",
            "surfaced": [],
            "plan": record,
            "route": "unowned",
            "task": TASK,
            "touched": today(),
        })
    );
    assert_eq!(
        output["flow"],
        json!({"name": "development", "phases": ["plan", "build", "review", "land"]})
    );
    let stored = board.read(&format!("{}.json", version.trim_start_matches('/')));
    assert_eq!(
        (&stored["$type"], &stored["name"]),
        (&json!("changes/flow/v1"), &json!("development"))
    );
    assert!(!board.exists("changes/let-a-suite-declare-a-timeout/flow.json"));
    assert_eq!(
        board.read(&format!("{}.json", record.trim_start_matches('/'))),
        json!({"$type": "changes/plan/v1", "text": text, "uses": []})
    );
    assert_eq!(
        board.text(text.trim_start_matches('/')),
        "# Let a suite declare a timeout!\n\n## Problem\n\n## Outcome\n\n## Out of scope\n\n## UX\n\n## Data\n"
    );
}

#[test]
fn the_route_is_owner_when_the_handle_owns_every_owned_pattern() {
    let board = Changes::new();
    let mount = board.mount_x(&[(
        ".github/CODEOWNERS",
        "# owners\n* @luke\ndocs/ @Luke  # docs too\n",
    )]);
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T", "handle": "@luke"}),
    );
    assert_eq!(
        (&output["route"], &output["ask_first"]),
        (&json!("owner"), &json!([]))
    );
    assert_eq!(output["codeowners"], "/repos/x/.github/CODEOWNERS");
    assert_eq!(
        output["owners"],
        json!([
            {"pattern": "*", "owners": ["@luke"]},
            {"pattern": "docs/", "owners": ["@Luke"]},
        ])
    );
}

#[test]
fn the_route_is_ask_first_and_lists_the_patterns_someone_else_owns() {
    let board = Changes::new();
    let mount = board.mount_x(&[("CODEOWNERS", "* @luke\n/auth/ @sam @kim\n")]);
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T", "handle": "@luke"}),
    );
    assert_eq!(
        (&output["route"], &output["ask_first"]),
        (&json!("ask-first"), &json!(["/auth/"]))
    );
    assert_eq!(board.read("changes/t.json")["route"], "ask-first");
}

#[test]
fn without_a_handle_every_owned_pattern_is_ask_first() {
    let board = Changes::new();
    let mount = board.mount_x(&[("CODEOWNERS", "* @luke\n")]);
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T"}),
    );
    assert_eq!(
        (&output["route"], &output["ask_first"]),
        (&json!("ask-first"), &json!(["*"]))
    );
}

#[test]
fn the_route_is_unowned_when_the_repository_names_no_owners() {
    let board = Changes::new();
    let mount = board.mount_x(&[("CODEOWNERS", "# nobody yet\n")]);
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T", "handle": "@luke"}),
    );
    assert_eq!(
        (&output["route"], &output["owners"], &output["codeowners"]),
        (&json!("unowned"), &json!([]), &json!("/repos/x/CODEOWNERS"))
    );
}

#[test]
fn the_brief_lists_the_contributing_docs_and_the_check_suite_they_name() {
    let board = Changes::new();
    let mount = board.mount_x(&[
        ("CONTRIBUTING.md", CONTRIBUTING),
        ("AGENTS.md", "Read `/nowhere`.\n"),
    ]);
    board.write("suites/checks.json", &suite());
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T"}),
    );
    assert_eq!(
        output["docs"],
        json!(["/repos/x/CONTRIBUTING.md", "/repos/x/AGENTS.md"])
    );
    assert_eq!(output["suite"], "/suites/checks");
}

#[test]
fn a_contributing_doc_in_the_github_directory_is_found() {
    let board = Changes::new();
    let mount = board.mount_x(&[(".github/CONTRIBUTING.md", CONTRIBUTING)]);
    board.write("suites/checks.json", &suite());
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T"}),
    );
    assert_eq!(
        (&output["docs"], &output["suite"]),
        (
            &json!(["/repos/x/.github/CONTRIBUTING.md"]),
            &json!("/suites/checks")
        )
    );
}

#[test]
fn a_suite_in_the_repositorys_own_board_is_found_before_one_in_this_board() {
    let board = Changes::new();
    let suite = suite();
    let mount = board.mount_x(&[
        ("CONTRIBUTING.md", CONTRIBUTING),
        ("suites/checks.json", &suite),
        (".board/types/runs/suite/v1.schema.json", "{}"),
    ]);
    board.write("suites/checks.json", &suite);
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T"}),
    );
    assert_eq!(output["suite"], "/repos/x/suites/checks");
}

#[test]
fn a_named_path_that_holds_no_suite_is_not_the_check_suite() {
    let board = Changes::new();
    let mount = board.mount_x(&[("CONTRIBUTING.md", CONTRIBUTING)]);
    board.write(
        "suites/checks.json",
        &typed("plans/decision/v1", &json!({"refs": []})),
    );
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T"}),
    );
    assert_eq!(output["suite"], Value::Null);
}

#[test]
fn start_refuses_a_name_a_change_already_has_and_admits_another() {
    let board = Changes::new();
    let mount = board.mount_x(&[("README.md", "x")]);
    board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T"}),
    );
    let refused = board.refusal(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T"}),
    );
    assert_eq!(refused["code"], "changes::change::exists");
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T", "name": "t-2"}),
    );
    assert_eq!(output["path"], "/changes/t-2");
}

#[test]
fn start_refuses_a_repository_path_that_holds_nothing_and_admits_a_mounted_one() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x")]);
    let refused = board.refusal(
        "start",
        &json!({"mount": "/repos/y", "task": TASK, "title": "T"}),
    );
    assert_eq!(refused["code"], "changes::mount::not_found");
    let output = board.call(
        "start",
        &json!({"mount": "/repos/x", "task": TASK, "title": "T"}),
    );
    assert_eq!(output["path"], "/changes/t");
}

#[test]
fn start_refuses_the_old_repo_input_and_admits_the_same_path_as_mount() {
    let board = Changes::new();
    let mount = board.mount_x(&[("README.md", "x")]);
    let refused = board.refusal("start", &json!({"repo": mount, "task": TASK, "title": "T"}));
    assert_eq!(refused["code"], "board::cli::usage");
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T"}),
    );
    assert_eq!(output["path"], "/changes/t");
}

#[test]
fn start_without_a_task_adds_one_titled_as_the_change_through_tasks_add() {
    let board = Changes::with_tasks();
    let mount = board.mount_x(&[("README.md", "x")]);
    let output = board.call(
        "start",
        &json!({"mount": mount, "title": "Ship it", "task_repo": "/"}),
    );
    assert_eq!(output["task"], "/tasks/ship-it.md");
    assert_eq!(
        board.read("changes/ship-it.json")["task"],
        "/tasks/ship-it.md"
    );
    let task = board.text("tasks/ship-it.md");
    assert!(task.contains(r#"title: "Ship it""#), "{task}");
}

#[test]
fn start_refuses_to_route_a_new_task_from_a_repository_with_no_origin_and_admits_a_named_repo() {
    let board = Changes::with_tasks();
    let mount = board.mount_x(&[("README.md", "x")]);
    let refused = board.refusal("start", &json!({"mount": mount, "title": "Ship it"}));
    assert_eq!(refused["code"], "changes::task::unrouted");
    let output = board.call(
        "start",
        &json!({"mount": mount, "title": "Ship it", "task_repo": "/"}),
    );
    assert_eq!(output["path"], "/changes/ship-it");
}

#[test]
fn start_refuses_a_task_that_is_not_a_task_record_and_admits_one_that_is() {
    let board = Changes::new();
    let mount = board.mount_x(&[("README.md", "x")]);
    board.write(
        "notes/a.json",
        &typed("plans/decision/v1", &json!({"refs": []})),
    );
    let refused = board.refusal(
        "start",
        &json!({"mount": mount, "task": "/notes/a", "title": "T"}),
    );
    assert_eq!(refused["code"], "changes::ref::wrong_type");
    let output = board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "T"}),
    );
    assert_eq!(output["task"], TASK);
}

/// The flow version the change at `path` references, checked to be
/// `/flows/<name>/` and 16 hex digits.
fn version_of(board: &Changes, path: &str) -> String {
    let change = board.read(&format!("{}.json", path.trim_start_matches('/')));
    let version = change["flow"].as_str().unwrap().to_owned();
    let (folder, digits) = version.rsplit_once('/').unwrap();
    assert!(folder.starts_with("/flows/"), "{version}");
    assert!(
        digits.len() == 16 && digits.chars().all(|digit| digit.is_ascii_hexdigit()),
        "{version}"
    );
    version
}

/// The files under `folder`, from the board's root, by name.
fn files_in(board: &Changes, folder: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(board.root().join(folder))
        .map(|read| {
            read.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn two_starts_on_the_same_flow_store_one_version_and_reference_the_same_path() {
    let board = Changes::new();
    let mount = board.mount_x(&[("README.md", "x")]);
    for title in ["A", "B"] {
        board.call(
            "start",
            &json!({"mount": mount, "task": TASK, "title": title}),
        );
    }
    let (a, b) = (
        version_of(&board, "/changes/a"),
        version_of(&board, "/changes/b"),
    );
    assert_eq!(a, b);
    let stored = files_in(&board, "flows/development");
    assert_eq!(stored, [format!("{}.json", a.rsplit_once('/').unwrap().1)]);
    assert_eq!(files_in(&board, "changes/a"), ["plans"]);
    assert_eq!(files_in(&board, "changes/b"), ["plans"]);
}

/// Two starts that both find the version missing both write its line. The
/// line takes no expect, so the second, which finds the version the first
/// wrote, writes the same bytes and succeeds; a line that expected none
/// would be stale.
#[test]
fn writing_a_version_over_the_same_version_succeeds_and_expecting_none_there_is_stale() {
    let board = Changes::new();
    let mount = board.mount_x(&[("README.md", "x")]);
    board.call(
        "start",
        &json!({"mount": mount, "task": TASK, "title": "A"}),
    );
    let version = version_of(&board, "/changes/a");
    let file = format!("{}.json", version.trim_start_matches('/'));
    let record = board.read(&file);
    let racing = json!({"path": version, "record": record});
    let written = board.json_with(&["put", "--batch"], &format!("{racing}\n"));
    assert_eq!(written["code"], Value::Null, "{written}");
    assert_eq!(board.read(&file), record);
    let expecting = json!({"path": version, "expect": "none", "record": record});
    let stale = board.json_with(&["put", "--batch"], &format!("{expecting}\n"));
    assert_eq!(stale["code"], "board::put::stale", "{stale}");
}
