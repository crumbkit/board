//! The plugins built into the binary, run in-process: controls, plans,
//! requests and evals, beside installed plugins.

use super::plugins::{TASKS, installed};
use super::*;

/// Runs `board` in `directory` with the binary's built-in plugins and those
/// installed in `data`, if any, and returns what the run recorded.
fn built_in(directory: &Path, data: Option<&Scenario>, arguments: &[&str]) -> (Output, Ran) {
    cached(directory, data, None, arguments)
}

/// Runs `board` as [`built_in`] does, keeping its index in `cache`, or in
/// memory without one.
fn cached(
    directory: &Path,
    data: Option<&Scenario>,
    cache: Option<&Path>,
    arguments: &[&str],
) -> (Output, Ran) {
    let context = Context {
        start: directory.to_path_buf(),
        home: None,
        cache: cache.map(Path::to_path_buf),
        plugins: data.map(|data| data.workdir().join("plugins")),
        boards: None,
        signer: None,
        allowed_signers: None,
        environment: board_plugin::Environment {
            path: Some("/usr/bin:/bin".into()),
            ..board_plugin::Environment::default()
        },
        complete: None,
        terminal: false,
        browser: None,
        binary: None,
        cursor: None,
        built_in: board_plugins::BUILT_IN,
    };
    let arguments = std::iter::once(BINARY_NAME)
        .chain(arguments.iter().copied())
        .map(OsString::from);
    let mut ran = Ran::default();
    let output = run(arguments, &context, &mut &b""[..], &mut ran);
    (output, ran)
}

/// `plugin list --json`'s row for `name`.
fn listed(board: &Scenario, data: &Scenario, name: &str) -> Value {
    let (output, _) = built_in(board.workdir(), Some(data), &["plugin", "list", "--json"]);
    let rows = json(&output)["data"].as_array().unwrap().clone();
    (rows.into_iter()).find(|row| row["name"] == name).unwrap()
}

#[test]
fn a_built_in_plugin_is_listed_without_a_program_to_run_and_an_installed_one_keeps_its_own() {
    let (board, data) = installed();
    assert_eq!(listed(&board, &data, "plans").get("run"), None);
    assert_eq!(listed(&board, &data, "tasks")["run"], json!(["./run"]));
}

