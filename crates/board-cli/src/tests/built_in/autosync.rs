//! The autosync plugin through the binary, in a personal board with the
//! test signer's key, over boards cloned from local remotes, with the real
//! git and gitleaks and a stub `osascript` first on the plugin's PATH. Each
//! secret is built at run time and never written to a test file. What the
//! guarded sync refuses is tested in board-tree's
//! `src/write/sync/guard/tests.rs`; the record's signature, the lock and
//! the agent in the crate's own tests.

use std::path::PathBuf;

use super::*;

/// The personal board, its known boards file, the test signer, the home
/// autosync keeps its state in, and a directory first on the plugin's PATH
/// holding gitleaks and a stub osascript that logs each notification.
struct Mac {
    personal: Personal,
    home: Scenario,
    bin: Scenario,
}

/// One board and the remote it follows.
struct Shared {
    remote: Scenario,
    clone: Scenario,
}

/// The gitleaks on the PATH the tests run with.
#[expect(
    clippy::disallowed_methods,
    reason = "the test finds the gitleaks it runs, as a person's shell would"
)]
fn gitleaks() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    (std::env::split_paths(&path))
        .map(|directory| directory.join("gitleaks"))
        .find(|program| program.is_file())
        .expect("these tests run the real gitleaks; install 8.19.0 or later on the PATH")
}

/// Runs git in `directory`, as a person resolving a merge by hand would.
#[expect(
    clippy::disallowed_methods,
    reason = "the owner fetches and merges with git when autosync refuses a conflict"
)]
fn git(directory: &std::path::Path, arguments: &[&str]) {
    let ran = std::process::Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "git {arguments:?}: {}",
        text(&ran.stderr)
    );
}

/// A GitHub token that gitleaks flags, made for this run, so no secret is
/// ever written in a test file.
fn secret() -> String {
    use std::hash::{BuildHasher, Hasher};
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut token = "ghp_".to_owned();
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    for round in 0..36_u64 {
        hasher.write_u64(round);
        let index = usize::try_from(hasher.finish() % 62).unwrap();
        token.push(char::from(alphabet[index]));
    }
    token
}

impl Mac {
    fn new() -> Self {
        let personal = Personal::enabling(&["autosync", "tasks"]);
        let bin = Scenario::new();
        let gitleaks = gitleaks().display().to_string();
        bin.write("gitleaks", Item::Symlink(&gitleaks));
        let mac = Self {
            personal,
            home: Scenario::new(),
            bin,
        };
        mac.stub("osascript");
        mac
    }

    /// Puts a stub `name` on the plugin's PATH that logs its last argument.
    fn stub(&self, name: &str) {
        let log = self.bin.workdir().join(format!("{name}.log"));
        let script = format!(
            "#!/bin/sh\nfor last; do :; done\nprintf '%s\\n' \"$last\" >> '{}'\n",
            log.display()
        );
        self.bin.write(name, Item::Executable(&script));
    }

