//! The changes plugin through the binary, against real git repositories
//! mounted beside the board and a stub gh. These were the plugin's pytest
//! suite: `conftest.py` is this module's fixture, and this module holds
//! `test_open.py`, `test_tickets.py` and the listing test of
//! `test_manifest.py`. The pure rules of the gates, the diff, the WIP limit,
//! the pull request body and the manifest are the crate's own tests.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;

mod authoring;
mod brief;
mod flows;
mod migrate;
mod phases;
mod plan;
mod pr;
mod prototype;
mod run;
mod start;
mod sync;
mod wip;

/// The schemas the changes plugin ships, which enabling it writes.
const CHANGE_SCHEMA: &str =
    include_str!("../../../../board-plugin-changes/types/changes/change/v1.schema.json");
const CHANGE_V2_SCHEMA: &str =
    include_str!("../../../../board-plugin-changes/types/changes/change/v2.schema.json");
const FLOW_SCHEMA: &str =
    include_str!("../../../../board-plugin-changes/types/changes/flow/v1.schema.json");
const PLAN_SCHEMA: &str =
    include_str!("../../../../board-plugin-changes/types/changes/plan/v1.schema.json");
const UNDERSTANDING_SCHEMA: &str =
    include_str!("../../../../board-plugin-changes/types/changes/understanding/v1.schema.json");
const PROTOTYPE_SCHEMA: &str =
    include_str!("../../../../board-plugin-changes/types/changes/prototype/v1.schema.json");

const RUN_SCHEMA: &str =
    include_str!("../../../../board-plugin-changes/types/changes/run/v1.schema.json");
const STOP_SCHEMA: &str =
    include_str!("../../../../board-plugin-changes/types/changes/stop/v1.schema.json");
const REOPEN_SCHEMA: &str =
    include_str!("../../../../board-plugin-changes/types/changes/reopen/v1.schema.json");

const HEAD: &str = "a3f9c0de1234567890abcdef1234567890abcdef";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
/// Types other plugins own, which a change references. The fixture board
/// keeps a loose schema of its own for each.
const FOREIGN_TYPES: [&str; 7] = [
    "tasks/task/v2",
    "runs/suite/v1",
    "runs/run/v1",
    "plans/decision/v1",
    "jira/issue/v1",
    "github/issue/v1",
    "github/pull/v1",
];
const REVIEW_TYPE: &str = "review/report/v1";
/// The task every fixture change does, which the fixture board holds.
const TASK: &str = "/tasks/given.md";
const PLAN: &str = "# Timeout\n\n## Problem\n\nSuites hang.\n\n## Outcome\n\nA suite stops.\n\n## Out of scope\n\nRetries.\n\n## UX\n\nNo UI change: suites run headless.\n\n## Data\n\nNone.\n";
/// The paths a fixture plan declares: everything under the fixture
/// change's mount.
const PATHS: [&str; 1] = ["/repos/x/**"];
/// The owner's decision approving /changes/t's plan, which [`Changes::approved`]
/// writes.
const APPROVED: &str = "/changes/t/approved";

/// A review report as the review plugin's design defines it.
fn review_schema() -> Value {
    json!({
        "type": "object",
        "required": ["range", "patch_ids", "findings", "verdict", "by"],
        "properties": {
            "brief": {"type": "string", "format": "board-ref"},
            "range": {"type": "string", "pattern": "^[0-9a-f]{7,40}\\.\\.[0-9a-f]{7,40}$"},
            "patch_ids": {"type": "array", "items": {"type": "string"}},
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["severity", "text"],
                    "properties": {
                        "severity": {"enum": ["critical", "high", "medium", "low"]},
                        "file": {"type": "string"},
                        "line": {"type": "integer"},
                        "text": {"type": "string"},
                    },
                },
            },
            "verdict": {"enum": ["approve", "changes", "comment"]},
            "by": {"type": "string"},
            "cost": {"type": "object"},
            "source": {"type": "string"},
            "follows": {"type": "string", "format": "board-ref"},
        },
    })
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

