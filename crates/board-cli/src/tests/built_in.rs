//! The plugins built into the binary, run in-process: controls, plans,
//! requests, evals and runs, beside installed plugins.

use std::collections::BTreeMap;

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
    cached_with(directory, data, cache, PATH, arguments, b"")
}

/// The `PATH` the built-in plugins' processes see.
const PATH: &str = "/usr/bin:/bin";

/// [`cached`] with `path` as the `PATH` and `stdin` as the standard input.
fn cached_with(
    directory: &Path,
    data: Option<&Scenario>,
    cache: Option<&Path>,
    path: &str,
    arguments: &[&str],
    stdin: &[u8],
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
            path: Some(path.into()),
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
    let output = run(arguments, &context, &mut &stdin[..], &mut ran);
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

/// A full run of a clean revision of `/suites/a`, started on day `day` of
/// October 2026, whose `wall_s` is `wall_s`.
fn suite_run(day: u8, wall_s: f64) -> String {
    let run = json!({
        "$type": "runs/run/v1",
        "suite": "/suites/a",
        "by": "claude",
        "started": format!("2026-10-0{day}T09:00:00Z"),
        "scope": "full",
        "dirty": false,
        "results": [],
        "measures": {"wall_s": wall_s},
    });
    run.to_string()
}

#[test]
fn call_runs_latest_and_trend_answer_from_a_warm_index_as_from_a_cold_one_after_a_run_lands() {
    let board = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    board.write(".board/board.json", Item::File(config));
    let (output, _) = built_in(board.workdir(), None, &["plugin", "enable", "runs"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let suite = r#"{"$type": "runs/suite/v1", "subjects": ["board"], "method": "Run it."}"#;
    board
        .write("suites/a.json", Item::File(suite))
        .write("suites/a/runs/1.json", Item::File(&suite_run(1, 50.0)));
    let cache = tempfile::tempdir().unwrap();
    let answer = |cache: Option<&Path>, arguments: &[&str]| {
        let (output, ran) = cached(board.workdir(), None, cache, arguments);
        assert_eq!(output.code, 0, "{}", text(&output.stdout));
        assert_eq!(ran.trace.work.get("started"), Some(&0));
        let mut output = json(&output)["data"]["output"].clone();
        // The age runs with the clock, so a later call may give a larger one.
        if let Some(run) = output.get_mut("run").and_then(Value::as_object_mut) {
            run.remove("age_s");
        }
        output
    };
    let calls: [&[&str]; 2] = [
        &["--json", "call", "runs.latest", "--suite", "/suites/a"],
        &[
            "--json",
            "call",
            "runs.trend",
            "--suite",
            "/suites/a",
            "--measure",
            "wall_s",
        ],
    ];
    let before = calls.map(|arguments| answer(Some(cache.path()), arguments));
    board.write("suites/a/runs/2.json", Item::File(&suite_run(2, 42.5)));
    for (arguments, before) in calls.iter().zip(before) {
        let warm = answer(Some(cache.path()), arguments);
        assert_ne!(warm, before, "the change is seen");
        assert_eq!(warm, answer(None, arguments));
    }
}

/// A board that does not commit, with built-in plugins enabled and an index
/// cache of its own. The parity modules below run the commands each plugin's
/// former pytest suite ran through the binary. The plugins' processes see
/// `path` as their `PATH`.
struct Fixture {
    board: Scenario,
    cache: tempfile::TempDir,
    path: String,
}

impl Fixture {
    /// A board that enables each of `plugins` with `board plugin enable`.
    fn enabling(plugins: &[&str]) -> Self {
        let board = Scenario::new();
        let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
        board.write(".board/board.json", Item::File(config));
        let fixture = Self {
            board,
            cache: tempfile::tempdir().unwrap(),
            path: PATH.to_owned(),
        };
        for plugin in plugins {
            let enabled = fixture.run(&["plugin", "enable", plugin]);
            assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
        }
        fixture
    }

    fn write(&self, path: &str, text: &str) -> &Self {
        self.board.write(path, Item::File(text));
        self
    }

    fn delete(&self, path: &str) {
        self.board.write(path, Item::Deleted);
    }

    /// The text of the file at `path` from the board's root.
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.board.workdir().join(path)).unwrap()
    }

    /// Runs `board` in the board's root with its cache.
    fn run(&self, arguments: &[&str]) -> Output {
        self.run_with(arguments, "")
    }

    /// Runs `board` in the board's root with its cache and `stdin` as its
    /// standard input.
    fn run_with(&self, arguments: &[&str], stdin: &str) -> Output {
        cached_with(
            self.board.workdir(),
            None,
            Some(self.cache.path()),
            &self.path,
            arguments,
            stdin.as_bytes(),
        )
        .0
    }

    /// Runs `board --json` and reads its one document.
    fn json(&self, arguments: &[&str]) -> Value {
        let arguments: Vec<&str> = std::iter::once("--json")
            .chain(arguments.iter().copied())
            .collect();
        json(&self.run(&arguments))
    }

    /// The output of `board call <target>` with `flags`, which starts no
    /// process. With `cold`, the call keeps its index in memory, as after
    /// the cache is deleted.
    fn answer(&self, target: &str, flags: &[&str], cold: bool) -> Value {
        let arguments: Vec<&str> = ["--json", "call", target]
            .into_iter()
            .chain(flags.iter().copied())
            .collect();
        let cache = (!cold).then(|| self.cache.path());
        let (output, ran) = cached_with(
            self.board.workdir(),
            None,
            cache,
            &self.path,
            &arguments,
            b"",
        );
        assert_eq!(
            output.code,
            0,
            "{target} {flags:?}: {}",
            text(&output.stdout)
        );
        assert_eq!(ran.trace.work.get("started"), Some(&0));
        let report = json(&output);
        assert_eq!(report["schema"], "board/call-report/v2", "{report}");
        report["data"]["output"].clone()
    }

    fn call(&self, target: &str, flags: &[&str]) -> Value {
        self.answer(target, flags, false)
    }

    /// The failure `board call <target>` prints when it is refused.
    fn refusal(&self, target: &str, flags: &[&str]) -> Value {
        let arguments: Vec<&str> = ["--json", "call", target]
            .into_iter()
            .chain(flags.iter().copied())
            .collect();
        let output = self.run(&arguments);
        assert_ne!(output.code, 0, "{}", text(&output.stdout));
        json(&output)
    }

    /// Each entry's code from `board check --all` whose path starts with
    /// `prefix`; `None` when it is whole.
    fn checked(&self, prefix: &str) -> BTreeMap<String, Option<String>> {
        let report = self.json(&["check", "--all"]);
        (report["data"].as_array().unwrap().iter())
            .filter_map(|entry| {
                let logical = entry["logical"].as_str()?;
                let code = entry["code"].as_str().map(str::to_owned);
                logical
                    .starts_with(prefix)
                    .then(|| (logical.to_owned(), code))
            })
            .collect()
    }
}

/// `base` with each of `fields` added or replacing its own, as JSON text.
fn record(mut base: Value, fields: &Value) -> String {
    for (name, value) in fields.as_object().unwrap() {
        base[name] = value.clone();
    }
    base.to_string()
}

/// `entries` by path, each with `code`, as [`Fixture::checked`] gives them.
fn codes(entries: &[(&str, Option<&str>)]) -> BTreeMap<String, Option<String>> {
    (entries.iter())
        .map(|(path, code)| ((*path).to_owned(), code.map(str::to_owned)))
        .collect()
}

/// The schemas the built-in `plugin` ships, by their path in a board.
fn shipped(plugin: &str) -> Vec<(String, String)> {
    let plugin = (board_plugins::BUILT_IN.0.iter())
        .find(|built| built.manifest().name.as_str() == plugin)
        .unwrap();
    (plugin.schemas().into_iter())
        .map(|(name, schema)| {
            let path = format!(".board/types/{}.schema.json", name.as_str());
            (
                path,
                board_model::canonical_json(&schema).as_str().to_owned(),
            )
        })
        .collect()
}

/// A new repository holding one commit of a board that enables each of
/// `plugins`, with the schemas they ship and `files` by path, and the
/// commit.
fn repository(plugins: &[&str], files: &[(&str, String)]) -> (Scenario, ObjectId) {
    let repository = Scenario::new();
    let config = json!({"$type": "board/config/v1", "plugins": plugins}).to_string();
    let schemas: Vec<(String, String)> =
        plugins.iter().flat_map(|plugin| shipped(plugin)).collect();
    let mut tree: Vec<(&str, &str)> = vec![(".board/board.json", &config)];
    tree.extend(
        schemas
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str())),
    );
    tree.extend(files.iter().map(|(path, text)| (*path, text.as_str())));
    let commit = repository.commit(&tree);
    (repository, commit)
}

/// Mounts `repository` at `commit` at `path` in `fixture`.
fn mount(fixture: &Fixture, path: &str, repository: &Scenario, commit: ObjectId) {
    fixture.write(path, &mount_of(&repository.path(), commit));
}

mod controls;
mod evals;
mod jira;
mod plans;
mod runs;