    /// Each line a stub logged.
    fn logged(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.bin.workdir().join(format!("{name}.log")))
            .map_or_default(|log| log.lines().map(str::to_owned).collect())
    }

    fn state(&self) -> PathBuf {
        self.home.workdir().join("Library/Logs/board-autosync")
    }

    /// Runs `board --json <arguments>` in the personal board, with the
    /// plugin's PATH `path`, the signer when `signing`, and `inherited`
    /// among Board's variables.
    fn run_with(
        &self,
        arguments: &[&str],
        path: &str,
        signing: bool,
        inherited: &[(&str, &str)],
    ) -> (u8, Value) {
        let signer = &self.personal.signer;
        let context = Context {
            start: self.personal.board.workdir().to_path_buf(),
            home: None,
            cache: Some(self.personal.cache.path().to_path_buf()),
            plugins: None,
            boards: Some(self.personal.xdg.workdir().join("board/config.json")),
            signer: signing.then(|| signer.program().into_os_string()),
            allowed_signers: Some(signer.allowed()),
            attention_allowed_signers: Some(signer.allowed()),
            environment: board_plugin::Environment {
                path: Some(path.into()),
                home: Some(self.home.workdir().as_os_str().to_owned()),
                inherited: (inherited.iter())
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
        let arguments = [BINARY_NAME, "--json"]
            .into_iter()
            .chain(arguments.iter().copied())
            .map(OsString::from);
        let output = run(arguments, &context, &mut &b""[..], &mut Ran::default());
        let shown = format!("{}{}", text(&output.stdout), text(&output.stderr));
        (
            output.code,
            serde_json::from_str(&text(&output.stdout)).unwrap_or(json!(shown)),
        )
    }

    fn path(&self) -> String {
        format!("{}:/usr/bin:/bin", self.bin.workdir().display())
    }

    fn run(&self, arguments: &[&str]) -> (u8, Value) {
        self.run_with(arguments, &self.path(), true, &[])
    }

    /// `autosync run`: each board's entry by name, and whether it exited 0.
    fn round(&self) -> (bool, BTreeMap<String, Value>) {
        let (code, report) = self.run(&["autosync", "run"]);
        let boards = if code == 0 {
            report["data"]["output"]["boards"].clone()
        } else {
            assert_eq!(report["code"], "autosync::run::failed", "{report}");
            report["fields"]["boards"].clone()
        };
        let boards = (boards.as_array().unwrap().iter())
            .map(|entry| (entry["name"].as_str().unwrap().to_owned(), entry.clone()))
            .collect();
        (code == 0, boards)
    }

    /// `autosync add` of `shared` as `name`, allowing `origins`.
    fn add(&self, name: &str, shared: &Shared, origins: &[&str]) {
        let path = shared.clone.workdir().display().to_string();
        let origins = origins.join(" ");
        let arguments = [
            "autosync", "add", "--name", name, "--path", &path, "--origin", &origins,
        ];
        let (code, report) = self.run(&arguments);
        assert_eq!(code, 0, "{report}");
    }

    /// Every file autosync left on this Mac and every task in the personal
    /// board, as text.
    fn everything(&self) -> String {
        let mut found = String::new();
        let mut pending = vec![
            self.home.workdir().to_path_buf(),
            self.personal.board.workdir().join("tasks"),
        ];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).into_iter().flatten() {
                let entry = entry.unwrap().path();
                if entry.is_dir() {
                    pending.push(entry);
                } else {
                    found += &std::fs::read_to_string(&entry).unwrap_or_default();
                }
            }
        }
        found + &self.logged("osascript").join("\n")
    }

    /// The open tasks' titles.
    fn tasks(&self) -> Vec<String> {
        let tasks = self.personal.board.workdir().join("tasks");
        let mut titles: Vec<String> = (std::fs::read_dir(tasks).into_iter().flatten())
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .filter(|text| text.contains("status: \"open\""))
            .filter_map(|text| {
                let line = text.lines().find(|line| line.starts_with("title: "))?;
                Some(
                    line.trim_start_matches("title: ")
                        .trim_matches('"')
                        .to_owned(),
                )
            })
            .collect();
        titles.sort();
        titles
    }
}

impl Shared {
    /// A remote that takes pushes, holding a board, and a clone of it.
    fn new() -> Self {
        let remote = Scenario::new();
        remote.identified().accepting_pushes();
        remote.commit(&[(".board/board.json", CONFIG)]);
        let clone = Scenario::clone_of(&remote);
        clone.identified();
        Self { remote, clone }
    }