/// Today's UTC day, on which a fixture change was last touched, so it is
/// active under the WIP limit.
fn today() -> String {
    days_ago(0)
}

/// Runs git in `directory` with an identity of its own and returns what it
/// printed, trimmed.
#[expect(
    clippy::disallowed_methods,
    reason = "the mounted repositories are git's, which the fixture commits to as a builder would"
)]
fn git(directory: &Path, arguments: &[&str]) -> String {
    let ran = std::process::Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
        ])
        .args(arguments)
        .current_dir(directory)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "git {arguments:?} in {}: {}{}",
        directory.display(),
        text(&ran.stdout),
        text(&ran.stderr)
    );
    text(&ran.stdout).trim().to_owned()
}

/// Writes `text` at `file` in a repository the board mounts, as a builder's
/// editor does.
#[expect(
    clippy::disallowed_methods,
    reason = "the mounted repositories are git's, which the fixture writes as a builder would"
)]
fn write_file(file: &Path, text: &str) {
    make_dir(file.parent().unwrap());
    std::fs::write(file, text).unwrap();
}

/// Makes the directory `directory` beside the board, for a repository git
/// then initialises there.
#[expect(
    clippy::disallowed_methods,
    reason = "the mounted repositories are git's, which the fixture makes as a builder would"
)]
fn make_dir(directory: &Path) {
    std::fs::create_dir_all(directory).unwrap();
}

/// A record of `type_name` holding `fields`, as JSON text.
fn typed(type_name: &str, fields: &Value) -> String {
    record(json!({"$type": type_name}), fields)
}

/// A `tasks/task/v2` Markdown task titled "A" in `status`.
fn task(status: &str) -> String {
    format!("---\n$type: tasks/task/v2\ntitle: A\nstatus: {status}\n---\n")
}

/// A review report of `base..head` with `verdict`, no findings and
/// `fields`.
fn report(head: &str, verdict: &str, base: &str, fields: &Value) -> String {
    let body = json!({
        "range": format!("{base}..{head}"),
        "patch_ids": ["p1"],
        "findings": [],
        "verdict": verdict,
        "by": "a reviewer",
    });
    typed(
        REVIEW_TYPE,
        &serde_json::from_str(&record(body, fields)).unwrap(),
    )
}

/// A `changes/change/v1` record in `phase`, touched today, with `fields`.
fn change(phase: &str, fields: &Value) -> String {
    let defaults = json!({
        "$type": "changes/change/v1",
        "title": "Let a suite declare a timeout",
        "mount": "/repos/x",
        "phase": phase,
        "task": TASK,
        "touched": today(),
    });
    record(defaults, fields)
}

/// The input of `changes.plan` for the change at `path`, declaring
/// `paths`, with the fixture plan.
fn plan_input(path: &str, paths: &[&str]) -> Value {
    json!({"path": path, "text": PLAN, "uses": [], "paths": paths})
}

/// `given` as the flags of `board call`: each string as it is, each number
/// and boolean as JSON.
fn flags(given: &Value) -> Vec<String> {
    let mut parts = Vec::new();
    for (name, value) in given.as_object().unwrap() {
        let shown = match value {
            Value::String(text) => text.clone(),
            Value::Number(_) | Value::Bool(_) => value.to_string(),
            other => panic!("{name} is {other}, which no flag can give"),
        };
        parts.extend([format!("--{name}"), shown]);
    }
    parts
}

/// A board that does not commit, with changes and controls enabled, a
/// loose schema for each cited type, the task at [`TASK`], the git
/// repositories it mounts beside it, and a directory first on the plugins'
/// `PATH` that may hold a stub gh.
struct Changes {
    board: Scenario,
    /// The mounted repositories and other clones, canonical, so a path
    /// compares with the one Board reports.
    beside: PathBuf,
    _beside: tempfile::TempDir,
    /// The directory first on the plugins' `PATH`.
    bin: Scenario,
    cache: tempfile::TempDir,
    /// The known boards file Board reads, when a test gives one.
    boards: Option<PathBuf>,
}

