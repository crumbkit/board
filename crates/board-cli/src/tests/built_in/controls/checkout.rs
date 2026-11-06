//! `board --checkout call controls.check` from a linked worktree of a
//! mounted repository measures the branch committed there, and says which
//! commit it read; without the flag it measures the mount's pin
//! (crumbkit/board#127).

use super::*;

/// `count` words.
fn words(count: usize) -> String {
    vec!["word"; count].join(" ")
}

/// A board that enables controls, with a budget of 300 words on the mounted
/// repository's `docs/usage.md`, and a known boards file listing it.
/// `library` is mounted at `/repos/lib` at commit A, where usage.md holds
/// 294 words. Its linked worktree `linked` is at commit B, where the file
/// holds 326, and its main worktree is at a later commit C.
struct Checkout {
    board: Fixture,
    xdg: Scenario,
    library: Scenario,
    linked: std::path::PathBuf,
    pinned: ObjectId,
    head: ObjectId,
}

impl Checkout {
    fn new() -> Self {
        let board = board();
        board.write(
            "controls/usage.json",
            &control(
                "Short usage.",
                &json!({
                    "paths": ["repos/lib/docs/usage.md"],
                    "rule": {"budget": "words", "max": 300},
                }),
            ),
        );
        let library = Scenario::new();
        let pinned = library.commit(&[("docs/usage.md", &words(294))]);
        let head = library.commit(&[("docs/usage.md", &words(326))]);
        library.commit(&[("docs/usage.md", &words(400))]);
        mount(&board, "repos/lib.json", &library, pinned);
        // A linked worktree as `git worktree add --detach` lays it out.
        let linked = format!("{}/linked", library.path());
        let git_dir = format!("{}/.git/worktrees/linked", library.path());
        library
            .write(
                ".git/worktrees/linked/HEAD",
                Item::File(&format!("{head}\n")),
            )
            .write(".git/worktrees/linked/commondir", Item::File("../..\n"))
            .write(
                ".git/worktrees/linked/gitdir",
                Item::File(&format!("{linked}/.git\n")),
            )
            .write("linked/.git", Item::File(&format!("gitdir: {git_dir}\n")))
            .write("linked/docs/usage.md", Item::File(&words(326)));
        let xdg = Scenario::new();
        let known = json!({"boards": [board.board.path()]}).to_string();
        xdg.write("board/config.json", Item::File(&known));
        Self {
            board,
            xdg,
            library,
            linked: linked.into(),
            pinned,
            head,
        }
    }

    /// Runs `board --json` in `directory` with the known boards file, and
    /// the board's index cache unless `cold`.
    fn run_in(&self, directory: &Path, arguments: &[&str], cold: bool) -> Output {
        self.fed(directory, arguments, cold, "")
    }