    /// Commits `files` beside the board's config in the clone, written and
    /// staged as a put leaves them.
    fn commit(&self, files: &[(&str, &str)]) -> ObjectId {
        let mut tree = vec![(".board/board.json", CONFIG)];
        tree.extend_from_slice(files);
        let before: Vec<String> = self.clone.head().unwrap().files.into_keys().collect();
        let id = self.clone.commit(&tree);
        for path in before {
            if !tree.iter().any(|(kept, _)| *kept == path) {
                self.clone.write(&path, Item::Deleted);
            }
        }
        for (path, text) in &tree {
            self.clone.write(path, Item::File(text));
        }
        self.clone.track(id);
        id
    }

    fn origin(&self) -> String {
        self.remote.path()
    }

    fn remote_head(&self) -> ObjectId {
        self.remote.head().unwrap().id
    }
}

/// A Mac with one board, `team`, added with its own remote allowed.
fn team() -> (Mac, Shared) {
    let mac = Mac::new();
    let shared = Shared::new();
    mac.add("team", &shared, &[&shared.origin()]);
    (mac, shared)
}

#[test]
fn clean_commits_are_pushed_and_the_remote_branch_equals_the_local_one() {
    let (mac, shared) = team();
    let new = shared.commit(&[("notes/a.json", "{}")]);
    let (ok, boards) = mac.round();
    assert!(ok, "{boards:?}");
    assert_eq!(boards["team"]["outcome"], "synced");
    assert_eq!(boards["team"]["pushed"], true);
    assert_eq!(shared.remote_head(), new);
    let (code, status) = mac.run(&["autosync", "status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["data"]["output"]["boards"][0]["outcome"], "synced");
    assert_eq!(mac.logged("osascript"), Vec::<String>::new());
}

#[test]
fn a_commit_holding_a_secret_is_not_pushed_and_its_text_reaches_no_report() {
    let (mac, shared) = team();
    let before = shared.remote_head();
    let token = secret();
    let config = format!("{{\"token\": \"{token}\"}}");
    shared.commit(&[("notes/config.json", &config)]);
    let (ok, boards) = mac.round();
    assert!(!ok);
    let team = &boards["team"];
    assert_eq!(team["outcome"], "findings", "{team}");
    assert_eq!(team["code"], "autosync::scan::found");
    assert_eq!(team["findings"][0]["file"], "notes/config.json");
    assert_eq!(shared.remote_head(), before);
    assert_eq!(mac.tasks(), ["Board sync stopped for the team board"]);
    assert_eq!(mac.logged("osascript").len(), 1);
    let everything = mac.everything() + &boards.values().map(Value::to_string).collect::<String>();
    assert!(!everything.contains(&token), "the secret reached a report");
    let left: Vec<_> = std::fs::read_dir(mac.state())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("scan-"))
        .collect();
    assert_eq!(left, Vec::<String>::new());
    // A second failing run writes no second task or notification.
    mac.round();
    assert_eq!(mac.tasks().len(), 1);
    assert_eq!(mac.logged("osascript").len(), 1);
}

#[test]
fn a_secret_added_in_one_commit_and_removed_in_the_next_still_blocks_the_push() {
    let (mac, shared) = team();
    let before = shared.remote_head();
    let config = format!("{{\"token\": \"{}\"}}", secret());
    shared.commit(&[("notes/config.json", &config)]);
    shared.commit(&[("notes/config.json", "{}")]);
    let (_, boards) = mac.round();
    assert_eq!(boards["team"]["outcome"], "findings");
    assert_eq!(shared.remote_head(), before);
}

#[test]
fn a_lasting_failure_keeps_one_task_and_the_next_clean_run_closes_it() {
    let (mac, shared) = team();
    let clean = shared.remote_head();
    let config = format!("{{\"token\": \"{}\"}}", secret());
    shared.commit(&[("notes/config.json", &config)]);
    mac.round();
    assert_eq!(mac.tasks().len(), 1);
    // The owner drops the commit and commits again without the secret.
    shared
        .clone
        .write(".git/refs/heads/main", Item::File(&format!("{clean}\n")));
    shared.clone.track(clean);
    shared.clone.write("notes/config.json", Item::Deleted);
    let new = shared.commit(&[("notes/config.json", "{}")]);
    let (ok, boards) = mac.round();
    assert!(ok, "{boards:?}");
    assert_eq!(shared.remote_head(), new);
    assert_eq!(mac.tasks(), Vec::<String>::new());
    assert_eq!(
        mac.logged("osascript").len(),
        2,
        "the change back is told once"
    );
}

#[test]
fn a_fingerprint_on_this_mac_lets_its_finding_push_and_gitleaks_config_from_the_caller_does_not() {
    let (mac, shared) = team();
    let config = format!("{{\"token\": \"{}\"}}", secret());
    let commit = shared.commit(&[("notes/config.json", &config)]);
    let allow_all = mac.home.workdir().join("allow.toml");
    mac.home.write(
        "allow.toml",
        Item::File("[allowlist]\npaths = ['''.*''']\n"),
    );
    let allow = allow_all.display().to_string();
    let inherited = [("GITLEAKS_CONFIG", allow.as_str())];
    let (code, _) = mac.run_with(&["autosync", "run"], &mac.path(), true, &inherited);
    assert_ne!(code, 0, "GITLEAKS_CONFIG reached gitleaks");
    // The fingerprint status prints is the line gitleaks ignores, with the
    // whole commit id.
    let (_, status) = mac.run(&["autosync", "status"]);
    let finding = &status["data"]["output"]["boards"][0]["findings"][0];
    let fingerprint = finding["fingerprint"].as_str().unwrap();
    assert_eq!(
        fingerprint,
        format!("{commit}:notes/config.json:github-pat:1")
    );
    assert_eq!(finding["commit"], commit.to_string());
    mac.home.write(
        ".config/board/autosync/gitleaksignore",
        Item::File(&format!("{fingerprint}\n")),
    );
    let (ok, boards) = mac.round();
    assert!(ok, "{boards:?}");
    assert_eq!(shared.remote_head(), commit);
}

#[test]
fn a_gitleaks_file_in_the_board_or_its_working_tree_stops_the_push_naming_it() {
    for (name, committed) in [
        (".gitleaks.toml", true),
        (".gitleaksignore", true),
        (".gitleaks.toml", false),
        (".gitleaksignore", false),
    ] {
        let (mac, shared) = team();
        let before = shared.remote_head();
        if committed {
            shared.commit(&[(name, "[allowlist]\n"), ("notes/a.json", "{}")]);
        } else {
            shared.commit(&[("notes/a.json", "{}")]);
            shared.clone.write(name, Item::File("[allowlist]\n"));
        }
        let (_, boards) = mac.round();
        let team = &boards["team"];
        assert_eq!(
            team["code"], "autosync::scan::board_config",
            "{name}: {team}"
        );
        assert!(team["message"].as_str().unwrap().contains(name), "{team}");
        assert_eq!(shared.remote_head(), before, "{name}");
    }
}

#[test]
fn a_colleagues_gitleaks_file_merged_from_the_remote_stops_the_push() {
    let (mac, shared) = team();
    shared
        .remote
        .commit(&[(".board/board.json", CONFIG), (".gitleaksignore", "x\n")]);
    let theirs = shared.remote_head();
    shared.commit(&[("notes/a.json", "{}")]);
    let (_, boards) = mac.round();
    assert_eq!(boards["team"]["code"], "autosync::scan::board_config");
    assert_eq!(shared.remote_head(), theirs);
}

#[test]
fn a_push_url_outside_the_records_origins_is_refused_before_the_fetch_and_its_twin_syncs() {
    let mac = Mac::new();
    let shared = Shared::new();
    mac.add("team", &shared, &["git@example.com:someone/else.git"]);
    let new = shared.commit(&[("notes/a.json", "{}")]);
    let before = shared.remote_head();
    let (_, boards) = mac.round();
    assert_eq!(boards["team"]["code"], "autosync::origin::not_allowed");
    assert_eq!(shared.remote_head(), before);
    mac.add("team", &shared, &[&shared.origin()]);
    let (ok, _) = mac.round();
    assert!(ok);
    assert_eq!(shared.remote_head(), new);
}

#[test]
fn a_push_instead_of_that_rewrites_to_another_url_is_refused() {
    let (mac, shared) = team();
    let rewrite = format!(
        "[url \"/nowhere/else\"]\n\tpushInsteadOf = {}\n",
        shared.origin()
    );
    shared.clone.write(".git/config", Item::Appended(&rewrite));
    shared.commit(&[("notes/a.json", "{}")]);
    let (_, boards) = mac.round();
    assert_eq!(boards["team"]["code"], "autosync::origin::not_allowed");
}

#[test]
fn a_board_inside_another_checkout_is_refused_as_nested_and_its_twin_at_the_top_syncs() {
    let mac = Mac::new();
    let shared = Shared::new();
    shared.commit(&[("notes/a.json", "{}")]);
    let inside = shared.clone.workdir().join("notes").display().to_string();
    let origin = shared.origin();
    let arguments = [
        "autosync", "add", "--name", "team", "--path", &inside, "--origin", &origin,
    ];
    assert_eq!(mac.run(&arguments).0, 0);
    let (_, boards) = mac.round();
    assert_eq!(boards["team"]["code"], "autosync::board::nested");
    mac.add("team", &shared, &[&origin]);
    assert!(mac.round().0);
}

#[test]
fn an_unsigned_record_an_agent_puts_leaves_its_board_unsynced() {
    let mac = Mac::new();
    let shared = Shared::new();
    let record = json!({"$type": "autosync/board/v1", "path": shared.clone.path(), "origins": [shared.origin()]});
    let directory = mac.personal.board.workdir().to_path_buf();
    let put = (mac.personal).run_in(
        &directory,
        &["put", "/autosync/boards/team"],
        &record.to_string(),
    );
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
    let before = shared.remote_head();
    shared.commit(&[("notes/a.json", "{}")]);
    let (_, boards) = mac.round();
    assert_eq!(boards["team"]["code"], "autosync::board::unsigned");
    assert_eq!(shared.remote_head(), before);
}

#[test]
fn add_without_a_signer_is_refused_and_writes_nothing() {
    let mac = Mac::new();
    let shared = Shared::new();
    let path = shared.clone.path();
    let origin = shared.origin();
    let arguments = [
        "autosync", "add", "--name", "team", "--path", &path, "--origin", &origin,
    ];
    let (code, refused) = mac.run_with(&arguments, &mac.path(), false, &[]);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("autosync::sign::no_signer"))
    );
    assert_eq!(mac.personal.record("/autosync/boards/team"), None);
    let (code, _) = mac.run_with(&arguments, &mac.path(), true, &[]);
    assert_eq!(code, 0);
    assert!(mac.personal.record("/autosync/boards/team").is_some());
}