impl Changes {
    /// The fixture board, with changes and controls enabled.
    fn new() -> Self {
        Self::enabling(&["changes", "controls"])
    }

    /// The fixture board with Board's built-in tasks plugin enabled too, so
    /// `tasks.add` writes tasks to its own tree.
    fn with_tasks() -> Self {
        Self::enabling(&["changes", "controls", "tasks"])
    }

    fn enabling(plugins: &[&str]) -> Self {
        let beside = tempfile::tempdir().unwrap();
        let fixture = Self {
            board: Scenario::new(),
            beside: beside.path().canonicalize().unwrap(),
            _beside: beside,
            bin: Scenario::new(),
            cache: tempfile::tempdir().unwrap(),
            boards: None,
        };
        fixture.write(
            ".board/board.json",
            r#"{"$type": "board/config/v1", "commit_on_put": false}"#,
        );
        for name in FOREIGN_TYPES {
            fixture.write(
                &format!(".board/types/{name}.schema.json"),
                r#"{"type": "object"}"#,
            );
        }
        fixture.write(
            &format!(".board/types/{REVIEW_TYPE}.schema.json"),
            &review_schema().to_string(),
        );
        fixture.write(TASK.trim_start_matches('/'), &task("open"));
        for plugin in plugins {
            let enabled = fixture.run(&["plugin", "enable", plugin], "");
            assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
        }
        fixture
    }

    /// Writes a file of the board's own tree at `path`, from its root.
    fn write(&self, path: &str, text: &str) {
        self.board.write(path, Item::Fresh(text));
    }

    fn delete(&self, path: &str) {
        self.board.write(path, Item::Deleted);
    }

    /// Writes a change in `phase` at `file`, such as `changes/t.json`, with
    /// `fields`, and the records the change's fields would name: `records`
    /// and a plan file named by `plan` go on a plan record at
    /// `<change>/plans/1`, a `pull_request` or `issue` page becomes a
    /// GitHub record the change's `pulls` or `tickets` names, and `kind`
    /// prototype with its `question`, `state`, `verdict` and
    /// `verdict_artifact` becomes the change's prototype record.
    fn change_at(&self, file: &str, phase: &str, fields: &Value) {
        let at = format!("/{}", file.trim_end_matches(".json"));
        let mut kept = fields.as_object().unwrap().clone();
        let records = kept.remove("records");
        let named = kept.remove("plan");
        // A plan that `changes.plan` wrote since stays the change's plan, and
        // the records go on it, as `attach --ref` would put them.
        let current = (named.is_none() && self.exists(file))
            .then(|| self.read(file)["plan"].as_str().map(str::to_owned))
            .flatten()
            .filter(|plan| *plan != format!("{at}/plans/1"));
        if let Some(plan) = current {
            let held = format!("{}.json", plan.trim_start_matches('/'));
            let mut record = self.read(&held);
            if let Some(records) = records {
                record["records"] = records;
            }
            self.write(&held, &serde_json::to_string(&record).unwrap());
            kept.insert("plan".to_owned(), json!(plan));
        } else if records.is_some() || named.is_some() {
            let text = match named.as_ref().and_then(Value::as_str) {
                Some(named) if std::path::Path::new(named).extension() != Some("md".as_ref()) => {
                    kept.insert("plan".to_owned(), json!(named));
                    None
                }
                Some(named) => Some(named.to_owned()),
                None => None,
            };
            if kept.get("plan").is_none() {
                let written = text.unwrap_or_else(|| {
                    let written = format!("{at}/plans/1.md");
                    self.write(written.trim_start_matches('/'), "# Plan\n");
                    written
                });
                let mut plan = json!({"text": written, "uses": []});
                if let Some(records) = records {
                    plan["records"] = records;
                }
                self.write(
                    &format!("{}/plans/1.json", at.trim_start_matches('/')),
                    &typed("changes/plan/v1", &plan),
                );
                kept.insert("plan".to_owned(), json!(format!("{at}/plans/1")));
            }
        }
        for (key, list, kind) in [
            ("pull_request", "pulls", "pull"),
            ("issue", "tickets", "issue"),
        ] {
            let Some(url) = kept.remove(key) else {
                continue;
            };
            let url = url.as_str().unwrap().to_owned();
            let parts: Vec<&str> = url
                .trim_start_matches("https://github.com/")
                .split('/')
                .collect();
            let [owner, repo, _, number] = parts.as_slice() else {
                panic!("{url} is no GitHub page");
            };
            let path = format!("gh/{owner}/{repo}/{kind}s/{number}");
            let body = json!({
                "owner": owner,
                "repo": repo,
                "number": number.parse::<u64>().unwrap(),
                "url": url,
            });
            self.write(
                &format!("{path}.json"),
                &typed(&format!("github/{kind}/v1"), &body),
            );
            kept.insert(list.to_owned(), json!([format!("/{path}")]));
        }
        let kind = kept.remove("kind");
        // The phase gives a prototype's state, so the record keeps none.
        kept.remove("state");
        let mut prototype = json!({});
        for key in ["question", "verdict", "verdict_artifact"] {
            if let Some(value) = kept.remove(key) {
                prototype[key] = value;
            }
        }
        let held = format!("{}/prototype.json", at.trim_start_matches('/'));
        if kind.as_ref().and_then(Value::as_str) == Some("prototype") {
            if prototype.get("question").is_none() {
                prototype["question"] = json!("Does it work?");
            }
            self.write(&held, &typed("changes/prototype/v1", &prototype));
        } else if self.exists(&held) {
            self.delete(&held);
        }
        self.write(file, &change(phase, &Value::Object(kept)));
    }

