//! A plugin call refused as not enabled in a linked worktree of a board's
//! own repository names, in its hint, each known board that mounts the
//! repository and enables the plugin, as `board --board <dir> call ...`.
//! The refusal's code, message, fields and exit code stay as they are.

use std::path::PathBuf;

use board_scenario::seed::mount_of;

use super::*;

/// Board A, a repository with its own board that enables no plugin, and a
/// linked worktree of it holding that board; known boards that mount A and
/// enable what a test asks; and the known boards file in `xdg`.
struct Worktree {
    own: Scenario,
    linked: PathBuf,
    xdg: Scenario,
    cache: tempfile::TempDir,
}

impl Worktree {
    fn new() -> Self {
        let own = Scenario::board();
        let head = own.commit(&[("x.json", "1")]);
        // A linked worktree as `git worktree add --detach` lays it out.
        let linked = format!("{}/linked", own.path());
        let git_dir = format!("{}/.git/worktrees/linked", own.path());
        own.write(
            ".git/worktrees/linked/HEAD",
            Item::File(&format!("{head}\n")),
        )
        .write(".git/worktrees/linked/commondir", Item::File("../..\n"))
        .write(
            ".git/worktrees/linked/gitdir",
            Item::File(&format!("{linked}/.git\n")),
        )
        .write("linked/.git", Item::File(&format!("gitdir: {git_dir}\n")))
        .write(
            "linked/.board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        );
        Self {
            linked: PathBuf::from(linked).canonicalize().unwrap(),
            own,
            xdg: Scenario::new(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    /// A board that mounts A at `/repos/a` and enables each of `plugins`.
    fn mounting(&self, plugins: &[&str]) -> Scenario {
        let commit = self.own.commit(&[("x.json", "1")]);
        self.holding(&mount_of(&self.own.path(), commit), plugins)
    }

    /// A board holding `record` at `/repos/a` and enabling each of
    /// `plugins`. It does not commit, so enabling needs no git identity.
    fn holding(&self, record: &str, plugins: &[&str]) -> Scenario {
        let board = Scenario::new();
        let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
        board.write(".board/board.json", Item::File(config));
        board.write("repos/a.json", Item::File(record));
        for plugin in plugins {
            let enabled = self.run_in(board.workdir(), &["plugin", "enable", plugin]);
            assert_eq!(enabled.code, 0, "{}", text(&enabled.stdout));
        }
        board
    }

    /// Writes the known boards file listing `boards`.
    fn list(&self, boards: &[&Scenario]) {
        let roots: Vec<String> = boards.iter().map(|board| board.path()).collect();
        let known = json!({"boards": roots}).to_string();
        self.xdg.write("board/config.json", Item::File(&known));
    }

    /// Runs `board --json` in `directory` with the known boards file.
    fn run_in(&self, directory: &Path, arguments: &[&str]) -> Output {
        let context = Context {
            start: directory.to_path_buf(),
            home: None,
            cache: Some(self.cache.path().to_path_buf()),
            plugins: None,
            boards: Some(self.xdg.workdir().join("board/config.json")),
            signer: None,
            allowed_signers: None,
            attention_allowed_signers: None,
            environment: board_plugin::Environment {
                path: Some(PATH.into()),
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
        run(arguments, &context, &mut &b""[..], &mut Ran::default())
    }

    /// Runs `board --json` in the linked worktree.
    fn run(&self, arguments: &[&str]) -> Output {
        self.run_in(&self.linked, arguments)
    }
}

/// A refused run's `--json` failure, which must be `board::plugin::not_enabled`
/// for `plugin` with exit code 1.
fn not_enabled(output: &Output, plugin: &str) -> Value {
    assert_eq!(output.code, 1, "{}", text(&output.stdout));
    let failure = json(output);
    assert_eq!(failure["code"], "board::plugin::not_enabled", "{failure}");
    assert_eq!(failure["fields"]["plugin"], plugin, "{failure}");
    failure
}

/// The hint of a `not_enabled` refusal of `plugin`.
fn hint(output: &Output, plugin: &str) -> String {
    let failure = not_enabled(output, plugin);
    failure["recovery"]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Today's hint for a plugin that is not enabled.
fn enable_hint(plugin: &str) -> String {
    format!(
        "run `board plugin enable {plugin}`, which writes the schemas the plugin ships and adds it to `plugins` in `.board/board.json`"
    )
}

#[test]
fn a_call_refused_in_a_linked_worktree_names_the_known_board_that_mounts_it_and_that_board_then_runs_it()
 {
    let worktree = Worktree::new();
    let mounts = worktree.mounting(&["plans"]);
    worktree.list(&[&mounts]);
    let through = format!("`board --board {} call plans.open`", mounts.path());
    for arguments in [&["call", "plans.open"][..], &["plans", "open"]] {
        let refused = worktree.run(arguments);
        let failure = not_enabled(&refused, "plans");
        let today = worktree.run_in(worktree.own.workdir(), arguments);
        // The message and fields are as a run in the board's own main
        // worktree gives them.
        let main = json(&today);
        assert_eq!(failure["message"], main["message"], "{failure}");
        assert_eq!(failure["fields"], main["fields"], "{failure}");
        let hint = failure["recovery"]["text"].as_str().unwrap();
        assert!(hint.contains(&through), "{hint}");
        assert!(hint.contains("same input"), "{hint}");
        assert!(!hint.contains("plugin enable"), "{hint}");
    }
    let ran = worktree.run(&["--board", &mounts.path(), "call", "plans.open"]);
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
}

#[test]
fn a_github_push_refused_in_a_linked_worktree_names_the_known_board_and_pushes_nothing() {
    let worktree = Worktree::new();
    let mounts = worktree.mounting(&["github"]);
    worktree.list(&[&mounts]);
    let refused = worktree.run(&["github", "push"]);
    let failure = not_enabled(&refused, "github");
    let hint = failure["recovery"]["text"].as_str().unwrap();
    let through = format!("`board --board {} call github.push`", mounts.path());
    assert!(hint.contains(&through), "{hint}");
    assert!(!hint.contains("plugin enable"), "{hint}");
    // No plugin process ran, so no git push did.
    assert_eq!(failure.get("stderr"), None, "{failure}");
}

#[test]
fn two_known_boards_that_mount_the_repository_and_enable_the_plugin_are_both_named() {
    let worktree = Worktree::new();
    let (one, two) = (worktree.mounting(&["plans"]), worktree.mounting(&["plans"]));
    worktree.list(&[&one, &two]);
    let hint = hint(&worktree.run(&["call", "plans.open"]), "plans");
    for board in [&one, &two] {
        let through = format!("`board --board {} call plans.open`", board.path());
        assert!(hint.contains(&through), "{hint}");
    }
}

#[test]
fn without_a_known_board_that_mounts_the_repository_and_enables_the_plugin_the_hint_is_todays() {
    let worktree = Worktree::new();
    // No known boards file.
    let none = hint(&worktree.run(&["call", "plans.open"]), "plans");
    assert_eq!(none, enable_hint("plans"));
    // A known board that mounts the repository but does not enable the plugin.
    let disabled = worktree.mounting(&[]);
    worktree.list(&[&disabled]);
    let off = hint(&worktree.run(&["call", "plans.open"]), "plans");
    assert_eq!(off, enable_hint("plans"));
}

#[test]
fn a_known_boards_file_that_cannot_be_read_refuses_the_call_as_before() {
    let worktree = Worktree::new();
    worktree.mounting(&["plans"]);
    worktree
        .xdg
        .write("board/config.json", Item::File("not json"));
    let refused = worktree.run(&["call", "plans.open"]);
    assert_eq!(refused.code, 1, "{}", text(&refused.stdout));
    let failure = json(&refused);
    assert_eq!(failure["code"], "board::config::invalid", "{failure}");
}

#[test]
fn a_board_chosen_with_a_flag_keeps_todays_hint() {
    let worktree = Worktree::new();
    let mounts = worktree.mounting(&["plans"]);
    worktree.list(&[&mounts]);
    let own = worktree.linked.display().to_string();
    let chosen = [
        vec!["-C", ".", "call", "plans.open"],
        vec!["--board", own.as_str(), "call", "plans.open"],
    ];
    for arguments in chosen {
        let hint = hint(&worktree.run(&arguments), "plans");
        assert_eq!(hint, enable_hint("plans"), "{arguments:?}");
    }
    // `--personal` runs as the personal board, here the linked worktree's own.
    let personal = json!({"boards": [mounts.path()], "personal": own}).to_string();
    worktree
        .xdg
        .write("board/config.json", Item::File(&personal));
    let hint = hint(
        &worktree.run(&["--personal", "call", "plans.open"]),
        "plans",
    );
    assert_eq!(hint, enable_hint("plans"));
}

#[test]
fn a_call_refused_in_a_mounted_clone_with_no_board_of_its_own_keeps_todays_hint() {
    let worktree = Worktree::new();
    // A clone with no `.board`: one known board mounts it without enabling
    // plans, and opens; another has a repo record for its origin and enables
    // plans.
    let clone = Scenario::new();
    let commit = clone.commit(&[("x.json", "1")]);
    let remote = "[remote \"origin\"]\n\turl = git@github.com:example-org/lib.git\n";
    clone.write(".git/config", Item::Appended(remote));
    let mounts = worktree.holding(&mount_of(&clone.path(), commit), &[]);
    let record = json!({"$type": "board/repo/v1", "remote": "example-org/lib"});
    let recorded = worktree.holding(&record.to_string(), &["plans"]);
    worktree.list(&[&mounts, &recorded]);
    let refused = worktree.run_in(clone.workdir(), &["call", "plans.open"]);
    assert_eq!(hint(&refused, "plans"), enable_hint("plans"));
}

#[test]
fn a_known_board_whose_path_a_shell_would_split_is_quoted_in_the_hint() {
    let worktree = Worktree::new();
    let mounts = worktree.mounting(&["plans"]);
    let links = Scenario::new();
    links.write("my boards/b", Item::Symlink(&mounts.path()));
    let listed = links.workdir().join("my boards/b");
    let known = json!({"boards": [listed]}).to_string();
    worktree.xdg.write("board/config.json", Item::File(&known));
    let hint = hint(&worktree.run(&["call", "plans.open"]), "plans");
    let through = format!("`board --board '{}' call plans.open`", listed.display());
    assert!(hint.contains(&through), "{hint}");
    assert!(
        hint.contains(&format!("known board '{}'", listed.display())),
        "{hint}"
    );
}

#[test]
fn a_plugin_word_given_flags_says_its_input_goes_as_json_on_stdin() {
    let worktree = Worktree::new();
    let mounts = worktree.mounting(&["plans"]);
    worktree.list(&[&mounts]);
    let flagged = hint(&worktree.run(&["plans", "open", "--limit", "5"]), "plans");
    assert!(
        flagged.contains(r#"with `{"limit":5}` as JSON on stdin"#),
        "{flagged}"
    );
    let bare = hint(&worktree.run(&["plans", "open"]), "plans");
    assert!(bare.contains("with the same input"), "{bare}");
}