#[test]
fn a_failing_board_does_not_stop_the_next_and_the_run_exits_non_zero() {
    let mac = Mac::new();
    let bad = Shared::new();
    let good = Shared::new();
    mac.add("a-bad", &bad, &["git@example.com:o/r.git"]);
    mac.add("b-good", &good, &[&good.origin()]);
    let new = good.commit(&[("notes/a.json", "{}")]);
    let (ok, boards) = mac.round();
    assert!(!ok);
    assert_eq!(boards["a-bad"]["code"], "autosync::origin::not_allowed");
    assert_eq!(boards["b-good"]["outcome"], "synced");
    assert_eq!(good.remote_head(), new);
}

#[test]
fn without_gitleaks_on_the_path_a_board_merges_and_pushes_nothing() {
    let (mac, shared) = team();
    let theirs = shared
        .remote
        .commit(&[(".board/board.json", CONFIG), ("theirs.json", "{}")]);
    shared.commit(&[("notes/a.json", "{}")]);
    let (code, report) = mac.run_with(&["autosync", "run"], "/usr/bin:/bin", true, &[]);
    assert_eq!(code, 1, "{report}");
    let team = &report["fields"]["boards"][0];
    assert_eq!(team["code"], "autosync::scan::scanner_missing", "{team}");
    assert_eq!(shared.remote_head(), theirs);
    assert!(
        shared
            .clone
            .head()
            .unwrap()
            .files
            .contains_key("theirs.json")
    );
}