    /// Writes the owner's decision whose refs name /changes/t, which the
    /// plan gate's owner-approval check needs, and returns its path for a
    /// plan record to attach.
    fn approved(&self) -> &'static str {
        self.write(
            "changes/t/approved.json",
            &typed("plans/decision/v1", &json!({"refs": ["/changes/t"]})),
        );
        APPROVED
    }

    /// The board's root.
    fn root(&self) -> &Path {
        self.board.workdir()
    }

    /// The text of the file at `path`, from the board's root.
    fn text(&self, path: &str) -> String {
        std::fs::read_to_string(self.root().join(path)).unwrap()
    }

    /// The JSON record at `path`, from the board's root.
    fn read(&self, path: &str) -> Value {
        serde_json::from_str(&self.text(path)).unwrap()
    }

    fn exists(&self, path: &str) -> bool {
        self.root().join(path).exists()
    }

    /// Runs `board` in the board's root with `stdin`.
    fn run(&self, arguments: &[&str], stdin: &str) -> Output {
        self.run_inheriting(arguments, stdin, &[])
    }

    /// Runs `board` in the board's root, which is also the caller's
    /// directory, with only the variables a user's shell would pass it and
    /// `inherited`, which may set `HOME`.
    fn run_inheriting(
        &self,
        arguments: &[&str],
        stdin: &str,
        inherited: &[(&str, &str)],
    ) -> Output {
        let root = self.root().to_path_buf();
        let context = Context {
            start: root.clone(),
            home: None,
            cache: Some(self.cache.path().to_path_buf()),
            plugins: None,
            boards: self.boards.clone(),
            signer: None,
            allowed_signers: None,
            attention_allowed_signers: None,
            environment: board_plugin::Environment {
                path: Some(format!("{}:{PATH}", self.bin.workdir().display()).into()),
                home: (inherited.iter())
                    .find(|(name, _)| *name == "HOME")
                    .map(|(_, value)| OsString::from(value)),
                caller: Some(root.into_os_string()),
                inherited: (inherited.iter())
                    .filter(|(name, _)| *name != "HOME")
                    .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                    .collect(),
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
        run(
            arguments,
            &context,
            &mut stdin.as_bytes(),
            &mut Ran::default(),
        )
    }

    /// Runs `board --json` with `stdin` and reads its one document.
    fn json_with(&self, arguments: &[&str], stdin: &str) -> Value {
        let arguments: Vec<&str> = std::iter::once("--json")
            .chain(arguments.iter().copied())
            .collect();
        json(&self.run(&arguments, stdin))
    }

    fn json(&self, arguments: &[&str]) -> Value {
        self.json_with(arguments, "")
    }

    /// `board --json call changes.<capability>`, given as flags or, with
    /// `stdin`, as JSON on stdin: its output, or its failure.
    fn invoke(
        &self,
        capability: &str,
        given: &Value,
        stdin: bool,
        inherited: &[(&str, &str)],
    ) -> Result<Value, Value> {
        let target = format!("changes.{capability}");
        let given_flags = if stdin { Vec::new() } else { flags(given) };
        let mut arguments = vec!["--json", "call", target.as_str()];
        arguments.extend(given_flags.iter().map(String::as_str));
        let input = if stdin {
            given.to_string()
        } else {
            String::new()
        };
        let output = self.run_inheriting(&arguments, &input, inherited);
        let document = json(&output);
        if output.code == 0 {
            assert_eq!(document["schema"], "board/call-report/v2", "{document}");
            Ok(document["data"]["output"].clone())
        } else {
            Err(document)
        }
    }

    /// The output of `board changes <capability>` with `given` as
    /// flags.
    fn call(&self, capability: &str, given: &Value) -> Value {
        (self.invoke(capability, given, false, &[]))
            .unwrap_or_else(|failure| panic!("{capability} {given}: {failure}"))
    }

    /// The output of `board changes <capability>` with `given` on
    /// stdin.
    fn call_stdin(&self, capability: &str, given: &Value) -> Value {
        (self.invoke(capability, given, true, &[]))
            .unwrap_or_else(|failure| panic!("{capability} {given}: {failure}"))
    }

    /// The failure `board changes <capability>` prints, with `given`
    /// as flags.
    fn refusal(&self, capability: &str, given: &Value) -> Value {
        self.refusal_inheriting(capability, given, &[])
    }

    fn refusal_inheriting(
        &self,
        capability: &str,
        given: &Value,
        inherited: &[(&str, &str)],
    ) -> Value {
        match self.invoke(capability, given, false, inherited) {
            Ok(output) => panic!("{capability} {given} was admitted: {output}"),
            Err(failure) => failure,
        }
    }

    /// The failure with `given` on stdin.
    fn refusal_stdin(&self, capability: &str, given: &Value) -> Value {
        match self.invoke(capability, given, true, &[]) {
            Ok(output) => panic!("{capability} {given} was admitted: {output}"),
            Err(failure) => failure,
        }
    }

    /// The code `board check <path>` gives the entry at `path`, or `None`
    /// when it is valid.
    fn code(&self, path: &str) -> Option<String> {
        let report = self.json(&["check", path, "--healthy"]);
        report["data"][0]["code"].as_str().map(str::to_owned)
    }

    /// Each entry's code from `board check --healthy --all` whose path starts with
    /// `prefix`; `None` when it is valid.
    fn checked(&self, prefix: &str) -> BTreeMap<String, Option<String>> {
        let report = self.json(&["check", "--healthy", "--all"]);
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

    /// A git repository beside the board holding a board and `files`,
    /// mounted at `/<at>` and following `follow`: its logical path.
    fn mount_repo(&self, files: &[(&str, &str)], at: &str, follow: &str) -> String {
        let root = self.repo_root(at);
        make_dir(&root);
        git(&root, &["init", "-q"]);
        write_file(
            &root.join(".board/board.json"),
            r#"{"$type": "board/config/v1"}"#,
        );
        for (path, text) in files {
            write_file(&root.join(path), text);
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "fixture"]);
        git(&root, &["branch", "-M", "main"]);
        let pointer = json!({
            "$type": "board/mount/v1",
            "repo": root,
            "follow": follow,
            "commit": git(&root, &["rev-parse", "HEAD"]),
        });
        self.write(&format!("{at}.json"), &pointer.to_string());
        format!("/{at}")
    }

    /// [`Self::mount_repo`] at `repos/x`, following HEAD.
    fn mount_x(&self, files: &[(&str, &str)]) -> String {
        self.mount_repo(files, "repos/x", "HEAD")
    }

    /// Where the repository mounted at `/<at>` is.
    fn repo_root(&self, at: &str) -> PathBuf {
        self.beside.join(at.replace('/', "-"))
    }

    /// A repository mounted at /repos/x, following `main`, and a branch
    /// `work` off it whose commit writes `files`: the commit `main` holds
    /// and the head of `work`.
    fn edited(&self, files: &[(&str, &str)]) -> (String, String) {
        self.mount_repo(&[("README.md", "x\n")], "repos/x", "main");
        let root = self.repo_root("repos/x");
        let base = git(&root, &["rev-parse", "HEAD"]);
        git(&root, &["checkout", "-qb", "work"]);
        (base, self.commit(files, None))
    }

    /// Commits `files` to the repository /repos/x mounts, on `branch` or
    /// the branch checked out: the commit id.
    fn commit(&self, files: &[(&str, &str)], branch: Option<&str>) -> String {
        let root = self.repo_root("repos/x");
        if let Some(branch) = branch {
            git(&root, &["checkout", "-q", branch]);
        }
        for (path, text) in files {
            write_file(&root.join(path), text);
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "edit"]);
        git(&root, &["rev-parse", "HEAD"])
    }
}