#[test]
fn an_installed_copy_of_a_built_in_plugin_is_listed_with_its_directory_and_one_under_another_name_is_not()
 {
    for (name, shadowed) in [("plans", true), ("tusks", false)] {
        let (board, data) = installed();
        let manifest = TASKS.replace(r#""tasks""#, &format!("{name:?}"));
        data.write(
            &format!("plugins/{name}/plugin.json"),
            Item::File(&manifest),
        );
        let directory = data.workdir().join("plugins").join(name);
        let row = listed(&board, &data, "plans");
        let (output, _) = built_in(board.workdir(), Some(&data), &["plugin", "list"]);
        let line = (text(&output.stdout).lines())
            .find(|line| line.contains(" plans "))
            .unwrap()
            .to_owned();
        if shadowed {
            assert_eq!(row["code"], "board::plugin::duplicate");
            assert_eq!(row["shadows"], json!(directory.display().to_string()));
            let expected = format!(
                "degraded board::plugin::duplicate plans 0.1.0 disabled: installed again at {} offers open",
                directory.display()
            );
            assert_eq!(line, expected);
        } else {
            assert_eq!((row.get("code"), row.get("shadows")), (None, None));
            assert_eq!(line, "ok plans 0.1.0 disabled offers open");
        }
    }
}

/// A board that enables plans, holding an open question under
/// `projects/a` and another under `projects/b`.
fn questions() -> Scenario {
    let board = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    board.write(".board/board.json", Item::File(config));
    let (output, _) = built_in(board.workdir(), None, &["plugin", "enable", "plans"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    for project in ["a", "b"] {
        let question = json!({
            "$type": "plans/question/v1",
            "question": "Which way?",
            "session": "local_0a1b",
            "asked": "2026-10-01T09:00:00Z",
            "status": "open",
        });
        let path = format!("projects/{project}/questions/q.json");
        board.write(&path, Item::File(&question.to_string()));
    }
    board
}

#[test]
fn call_plans_open_reads_its_path_flag_from_the_current_directory_and_starts_no_process() {
    let board = questions();
    let projects = board.workdir().join("projects");
    for (path, expected) in [
        ("a", "/projects/a/questions/q"),
        ("b", "/projects/b/questions/q"),
    ] {
        let arguments = ["--json", "call", "plans.open", "--path", path];
        let (output, ran) = built_in(&projects, None, &arguments);
        assert_eq!(output.code, 0, "{}", text(&output.stdout));
        let questions = &json(&output)["data"]["output"]["questions"];
        assert_eq!(questions.as_array().unwrap().len(), 1, "{questions}");
        assert_eq!(questions[0]["path"], expected);
        assert_eq!(ran.trace.work.get("started"), Some(&0));
    }
}

/// A question of plans in `status`, as its schema admits it.
fn question(status: &str) -> String {
    let question = json!({
        "$type": "plans/question/v1",
        "question": "Which way?",
        "session": "local_0a1b",
        "asked": "2026-10-01T09:00:00Z",
        "status": status,
    });
    question.to_string()
}

#[test]
fn call_plans_open_answers_from_a_warm_index_as_from_a_cold_one_after_the_mount_moves() {
    let board = questions();
    let library = Scenario::new();
    let first = library.commit(&[("questions/l.json", &question("open"))]);
    let second = library.commit(&[("notes.md", "No questions.")]);
    let mount = |commit| {
        let repo = library.path();
        let mount =
            json!({"$type": "board/mount/v1", "repo": repo, "follow": "main", "commit": commit});
        mount.to_string()
    };
    board.write("vendor.json", Item::File(&mount(first.to_string())));
    let cache = tempfile::tempdir().unwrap();
    let answer = |cache: Option<&Path>| {
        let arguments = ["--json", "call", "plans.open", "--limit", "20"];
        let (output, _) = cached(board.workdir(), None, cache, &arguments);
        assert_eq!(output.code, 0, "{}", text(&output.stdout));
        json(&output)["data"]["output"].clone()
    };
    let before = answer(Some(cache.path()));
    board
        .write("vendor.json", Item::File(&mount(second.to_string())))
        .write("projects/c/questions/q.json", Item::File(&question("open")))
        .write("projects/a/questions/q.json", Item::Deleted);
    let warm = answer(Some(cache.path()));
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm, answer(None));
}

#[test]
fn call_plans_open_help_lists_its_input_as_flags() {
    let board = questions();
    let (output, _) = built_in(board.workdir(), None, &["call", "plans.open", "-h"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let help = text(&output.stdout);
    for flag in [
        "--path",
        "--limit",
        "The questions under a path whose status is open",
    ] {
        assert!(help.contains(flag), "{flag}: {help}");
    }
}

/// A board that enables requests, holding a pending effect at `/x` asked at
/// `asked`, showing `text`; returns its id.
fn request(board: &Scenario, text: &str, asked: &str) -> String {
    let mut effect = board_model::Effect::new(
        "/b".to_owned(),
        "review".parse().unwrap(),
        "review.comment".to_owned(),
        "/x".to_owned(),
    )
    .showing(None, Some(text.to_owned()));
    effect.asked = Some(asked.to_owned());
    let id = effect.id();
    let effect = serde_json::to_string(&effect).unwrap();
    board.write(&format!("effects/pending/{id}.json"), Item::File(&effect));
    id
}

#[test]
fn call_requests_thread_answers_from_a_warm_index_as_from_a_cold_one_after_a_new_version_and_reply()
{
    let board = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    board.write(".board/board.json", Item::File(config));
    let (output, _) = built_in(board.workdir(), None, &["plugin", "enable", "requests"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let first = request(&board, "one\n", "2026-10-30T09:00:00.000000Z");
    let cache = tempfile::tempdir().unwrap();
    let answer = |cache: Option<&Path>| {
        let arguments = ["--json", "call", "requests.thread", "--effect", &first];
        let (output, ran) = cached(board.workdir(), None, cache, &arguments);
        assert_eq!(output.code, 0, "{}", text(&output.stdout));
        assert_eq!(ran.trace.work.get("started"), Some(&0));
        json(&output)["data"]["output"].clone()
    };
    let before = answer(Some(cache.path()));
    let second = request(&board, "two\n", "2026-10-30T10:00:00.000000Z");
    let reply = json!({"$type": "requests/reply/v1", "effect": first, "text": "Shorter."});
    let path = format!("requests/replies/{first}/20261030T093000Z.json");
    board.write(&path, Item::File(&reply.to_string()));
    let warm = answer(Some(cache.path()));
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm["versions"][1]["id"], json!(second));
    assert_eq!(warm["replies"][0]["text"], "Shorter.");
    assert_eq!(warm, answer(None));
}

/// An eval about gix in `status`, as its schema admits it.
fn eval(status: &str) -> String {
    let eval = json!({
        "$type": "evals/eval/v1",
        "question": "Is gix faster?",
        "subjects": ["gix"],
        "status": status,
    });
    eval.to_string()
}

/// A board that enables evals, holding a planned eval at `/evals/a`.
fn evals() -> Scenario {
    let board = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    board.write(".board/board.json", Item::File(config));
    let (output, _) = built_in(board.workdir(), None, &["plugin", "enable", "evals"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    board.write("evals/a.json", Item::File(&eval("planned")));
    board
}

#[test]
fn call_evals_about_and_open_answer_from_a_warm_index_as_from_a_cold_one_after_evals_change() {
    let board = evals();
    let cache = tempfile::tempdir().unwrap();
    let answer = |cache: Option<&Path>, arguments: &[&str]| {
        let (output, ran) = cached(board.workdir(), None, cache, arguments);
        assert_eq!(output.code, 0, "{}", text(&output.stdout));
        assert_eq!(ran.trace.work.get("started"), Some(&0));
        json(&output)["data"]["output"].clone()
    };
    let calls: [&[&str]; 2] = [
        &["--json", "call", "evals.about", "--subject", "GIX"],
        &["--json", "call", "evals.open"],
    ];
    let before = calls.map(|arguments| answer(Some(cache.path()), arguments));
    board
        .write("evals/a.json", Item::File(&eval("abandoned")))
        .write("evals/b.json", Item::File(&eval("running")));
    for (arguments, before) in calls.iter().zip(before) {
        let warm = answer(Some(cache.path()), arguments);
        assert_ne!(warm, before, "the change is seen");
        assert_eq!(warm, answer(None, arguments));
    }
}

#[test]
fn call_evals_about_help_lists_its_input_as_flags() {
    let board = evals();
    let (output, _) = built_in(board.workdir(), None, &["call", "evals.about", "-h"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let help = text(&output.stdout);
    for flag in ["--limit", "--mount", "--subject", "whatever their status"] {
        assert!(help.contains(flag), "{flag}: {help}");
    }
}

/// A control in `/controls/docs` that limits `docs/*.md` to `max` words.
fn docs_budget(max: u64) -> String {
    let control = json!({
        "$type": "controls/control/v1",
        "requirement": "Short docs.",
        "purpose": "A test.",
        "paths": ["docs/*.md"],
        "rule": {"budget": "words", "max": max},
    });
    control.to_string()
}

#[test]
fn call_controls_check_answers_from_a_warm_index_as_from_a_cold_one_after_a_control_changes() {
    let board = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    board.write(".board/board.json", Item::File(config));
    let (output, _) = built_in(board.workdir(), None, &["plugin", "enable", "controls"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    board
        .write("docs/a.md", Item::File("one two three"))
        .write("controls/docs.json", Item::File(&docs_budget(9)));
    let cache = tempfile::tempdir().unwrap();
    let answer = |cache: Option<&Path>| {
        let arguments = ["--json", "call", "controls.check", "--path", "/docs"];
        let (output, ran) = cached(board.workdir(), None, cache, &arguments);
        assert_eq!(output.code, 0, "{}", text(&output.stdout));
        assert_eq!(ran.trace.work.get("started"), Some(&0));
        json(&output)["data"]["output"].clone()
    };
    let before = answer(Some(cache.path()));
    assert_eq!(before["measured"][0]["used"], 3);
    board
        .write("docs/b.md", Item::File("four"))
        .write("controls/docs.json", Item::File(&docs_budget(8)));
    let warm = answer(Some(cache.path()));
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm["measured"][0]["max"], 8);
    assert_eq!(warm, answer(None));
}
