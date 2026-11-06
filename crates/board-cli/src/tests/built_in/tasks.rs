//! The tasks plugin through the binary, against real git repositories: a
//! board enabling tasks, notes repos it mounts with bare remotes beside
//! them, and phones that clone those remotes. These were the plugin's
//! pytest suite (`test_available.py`, `test_blockers.py`, `test_trees.py`,
//! `test_write.py`, `test_schema.py` and the listing test of
//! `test_manifest.py`). The rules of the sweep, the tree, the line editor
//! and repeats are the crate's own tests, and so are the counts of Board's
//! work and the retries of a write against a real mount.

use std::cell::RefCell;
use std::path::PathBuf;

use super::*;

mod available;
mod owner;
mod schema;
mod trees;
mod write;

/// The schema the tasks plugin ships, which a notes repo holds so its
/// tasks read as valid there.
const SCHEMA: &str = include_str!("../../../../board-plugin-tasks/types/tasks/task/v2.schema.json");
const NOTES_SCHEMA: &str =
    include_str!("../../../../board-plugin-notes/types/notes/repo/v1.schema.json");

/// Runs git in `directory` and returns what it printed, trimmed.
#[expect(
    clippy::disallowed_methods,
    reason = "the fixture's other writers, the phones and the remotes, commit and push with git"
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

/// Writes `text` at `file` in a clone no scenario holds, such as a
/// phone's, as its app does.
#[expect(
    clippy::disallowed_methods,
    reason = "the phone's clone is git's, which the fixture writes as the phone's app would"
)]
fn write_file(file: &Path, text: &str) {
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

/// A `tasks/task/v2` Markdown task, each value written as JSON, which is
/// YAML, then `body`.
fn task(title: &str, status: &str, fields: &Value, body: &str) -> String {
    let mut lines = vec![
        "$type: \"tasks/task/v2\"".to_owned(),
        format!("title: {}", json!(title)),
        format!("status: {}", json!(status)),
    ];
    for (key, value) in fields.as_object().unwrap() {
        let shown = match value {
            Value::Array(items) => {
                let items: Vec<String> = items.iter().map(Value::to_string).collect();
                format!("[{}]", items.join(", "))
            }
            other => other.to_string(),
        };
        lines.push(format!("{key}: {shown}"));
    }
    format!("---\n{}\n---\n{body}", lines.join("\n"))
}

/// An open task with `fields`.
#[expect(
    clippy::needless_pass_by_value,
    reason = "a caller builds the fields with json! in place"
)]
fn open(title: &str, fields: Value) -> String {
    task(title, "open", &fields, "")
}

/// The date `days` from today in UTC, the zone each call runs in unless a
/// test names another, as YYYY-MM-DD.
fn day(days: i64) -> String {
    let today = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();
    today
        .checked_add(jiff::Span::new().days(days))
        .unwrap()
        .to_string()
}

/// The seconds east of UTC of a zone whose local time is now between
/// 01:00 and 01:59, and its local date.
fn one_in_the_morning() -> (i32, String) {
    let now = jiff::Timestamp::now();
    let hour = i32::from(now.to_zoned(jiff::tz::TimeZone::UTC).hour());
    let mut offset = (1 - hour).rem_euclid(24);
    if offset > 14 {
        offset -= 24;
    }
    let zone = jiff::tz::Offset::from_seconds(offset * 3600).unwrap();
    (
        offset * 3600,
        now.to_zoned(zone.to_time_zone()).date().to_string(),
    )
}

/// A board that enables tasks and leaves its puts uncommitted, the notes
/// repos it mounts, and their bare remotes and phones beside it.
struct Tasks {
    board: Scenario,
    /// The bare remotes, the phones and the callers' checkouts.
    beside: tempfile::TempDir,
    /// A directory in no git repository, where a call runs by default.
    outside: tempfile::TempDir,
    /// `$XDG_CONFIG_HOME`, holding `board/config.json`.
    config: Scenario,
    /// `$XDG_CACHE_HOME`, holding the index.
    cache: tempfile::TempDir,
    /// The clones of the notes repos, by name.
    clones: RefCell<Vec<(String, Scenario)>>,
}

impl Tasks {
    fn new() -> Self {
        let tasks = Self {
            board: Scenario::new(),
            beside: tempfile::tempdir().unwrap(),
            outside: tempfile::tempdir().unwrap(),
            config: Scenario::new(),
            cache: tempfile::tempdir().unwrap(),
            clones: RefCell::default(),
        };
        tasks.config.write("board/config.json", Item::File("{}"));
        tasks.configure(false);
        let enabled = tasks.run(&["plugin", "enable", "tasks"], 0, None, "");
        assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
        tasks
    }