/// Splits `line` as a POSIX shell does, for the commands the plugin
/// quotes: single quotes, double quotes and backslashes.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' => {
                words.extend(word.take());
            }
            '\'' => {
                let held = word.get_or_insert_with(String::new);
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    held.push(c);
                }
            }
            '"' => {
                let held = word.get_or_insert_with(String::new);
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => {
                            let next = chars.next().unwrap();
                            if !matches!(next, '"' | '\\' | '$' | '`') {
                                held.push('\\');
                            }
                            held.push(next);
                        }
                        other => held.push(other),
                    }
                }
            }
            '\\' => {
                let next = chars.next().unwrap();
                word.get_or_insert_with(String::new).push(next);
            }
            other => word.get_or_insert_with(String::new).push(other),
        }
    }
    words.extend(word);
    words
}

#[test]
fn board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schema_it_ships() {
    let board = Changes::new();
    let report = board.json(&["plugin", "list"]);
    let rows = report["data"].as_array().unwrap();
    let plugin = rows.iter().find(|row| row["name"] == "changes").unwrap();
    assert_eq!(
        (&plugin["status"], &plugin["enabled"]),
        (&json!("ok"), &json!(true))
    );
    assert_eq!(
        plugin["offers"],
        json!([
            "start",
            "next",
            "attach",
            "advance",
            "open",
            "pr",
            "plan",
            "brief",
            "prototype",
            "verdict",
            "pause",
            "resume",
            "reopen",
            "reflow",
            "sync",
            "drop",
            "migrate",
            "run",
            "stop"
        ])
    );
    for (kind, schema) in [
        ("change/v1", CHANGE_SCHEMA),
        ("change/v2", CHANGE_V2_SCHEMA),
        ("flow/v1", FLOW_SCHEMA),
        ("plan/v1", PLAN_SCHEMA),
        ("understanding/v1", UNDERSTANDING_SCHEMA),
        ("prototype/v1", PROTOTYPE_SCHEMA),
        ("run/v1", RUN_SCHEMA),
        ("stop/v1", STOP_SCHEMA),
        ("reopen/v1", REOPEN_SCHEMA),
    ] {
        let written = board.read(&format!(".board/types/changes/{kind}.schema.json"));
        let shipped: Value = serde_json::from_str(schema).unwrap();
        assert_eq!(written, shipped, "{kind}");
    }
}