/// A `.gitattributes` that marks every file as binary makes `git log -p`
/// print no text for them; the scan reads them as text anyway. The twin
/// with the same attributes and no secret is pushed.
#[test]
fn a_gitattributes_marking_files_binary_hides_no_secret_and_its_clean_twin_is_pushed() {
    let (mac, shared) = team();
    let attributes = [(".gitattributes", "* -diff\n")];
    let clean = shared.commit(&[attributes[0], ("notes/a.json", "{}")]);
    assert!(mac.round().0);
    assert_eq!(shared.remote_head(), clean);
    let config = format!("{{\"token\": \"{}\"}}", secret());
    shared.commit(&[attributes[0], ("notes/a.json", &config)]);
    let (_, boards) = mac.round();
    assert_eq!(
        boards["team"]["outcome"], "findings",
        "{:?}",
        boards["team"]
    );
    assert_eq!(boards["team"]["findings"][0]["file"], "notes/a.json");
    assert_eq!(shared.remote_head(), clean);
}

/// A merge made by hand whose result adds a secret neither parent holds is
/// scanned against its first parent, so the secret blocks the push.
#[test]
fn a_secret_a_hand_made_merge_adds_blocks_the_push() {
    let (mac, shared) = team();
    shared
        .remote
        .commit(&[(".board/board.json", CONFIG), ("notes/theirs.json", "{}")]);
    let theirs = shared.remote_head();
    git(shared.clone.workdir(), &["fetch", "origin"]);
    let mine = shared.commit(&[("notes/mine.json", "{}")]);
    let config = format!("{{\"token\": \"{}\"}}", secret());
    let tree = [
        (".board/board.json", CONFIG),
        ("notes/mine.json", "{}"),
        ("notes/theirs.json", "{}"),
        ("notes/resolved.json", config.as_str()),
    ];
    let merge = shared
        .clone
        .commit_at(&tree, &[mine, theirs], 1_700_000_000, "merge by hand");
    for (path, text) in tree {
        shared.clone.write(path, Item::File(text));
    }
    shared.clone.track(merge);
    let (_, boards) = mac.round();
    let team = &boards["team"];
    assert_eq!(team["outcome"], "findings", "{team}");
    assert_eq!(team["findings"][0]["file"], "notes/resolved.json", "{team}");
    assert_eq!(shared.remote_head(), theirs);
}

