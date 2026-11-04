//! The notes plugin through the binary, against real git repositories: a
//! board that mounts a notes repo at `/repos/notes-personal`, the repo's
//! bare remote, and a second clone standing in for the phone. These were
//! the plugin's pytest suite (`test_add.py`, `test_daily.py`,
//! `test_repos.py` and the listing test of `test_manifest.py`). The clone
//! is the one Reflect's desktop app would hold.

use std::path::PathBuf;

use super::*;

mod add;
mod daily;
mod repos;

/// The notes plugin's `notes/repo/v1` schema, which a notes repo holds so
/// its `notes-repo.json` reads as valid.
const SCHEMA: &str = include_str!("../../../../board-plugin-notes/types/notes/repo/v1.schema.json");

/// Runs git in `directory` and returns what it printed, trimmed.
#[expect(
    clippy::disallowed_methods,
    reason = "the fixture's other writers, the phone and Reflect's desktop app, commit and push with git"
)]
fn git(directory: &Path, arguments: &[&str]) -> String {
    let ran = std::process::Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
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

/// Gives the repository at `directory` an identity of its own.
fn identify(directory: &Path) {
    git(directory, &["config", "user.name", "t"]);
    git(directory, &["config", "user.email", "t@t"]);
}

/// Commits everything in `directory` and returns the commit.
fn commit(directory: &Path) -> String {
    git(directory, &["add", "."]);
    git(directory, &["commit", "-qm", "fixture"]);
    git(directory, &["rev-parse", "HEAD"])
}

/// Reflect's layout in `scenario`: daily/, notes/ and assets/, each kept
/// by a `.gitkeep`.
fn layout(scenario: &Scenario) {
    for directory in ["daily", "notes", "assets"] {
        scenario.write(&format!("{directory}/.gitkeep"), Item::File(""));
    }
}

/// A board, a notes repo mounted in it, the repo's bare remote, and a
/// phone that clones it.
struct Notes {
    board: Scenario,
    clone: Scenario,
    phone: Scenario,
    /// The bare remotes and the callers' checkouts.
    beside: tempfile::TempDir,
    /// `$XDG_CONFIG_HOME`, holding `board/config.json`.
    config: Scenario,
    runner: Runner,
}

/// What runs `board` on the board, which threads can share.
#[derive(Clone)]
struct Runner {
    board: PathBuf,
    /// `$XDG_CONFIG_HOME/board/config.json`.
    known: PathBuf,
}

impl std::ops::Deref for Notes {
    type Target = Runner;

    fn deref(&self) -> &Runner {
        &self.runner
    }
}

/// The time zone a call runs in, as seconds east of UTC.
#[derive(Clone, Copy)]
struct Zone(i32);

const UTC: Zone = Zone(0);

impl Notes {
    fn new() -> Self {
        let (board, config) = (Scenario::new(), Scenario::new());
        let runner = Runner {
            board: board.workdir().to_path_buf(),
            known: config.workdir().join("board/config.json"),
        };
        let notes = Self {
            board,
            clone: Scenario::new(),
            phone: Scenario::new(),
            beside: tempfile::tempdir().unwrap(),
            config,
            runner,
        };
        let remote = notes.notes_repo(&notes.clone, "remote");
        git(
            notes.phone.workdir(),
            &["symbolic-ref", "HEAD", "refs/heads/main"],
        );
        git(notes.phone.workdir(), &["remote", "add", "origin", &remote]);
        git(notes.phone.workdir(), &["pull", "-q", "origin", "main"]);
        git(
            notes.phone.workdir(),
            &["branch", "-q", "-u", "origin/main"],
        );
        identify(notes.phone.workdir());
        let board = notes.board.workdir();
        git(board, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        identify(board);
        (notes.board).write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        );
        commit(board);
        let enabled = notes.run(&["plugin", "enable", "notes"], UTC, None);
        assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
        if !git(board, &["status", "--porcelain"]).is_empty() {
            commit(board);
        }
        notes.mount("notes-personal", &notes.clone);
        notes
    }

    /// Makes `scenario` a notes repo in Reflect's layout, a board itself,
    /// pushed to a new bare remote beside it named `name`, whose path it
    /// returns.
    fn notes_repo(&self, scenario: &Scenario, name: &str) -> String {
        let remote = self.beside.path().join(format!("{name}.git"));
        let remote = remote.to_str().unwrap().to_owned();
        git(
            self.beside.path(),
            &["init", "-q", "--bare", "-b", "main", &remote],
        );
        let root = scenario.workdir();
        git(root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        identify(root);
        scenario
            .write(
                ".board/board.json",
                Item::File(r#"{"$type": "board/config/v1"}"#),
            )
            .write(".gitignore", Item::File(".reflect/\n"))
            // Repos with the same tree, author and time would share a
            // commit, which the index lists under one mount.
            .write("repo.txt", Item::File(&format!("{name}\n")));
        layout(scenario);
        commit(root);
        git(root, &["remote", "add", "origin", &remote]);
        git(root, &["push", "-qu", "origin", "main"]);
        remote
    }

    /// Mounts the repository at `scenario` at `/repos/<name>`, pinned to
    /// its HEAD, and commits the mount.
    fn mount(&self, name: &str, scenario: &Scenario) {
        let head = git(scenario.workdir(), &["rev-parse", "HEAD"]);
        let mount = json!({"$type": "board/mount/v1", "repo": scenario.path(), "follow": "main", "commit": head});
        (self.board).write(
            &format!("repos/{name}.json"),
            Item::Fresh(&mount.to_string()),
        );
        commit(self.board.workdir());
    }

    /// Another notes repo with a remote of its own, mounted at
    /// `/repos/<name>`: its clone.
    fn another(&self, name: &str) -> Scenario {
        let scenario = Scenario::new();
        self.notes_repo(&scenario, name);
        self.mount(name, &scenario);
        scenario
    }

    /// Gives the notes repo `scenario` its `notes/repo/v1` record and that
    /// type's schema, commits and pushes them, and pins its mount at
    /// `/repos/<name>` to them, as a person would after writing the record.
    fn cover(&self, scenario: &Scenario, name: &str, covers: &[&str], personal: bool) {
        let mut record = json!({"$type": "notes/repo/v1", "covers": covers});
        if personal {
            record["personal"] = json!(true);
        }
        scenario
            .write("notes-repo.json", Item::File(&record.to_string()))
            .write(".board/types/notes/repo/v1.schema.json", Item::File(SCHEMA));
        commit(scenario.workdir());
        git(scenario.workdir(), &["push", "-q"]);
        let pinned = self.run(&["mount", "update", &format!("/repos/{name}")], UTC, None);
        assert_eq!(pinned.code, 0, "{}", text(&pinned.stderr));
    }

    /// Pulls on the phone, appends `text` to the file at `path`, commits
    /// and pushes, as Reflect's phone app does.
    fn phone_appends(&self, path: &str, text: &str) {
        let phone = self.phone.workdir();
        git(phone, &["pull", "-q"]);
        self.phone.write(path, Item::Appended(text));
        commit(phone);
        git(phone, &["push", "-q"]);
    }

    /// The path in the notes repo of the note a call's `output` names.
    fn note(output: &Value) -> String {
        let path = output["path"].as_str().unwrap();
        path.strip_prefix("/repos/notes-personal/")
            .unwrap_or(path)
            .to_owned()
    }

    /// The file at `path` in the clone's working tree.
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.clone.workdir().join(path)).unwrap()
    }

    /// The file at `path` as the notes repo's remote holds it.
    fn remote_text(&self, path: &str) -> String {
        let remote = self.beside.path().join("remote.git");
        git(&remote, &["show", &format!("main:{path}")])
    }

    /// The commit the board's mount of the notes repo reads.
    fn pin(&self) -> String {
        let pointer =
            std::fs::read_to_string(self.board.workdir().join("repos/notes-personal.json"));
        let pointer: Value = serde_json::from_str(&pointer.unwrap()).unwrap();
        pointer["commit"].as_str().unwrap().to_owned()
    }

    fn head(&self) -> String {
        git(self.clone.workdir(), &["rev-parse", "HEAD"])
    }

    /// A caller's checkout, outside the board, whose origin is `origin`.
    fn checkout(&self, origin: &str) -> PathBuf {
        let index = std::fs::read_dir(self.beside.path()).unwrap().count();
        let root = self.beside.path().join(format!("checkout-{index}"));
        git(self.beside.path(), &["init", "-q", root.to_str().unwrap()]);
        git(&root, &["remote", "add", "origin", origin]);
        root
    }

    /// Sets the known boards file's `day_start`, or leaves it out.
    fn day_start(&self, start: Option<&str>) {
        let config = start.map_or_else(|| json!({}), |start| json!({"day_start": start}));
        (self.config).write("board/config.json", Item::File(&config.to_string()));
    }
}

impl Runner {
    /// Runs `board` with the known boards file in `config`, in the time
    /// zone `zone`. From `caller`, a caller's checkout, it runs `board -C
    /// <board>`; otherwise in the board.
    fn run(&self, arguments: &[&str], zone: Zone, caller: Option<&Path>) -> Output {
        let start = caller.unwrap_or(&self.board).to_path_buf();
        let root = self.board.to_str().unwrap().to_owned();
        let mut every: Vec<&str> = vec![BINARY_NAME];
        if caller.is_some() {
            every.extend(["-C", root.as_str()]);
        }
        every.extend(arguments);
        let context = Context {
            start: start.clone(),
            home: None,
            cache: None,
            plugins: None,
            boards: Some(self.known.clone()),
            signer: None,
            allowed_signers: None,
            environment: board_plugin::Environment {
                path: Some(PATH.into()),
                caller: Some(start.into_os_string()),
                utc_offset: zone.0,
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
            &mut &b""[..],
            &mut ran,
        );
        assert_eq!(ran.trace.work.get("started").copied().unwrap_or(0), 0);
        output
    }

    /// `board --json call notes.<capability> <flags>`: its output, or its
    /// failure.
    fn call_in(
        &self,
        capability: &str,
        flags: &[&str],
        zone: Zone,
        caller: Option<&Path>,
    ) -> Result<Value, Value> {
        let target = format!("notes.{capability}");
        let mut arguments = vec!["--json", "call", target.as_str()];
        arguments.extend(flags);
        let output = self.run(&arguments, zone, caller);
        let document = json(&output);
        if output.code == 0 {
            Ok(document["data"]["output"].clone())
        } else {
            Err(document)
        }
    }

    /// [`Notes::call_in`] in UTC, in the board.
    fn call(&self, capability: &str, flags: &[&str]) -> Result<Value, Value> {
        self.call_in(capability, flags, UTC, None)
    }

    /// The code of the refusal of a call.
    fn refused(&self, capability: &str, flags: &[&str]) -> Value {
        let refused = self.call(capability, flags).unwrap_err();
        assert!(refused["code"].is_string(), "{refused}");
        refused
    }
}

/// The person's day in `zone`, for a day starting `start` hours after
/// midnight, read now and an hour on. A call made between the two
/// readings is on one of the two days.
fn dates(zone: Zone, start: i64) -> Vec<String> {
    let offset = jiff::tz::Offset::from_seconds(zone.0).unwrap();
    let now = jiff::Timestamp::now();
    [now, now + jiff::SignedDuration::from_hours(1)]
        .into_iter()
        .map(|moment| {
            let local =
                (moment - jiff::SignedDuration::from_hours(start)).to_zoned(offset.to_time_zone());
            local.date().to_string()
        })
        .collect()
}

/// Kiritimati is 14 hours ahead of UTC and Etc/GMT+12 is 12 behind, so
/// their dates always differ.
const ZONES: [Zone; 2] = [Zone(14 * 3600), Zone(-12 * 3600)];

/// Whether `text` is one `## HH:MM` section holding `body`.
fn section(text: &str, body: &str) -> bool {
    let Some(rest) = text.strip_prefix("## ") else {
        return false;
    };
    let (clock, rest) = rest.split_at(5.min(rest.len()));
    clock.len() == 5
        && clock.as_bytes()[2] == b':'
        && clock
            .bytes()
            .enumerate()
            .all(|(at, byte)| at == 2 || byte.is_ascii_digit())
        && rest == format!("\n\n{body}")
}

/// The sibling `.agent.md` file beside the note at `path`.
fn sibling(path: &str) -> String {
    format!("{}.agent.md", path.trim_end_matches(".md"))
}

#[test]
fn board_lists_notes_as_built_in_enabled_and_working_and_a_shipped_schema_needs_no_enable() {
    let notes = Notes::new();
    let report = json(&notes.run(&["--json", "plugin", "list"], UTC, None));
    let rows = report["data"].as_array().unwrap();
    let listed = rows.iter().find(|row| row["name"] == "notes").unwrap();
    assert_eq!(
        (&listed["status"], &listed["offers"], listed.get("run")),
        (&json!("ok"), &json!(["add", "daily"]), None)
    );
    assert_eq!(listed["enabled"], true);
    let shipped = std::fs::read_to_string(
        (notes.board.workdir()).join(".board/types/notes/repo/v1.schema.json"),
    );
    assert_eq!(shipped.unwrap(), SCHEMA, "the Python plugin's bytes");
}