// test_open.py: `changes.open` and the change schema.

#[test]
fn open_lists_changes_that_are_not_done_or_dropped_by_path() {
    let board = Changes::new();
    board.change_at("changes/b.json", "build", &json!({"route": "owner"}));
    board.change_at("changes/a.json", "plan", &json!({}));
    board.change_at("changes/c.json", "done", &json!({}));
    board.change_at("changes/d.json", "dropped", &json!({}));
    let output = board.call("open", &json!({}));
    assert_eq!(
        output["changes"],
        json!([
            {
                "path": "/changes/a",
                "title": "Let a suite declare a timeout",
                "mount": "/repos/x",
                "flow": {"name": "development", "phases": ["plan", "build", "review", "land"]},
                "phase": "plan",
                "route": null,
                "task": TASK,
                "kind": "build",
                "prototype": null,
            },
            {
                "path": "/changes/b",
                "title": "Let a suite declare a timeout",
                "mount": "/repos/x",
                "flow": {"name": "development", "phases": ["plan", "build", "review", "land"]},
                "phase": "build",
                "route": "owner",
                "task": TASK,
                "kind": "build",
                "prototype": null,
            },
        ])
    );
    assert_eq!(
        (
            &output["total"],
            &output["truncated"],
            &output["unreadable"]
        ),
        (&json!(2), &json!(false), &json!(0))
    );
}