#[test]
fn a_conflict_moves_nothing_and_pushes_nothing() {
    let (mac, shared) = team();
    shared
        .remote
        .commit(&[(".board/board.json", CONFIG), ("notes/a.json", "1")]);
    let theirs = shared.remote_head();
    let mine = shared.commit(&[("notes/a.json", "2")]);
    let (_, boards) = mac.round();
    assert_eq!(boards["team"]["code"], "board::sync::conflicted");
    assert_eq!(shared.clone.head().unwrap().id, mine);
    assert_eq!(shared.remote_head(), theirs);
}

#[test]
fn uncommitted_files_are_byte_identical_after_a_run() {
    let (mac, shared) = team();
    shared.commit(&[("notes/a.json", "{}")]);
    shared.clone.write("notes/draft.md", Item::File("draft\n"));
    shared
        .clone
        .write("notes/a.json", Item::File("{\"edited\": true}"));
    mac.round();
    let read = |path: &str| std::fs::read_to_string(shared.clone.workdir().join(path)).unwrap();
    assert_eq!(read("notes/draft.md"), "draft\n");
    assert_eq!(read("notes/a.json"), "{\"edited\": true}");
}

#[test]
fn a_dry_run_checks_each_board_and_changes_nothing() {
    let (mac, shared) = team();
    let before = shared.remote_head();
    let mine = shared.commit(&[("notes/a.json", "{}")]);
    let (code, report) = mac.run(&["autosync", "run", "--dry-run"]);
    assert_eq!(code, 0, "{report}");
    let team = &report["data"]["output"]["boards"][0];
    assert_eq!(team["outcome"], "checked");
    assert!(
        team["message"].as_str().unwrap().contains("1 commits"),
        "{team}"
    );
    assert_eq!(shared.remote_head(), before);
    assert_eq!(shared.clone.head().unwrap().id, mine);
    assert!(!mac.state().join("status.json").exists());
}