    /// Writes `.board/board.json`, keeping the plugins it enables and
    /// committing puts when `commit` says so, and gives the board's
    /// repository an identity to commit with.
    fn configure(&self, commit: bool) {
        let path = self.board.workdir().join(".board/board.json");
        let mut config: Value = std::fs::read_to_string(path).map_or_else(
            |_| json!({"$type": "board/config/v1", "plugins": []}),
            |text| serde_json::from_str(&text).unwrap(),
        );
        config["commit_on_put"] = json!(commit);
        self.write(".board/board.json", &config.to_string());
        git(self.board.workdir(), &["config", "user.name", "t"]);
        git(self.board.workdir(), &["config", "user.email", "t@t"]);
    }

    /// Writes a file of the board's own tree, uncommitted.
    fn write(&self, path: &str, text: &str) {
        self.board.write(path, Item::Fresh(text));
    }

    /// The file at `path` in the board's own tree.
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.board.workdir().join(path)).unwrap()
    }

    /// A notes repo pushed to a bare remote beside the board, holding
    /// `tasks` by slug and `record` as its `notes-repo.json`, mounted at
    /// `/repos/<name>`: its clone. Without `daily` it has no daily/, so it
    /// is a mount that is not a notes repo.
    fn notes_repo(
        &self,
        name: &str,
        record: Option<Value>,
        tasks: &[(&str, &str)],
        daily: bool,
    ) -> PathBuf {
        let remote = self.beside.path().join(format!("{name}.git"));
        git(
            self.beside.path(),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ],
        );
        let clone = Scenario::new();
        let root = clone.workdir().to_path_buf();
        git(&root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(&root, &["config", "user.name", "t"]);
        git(&root, &["config", "user.email", "t@t"]);
        clone
            .write(
                ".board/board.json",
                Item::Fresh(r#"{"$type": "board/config/v1"}"#),
            )
            .write(
                ".board/types/tasks/task/v2.schema.json",
                Item::Fresh(SCHEMA),
            )
            .write(
                ".board/types/notes/repo/v1.schema.json",
                Item::Fresh(NOTES_SCHEMA),
            )
            .write("repo.txt", Item::Fresh(name));
        if daily {
            clone.write("daily/.gitkeep", Item::Fresh(""));
        }
        if let Some(mut record) = record {
            record["$type"] = json!("notes/repo/v1");
            clone.write("notes-repo.json", Item::Fresh(&record.to_string()));
        }
        for (slug, text) in tasks {
            clone.write(&format!("tasks/{slug}.md"), Item::Fresh(text));
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "fixture"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&root, &["push", "-qu", "origin", "main"]);
        self.mount(name, &root);
        self.clones.borrow_mut().push((name.to_owned(), clone));
        root
    }

    /// Mounts the repository at `root` at `/repos/<name>`, pinned to its
    /// HEAD, in the board's working tree.
    fn mount(&self, name: &str, root: &Path) {
        let head = git(root, &["rev-parse", "HEAD"]);
        let pointer = json!({
            "$type": "board/mount/v1", "repo": root, "follow": "main", "commit": head,
        });
        self.write(&format!("repos/{name}.json"), &pointer.to_string());
    }

    /// The board committing puts, with a personal notes repo and a work one
    /// covering example-org/*, which holds one task: the work clone.
    fn mounted(&self) -> PathBuf {
        self.configure(true);
        self.notes_repo(
            "personal",
            Some(json!({"covers": [], "personal": true})),
            &[],
            true,
        );
        let report = open("Report", json!({"reviewed": day(-30)}));
        self.notes_repo(
            "work",
            Some(json!({"covers": ["example-org/*"]})),
            &[("report", &report)],
            true,
        )
    }

    /// A second clone of the notes repo `name`'s remote, as a phone holds
    /// it.
    fn phone(&self, name: &str) -> PathBuf {
        let clone = self.beside.path().join(format!("{name}-phone"));
        let remote = self.beside.path().join(format!("{name}.git"));
        git(
            self.beside.path(),
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        clone
    }

    /// A caller's checkout, beside the board, with no origin.
    fn checkout(&self) -> PathBuf {
        let root = self.beside.path().join("checkout");
        git(self.beside.path(), &["init", "-q", root.to_str().unwrap()]);
        root
    }

    /// Sets the known boards file's `day_start`, or leaves it out.
    fn day_start(&self, start: Option<&str>) {
        let config = start.map_or_else(|| json!({}), |start| json!({"day_start": start}));
        (self.config).write("board/config.json", Item::Fresh(&config.to_string()));
    }

    /// Runs `board` in the time zone `offset` seconds east of UTC. From
    /// `caller` it runs `board -C <board>`; otherwise in the board.
    fn run(&self, arguments: &[&str], offset: i32, caller: Option<&Path>, stdin: &str) -> Output {
        let board = self.board.workdir().to_path_buf();
        let start = caller.unwrap_or(&board).to_path_buf();
        let root = board.to_str().unwrap().to_owned();
        let mut every: Vec<&str> = vec![BINARY_NAME];
        if caller.is_some() {
            every.extend(["-C", root.as_str()]);
        }
        every.extend(arguments);
        let context = Context {
            start: start.clone(),
            home: None,
            cache: Some(self.cache.path().to_path_buf()),
            plugins: None,
            boards: Some(self.config.workdir().join("board/config.json")),
            signer: None,
            allowed_signers: None,
            attention_allowed_signers: None,
            environment: board_plugin::Environment {
                path: Some(PATH.into()),
                caller: Some(start.into_os_string()),
                utc_offset: offset,
                ..board_plugin::Environment::default()
            },
            complete: None,
            terminal: false,
            browser: None,
            binary: None,
            cursor: None,
            built_in: board_plugins::BUILT_IN,
        };
        let mut ran = Ran::default();
        let output = run(
            every.into_iter().map(OsString::from),
            &context,
            &mut stdin.as_bytes(),
            &mut ran,
        );
        assert_eq!(ran.trace.work.get("started").copied().unwrap_or(0), 0);
        output
    }

    /// `board --json call tasks.<capability> <flags>` from `caller`, in the
    /// zone `offset` seconds east of UTC, with `stdin`: its output, or its
    /// failure.
    fn call_with(
        &self,
        capability: &str,
        flags: &[&str],
        offset: i32,
        caller: &Path,
        stdin: &str,
    ) -> Result<Value, Value> {
        let target = format!("tasks.{capability}");
        let mut arguments = vec!["--json", "call", target.as_str()];
        arguments.extend(flags);
        let output = self.run(&arguments, offset, Some(caller), stdin);
        let document = json(&output);
        if output.code == 0 {
            Ok(document["data"]["output"].clone())
        } else {
            assert_eq!(output.code, 1, "{document}");
            Err(document)
        }
    }

    /// A call in UTC from outside any git repository.
    fn call(&self, capability: &str, flags: &[&str]) -> Result<Value, Value> {
        self.call_with(capability, flags, 0, self.outside.path(), "")
    }

    /// The refusal of a call, which carries a code.
    fn refused(&self, capability: &str, flags: &[&str]) -> Value {
        let refused = self.call(capability, flags).unwrap_err();
        assert!(refused["code"].is_string(), "{refused}");
        refused
    }
}

/// The titles in one section of `available`'s output.
fn titles(output: &Value, section: &str) -> Vec<String> {
    (output[section]["tasks"].as_array().unwrap().iter())
        .map(|item| item["title"].as_str().unwrap().to_owned())
        .collect()
}

/// The titles of `available`'s `available` section.
fn shown(output: &Value) -> Vec<String> {
    titles(output, "available")
}

#[test]
fn board_lists_tasks_as_built_in_enabled_and_working_and_enabling_wrote_the_schema_it_ships() {
    let tasks = Tasks::new();
    let report = json(&tasks.run(&["--json", "plugin", "list"], 0, None, ""));
    let rows = report["data"].as_array().unwrap();
    let listed = rows.iter().find(|row| row["name"] == "tasks").unwrap();
    let offers = json!([
        "available",
        "tree",
        "add",
        "claim",
        "done",
        "defer",
        "drop",
        "reviewed",
        "move"
    ]);
    assert_eq!(
        (&listed["status"], &listed["offers"], listed.get("run")),
        (&json!("ok"), &offers, None)
    );
    assert_eq!(listed["enabled"], true);
    let shipped = tasks.read(".board/types/tasks/task/v2.schema.json");
    assert_eq!(shipped, SCHEMA, "the Python plugin's bytes");
    assert!(
        !tasks
            .board
            .workdir()
            .join(".board/types/tasks/task/v1.schema.json")
            .exists()
    );
}