#[test]
fn open_returns_at_most_the_limit_and_says_how_many_it_left_out() {
    let board = Changes::new();
    for name in ["a", "b", "c"] {
        board.change_at(&format!("changes/{name}.json"), "plan", &json!({}));
    }
    let output = board.call("open", &json!({"limit": 2}));
    let paths: Vec<&Value> = (output["changes"].as_array().unwrap().iter())
        .map(|entry| &entry["path"])
        .collect();
    assert_eq!(paths, [&json!("/changes/a"), &json!("/changes/b")]);
    assert_eq!(
        (&output["total"], &output["truncated"]),
        (&json!(3), &json!(true))
    );
}

#[test]
fn open_counts_a_degraded_change_as_unreadable_and_lists_a_valid_one() {
    let board = Changes::new();
    board.change_at("changes/bad.json", "plan", &json!({"route": "sometimes"}));
    board.change_at("changes/good.json", "plan", &json!({}));
    let output = board.call("open", &json!({}));
    let paths: Vec<&Value> = (output["changes"].as_array().unwrap().iter())
        .map(|entry| &entry["path"])
        .collect();
    assert_eq!(paths, [&json!("/changes/good")]);
    assert_eq!(output["unreadable"], 1);
}

#[test]
fn the_schema_refuses_an_unknown_phase_or_field_and_reports_a_reference_that_reaches_nothing() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board.write("tasks/a.md", &task("open"));
    board.change_at(
        "changes/good.json",
        "plan",
        &json!({"task": "/tasks/a.md", "head": "a3f9c0d"}),
    );
    board.change_at("changes/phase.json", "later", &json!({}));
    // `records` moved to the change's plans, so a change holding it is
    // invalid.
    board.write(
        "changes/extra.json",
        &change("plan", &json!({"records": ["/tasks/a.md"]})),
    );
    board.change_at(
        "changes/dangling.json",
        "plan",
        &json!({"task": "/tasks/b"}),
    );
    assert_eq!(
        board.checked("/changes/"),
        codes(&[
            ("/changes/good", None),
            ("/changes/phase", Some("board::record::invalid")),
            ("/changes/extra", Some("board::record::invalid")),
            ("/changes/dangling", Some("board::ref::dangling")),
        ])
    );
}

#[test]
fn the_schema_refuses_a_repository_by_owner_and_name_or_a_plain_directory_or_under_the_old_repo_key_and_admits_its_mount()
 {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board.change_at(
        "changes/named.json",
        "plan",
        &json!({"mount": "crumbkit/board"}),
    );
    board.write(
        "changes/old.json",
        &typed(
            "changes/change/v1",
            &json!({"title": "T", "repo": "/repos/x", "phase": "plan"}),
        ),
    );
    board.change_at(
        "changes/mounted.json",
        "plan",
        &json!({"mount": "/repos/x"}),
    );
    board.write("plain/README.md", "x");
    board.change_at("changes/plain.json", "plan", &json!({"mount": "/plain"}));
    assert_eq!(
        board.checked("/changes/"),
        codes(&[
            ("/changes/named", Some("board::ref::malformed")),
            ("/changes/old", Some("board::record::invalid")),
            ("/changes/plain", Some("board::ref::wrong_type")),
            ("/changes/mounted", None),
        ])
    );
}