/// The signature `autosync add` writes verifies through Board's host for
/// its own path; the same record and signature copied to another path do
/// not, because the signed statement names the path.
#[test]
fn the_signature_add_writes_verifies_for_its_path_and_a_copy_elsewhere_does_not() {
    let (mac, _) = team();
    let record = mac.personal.record("/autosync/boards/team").unwrap();
    let directory = mac.personal.board.workdir().to_path_buf();
    let put = (mac.personal).run_in(
        &directory,
        &["put", "/autosync/boards/copy"],
        &record.to_string(),
    );
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
    let (code, report) = mac.run(&["autosync", "run", "--dry-run"]);
    let boards = if code == 0 {
        report["data"]["output"]["boards"].clone()
    } else {
        report["fields"]["boards"].clone()
    };
    let outcome = |name: &str| {
        (boards.as_array().unwrap().iter())
            .find(|entry| entry["name"] == name)
            .map(|entry| entry["code"].clone())
            .unwrap()
    };
    assert_eq!(outcome("copy"), "autosync::board::unsigned", "{boards}");
    assert_ne!(outcome("team"), "autosync::board::unsigned", "{boards}");
}

#[test]
fn a_board_on_another_branch_is_skipped_and_back_on_its_branch_syncs() {
    let (mac, shared) = team();
    let new = shared.commit(&[("notes/a.json", "{}")]);
    let before = shared.remote_head();
    shared
        .clone
        .write(".git/HEAD", Item::File("ref: refs/heads/other\n"));
    shared
        .clone
        .write(".git/refs/heads/other", Item::File(&format!("{new}\n")));
    let (_, boards) = mac.round();
    assert_eq!(boards["team"]["outcome"], "skipped");
    assert_eq!(boards["team"]["code"], "board::sync::other_branch");
    assert_eq!(shared.remote_head(), before);
    shared
        .clone
        .write(".git/HEAD", Item::File("ref: refs/heads/main\n"));
    assert!(mac.round().0);
    assert_eq!(shared.remote_head(), new);
}

/// Every git command the plugin and gitleaks run goes through a stub that
/// logs it; none forces, rebases, stashes or stages. The sync's own fetch
/// and push start git from Board's PATH, which the stub is not on; their
/// arguments are tested in board-tree's `src/transport/tests.rs`.
#[test]
fn no_git_command_the_plugin_runs_forces_rebases_stashes_or_stages() {
    let (mac, shared) = team();
    let log = mac.bin.workdir().join("git.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec /usr/bin/git \"$@\"\n",
        log.display()
    );
    mac.bin.write("git", Item::Executable(&script));
    shared.commit(&[("notes/a.json", "{}")]);
    assert!(mac.round().0);
    let ran = std::fs::read_to_string(log).unwrap();
    assert!(ran.contains("remote get-url --push --all origin"), "{ran}");
    for line in ran.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        for refused in ["--force", "rebase", "stash", "add", "commit", "reset"] {
            assert!(!words.contains(&refused), "{line}");
        }
        assert!(!words.iter().any(|word| word.starts_with('+')), "{line}");
    }
}