    /// [`Self::run_in`] with `stdin` as standard input.
    fn fed(&self, directory: &Path, arguments: &[&str], cold: bool, stdin: &str) -> Output {
        let context = Context {
            start: directory.to_path_buf(),
            home: None,
            cache: (!cold).then(|| self.board.cache.path().to_path_buf()),
            plugins: None,
            boards: Some(self.xdg.workdir().join("board/config.json")),
            signer: None,
            allowed_signers: None,
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
        run(
            arguments,
            &context,
            &mut stdin.as_bytes(),
            &mut Ran::default(),
        )
    }

    /// `controls.check --path .` from the linked worktree, with `flags`
    /// before `call`.
    fn check(&self, flags: &[&str], cold: bool) -> Value {
        let arguments: Vec<&str> = (flags.iter().copied())
            .chain(["call", "controls.check", "--path", "."])
            .collect();
        let output = self.run_in(&self.linked, &arguments, cold);
        assert_eq!(output.code, 0, "{}", text(&output.stdout));
        json(&output)["data"]["output"].clone()
    }
}

/// The code of a refused run's `--json` failure.
fn refused_code(output: &Output) -> Value {
    assert_ne!(output.code, 0, "{}", text(&output.stdout));
    json(output)["code"].clone()
}

#[test]
fn with_checkout_the_check_measures_the_linked_worktrees_head_and_without_it_the_pin() {
    let checkout = Checkout::new();
    let branch = checkout.check(&["--checkout"], false);
    assert_eq!(branch["passed"], false, "{branch}");
    assert_eq!(branch["measured"][0]["used"], 326);
    assert_eq!(
        branch["read"],
        json!([{"mount": "/repos/lib", "commit": checkout.head.to_string()}])
    );
    let pin = checkout.check(&[], false);
    assert_eq!(pin["passed"], true, "{pin}");
    assert_eq!(pin["measured"][0]["used"], 294);
    assert_eq!(
        pin["read"],
        json!([{"mount": "/repos/lib", "commit": checkout.pinned.to_string()}])
    );
    let pointer = checkout.board.read("repos/lib.json");
    assert!(pointer.contains(&checkout.pinned.to_string()), "{pointer}");
}

#[test]
fn a_plain_check_after_a_checkout_one_reads_the_pin_from_a_warm_index_as_from_a_cold_one() {
    let checkout = Checkout::new();
    checkout.check(&["--checkout"], false);
    let warm = checkout.check(&[], false);
    assert_eq!(warm["measured"][0]["used"], 294);
    assert_eq!(warm, checkout.check(&[], true));
    let again = checkout.check(&["--checkout"], false);
    assert_eq!(again, checkout.check(&["--checkout"], true));
}

#[test]
fn checkout_from_the_boards_own_tree_is_refused_and_from_the_mounted_checkout_is_admitted() {
    let checkout = Checkout::new();
    let arguments = ["--checkout", "call", "controls.check", "--path", "/"];
    let own = checkout.run_in(checkout.board.board.workdir(), &arguments, false);
    assert_eq!(refused_code(&own), "board::cli::usage");
    let hint = json(&own)["recovery"]["text"].to_string();
    assert!(
        hint.contains("from a checkout of a repository a known board mounts"),
        "{hint}"
    );
    let mounted = checkout.run_in(&checkout.linked, &arguments, false);
    assert_eq!(mounted.code, 0, "{}", text(&mounted.stdout));
}

#[test]
fn checkout_from_a_separate_clone_is_refused_and_from_the_mounted_repository_is_admitted() {
    let checkout = Checkout::new();
    let arguments = ["--checkout", "get", "/repos/lib/docs/usage.md"];
    // A clone with the same origin is matched by origin, not as the mount.
    let origin = "https://example.com/acme/lib.git";
    checkout.library.write(
        ".git/config",
        Item::Appended(&format!("[remote \"origin\"]\n\turl = {origin}\n")),
    );
    let clone = Scenario::new();
    clone.commit(&[("docs/usage.md", "x")]);
    clone.write(
        ".git/config",
        Item::Appended(&format!("[remote \"origin\"]\n\turl = {origin}\n")),
    );
    let separate = checkout.run_in(clone.workdir(), &arguments, false);
    assert_eq!(refused_code(&separate), "board::cli::usage");
    let own = checkout.run_in(checkout.library.workdir(), &arguments, false);
    assert_eq!(own.code, 0, "{}", text(&own.stdout));
}

#[test]
fn checkout_with_put_is_refused_and_the_same_put_without_it_is_admitted() {
    let checkout = Checkout::new();
    let record = control("R.", &json!({}));
    let put = |flags: &[&str]| {
        let arguments: Vec<&str> = (flags.iter().copied())
            .chain(["put", "/controls/new"])
            .collect();
        checkout.fed(&checkout.linked, &arguments, false, &record)
    };
    assert_eq!(refused_code(&put(&["--checkout"])), "board::cli::usage");
    let admitted = put(&[]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stdout));
}

#[test]
fn checkout_with_personal_is_refused_and_checkout_alone_is_admitted() {
    let checkout = Checkout::new();
    let both = checkout.run_in(
        &checkout.linked,
        &["--checkout", "--personal", "ls", "."],
        false,
    );
    assert_eq!(both.code, 2, "{}", text(&both.stdout));
    assert_eq!(refused_code(&both), "board::cli::usage");
    let alone = checkout.run_in(&checkout.linked, &["--checkout", "ls", "."], false);
    assert_eq!(alone.code, 0, "{}", text(&alone.stdout));
}

#[test]
fn checkout_with_two_pointers_mounting_the_repository_is_refused_and_with_one_is_admitted() {
    let checkout = Checkout::new();
    let arguments = ["--checkout", "ls", "."];
    let one = checkout.run_in(&checkout.linked, &arguments, false);
    assert_eq!(one.code, 0, "{}", text(&one.stdout));
    mount(
        &checkout.board,
        "repos/again.json",
        &checkout.library,
        checkout.pinned,
    );
    let two = checkout.run_in(&checkout.linked, &arguments, false);
    assert_eq!(refused_code(&two), "board::config::ambiguous");
}

#[test]
fn checkout_whose_head_names_no_commit_is_refused_and_one_at_a_commit_is_admitted() {
    let checkout = Checkout::new();
    let arguments = ["--checkout", "ls", "."];
    let at_commit = checkout.run_in(&checkout.linked, &arguments, false);
    assert_eq!(at_commit.code, 0, "{}", text(&at_commit.stdout));
    checkout.library.write(
        ".git/worktrees/linked/HEAD",
        Item::File("ref: refs/heads/unborn\n"),
    );
    let unborn = checkout.run_in(&checkout.linked, &arguments, false);
    assert_eq!(refused_code(&unborn), "board::cli::usage");
    let message = json(&unborn)["message"].to_string();
    assert!(message.contains("HEAD that names no commit"), "{message}");
}