// test_tickets.py: a change's tickets and pulls are typed references, which
// `board check` reports when broken and `board refs` finds.

/// The board with the repository a change names mounted, so only its
/// tickets and pulls can break it.
fn mounted() -> Changes {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board
}

#[test]
fn check_reports_a_ticket_that_reaches_nothing_and_admits_one_that_does() {
    let board = mounted();
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"tickets": ["/tickets/ACME-123"]}),
    );
    assert_eq!(
        board.code("/changes/t").as_deref(),
        Some("board::ref::dangling")
    );
    board.write("tickets/ACME-123.json", &typed("jira/issue/v1", &json!({})));
    assert_eq!(board.code("/changes/t"), None);
}

#[test]
fn check_reports_a_ticket_of_another_type_and_admits_each_issue_type() {
    let board = mounted();
    board.write("tickets/ACME-123.json", &typed("jira/issue/v1", &json!({})));
    board.write(
        "issues/octo-widgets-6.json",
        &typed("github/issue/v1", &json!({})),
    );
    board.write(
        "pulls/octo-widgets-7.json",
        &typed("github/pull/v1", &json!({})),
    );
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"tickets": ["/pulls/octo-widgets-7"]}),
    );
    assert_eq!(
        board.code("/changes/t").as_deref(),
        Some("board::ref::wrong_type")
    );
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"tickets": ["/tickets/ACME-123", "/issues/octo-widgets-6"]}),
    );
    assert_eq!(board.code("/changes/t"), None);
}

#[test]
fn check_reports_a_pull_of_another_type_and_admits_a_pull() {
    let board = mounted();
    board.write(
        "issues/octo-widgets-6.json",
        &typed("github/issue/v1", &json!({})),
    );
    board.write(
        "pulls/octo-widgets-7.json",
        &typed("github/pull/v1", &json!({})),
    );
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"pulls": ["/issues/octo-widgets-6"]}),
    );
    assert_eq!(
        board.code("/changes/t").as_deref(),
        Some("board::ref::wrong_type")
    );
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"pulls": ["/pulls/octo-widgets-7"]}),
    );
    assert_eq!(board.code("/changes/t"), None);
}

#[test]
fn refs_finds_the_changes_that_reference_a_ticket_and_not_one_that_does_not() {
    let board = mounted();
    board.write("tickets/ACME-123.json", &typed("jira/issue/v1", &json!({})));
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"tickets": ["/tickets/ACME-123"]}),
    );
    board.change_at("changes/u.json", "plan", &json!({}));
    let report = board.json(&["refs", "/tickets/ACME-123"]);
    let [answer] = report["data"].as_array().unwrap().as_slice() else {
        panic!("{report}");
    };
    let found: Vec<(&Value, &Value)> = (answer["to"].as_array().unwrap().iter())
        .map(|found| (&found["logical"], &found["pointers"]))
        .collect();
    assert_eq!(found, [(&json!("/changes/t"), &json!(["/tickets/0"]))]);
}

/// What `next` adds where its instruction asks for a plan for the change
/// at `path` on `mount`, given without its trailing slash, that has none.
fn new_plan_usage(path: &str, mount: &str) -> String {
    format!(
        "Pipe `board changes plan` JSON on stdin with the fields `path`, `text`, `uses` and `paths`, \
         such as `{}`.",
        plan_example(path, mount)
    )
}

/// What `next` adds where its instruction asks for a plan for the change
/// at `path` on `mount` that already has one.
fn revised_plan_usage(path: &str, mount: &str) -> String {
    format!(
        "`board changes plan` reads JSON on stdin and replaces the plan, so resend all four fields, \
         such as `{}`.",
        plan_example(path, mount)
    )
}

/// The one-line input of `changes.plan` the hints show.
fn plan_example(path: &str, mount: &str) -> String {
    format!(r#"{{"path":"{path}","text":"<text>","uses":["<use>"],"paths":["{mount}/<path>"]}}"#)
}
