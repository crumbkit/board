//! Board's help, run as written: each example in a command's help runs on a
//! fixture board and prints its golden output, and each `-h` page fits a
//! byte budget (STANDARDS.md 24).

use clap::{Command, CommandFactory};
use crumbkit::Tool;
use crumbkit::help::RuntimeLeaf;
use crumbkit_test::{Allow, Budget, Budgets, Check, Examples, Suite};

use super::cli::{examples, words};
use super::*;
use crate::run::Cli;

/// The `board` binary cargo built beside this test binary
/// (`target/debug/deps/board-<hash>`). cargo sets `CARGO_BIN_EXE_board`
/// only for integration tests, and an integration test cannot reach `Cli`.
pub(super) fn binary() -> String {
    let test = std::env::current_exe().unwrap();
    let path = test.parent().unwrap().parent().unwrap().join("board");
    path.to_str().unwrap().to_owned()
}

/// Board's command tree with a tool record whose leaves hold the examples
/// each command's own help ends with. crumbkit runs no shell: an example's
/// `< file` becomes its golden's `.stdin` file, and of a pipe each command
/// belongs to its own leaf.
pub(super) fn build() -> (Command, Tool) {
    fn walk(path: &[String], command: &Command, leaves: &mut Vec<RuntimeLeaf>) {
        for sub in command
            .get_subcommands()
            .filter(|sub| sub.get_name() != "help")
        {
            let mut below = path.to_vec();
            below.push(sub.get_name().to_owned());
            if sub.has_subcommands() {
                walk(&below, sub, leaves);
                continue;
            }
            let prefix = format!("board {} ", below.join(" "));
            let help = sub.get_after_help().map(ToString::to_string);
            let mut leaf = RuntimeLeaf::new(below.clone());
            for line in examples(&help.unwrap_or_default()) {
                if format!("{line} ").starts_with(&prefix) {
                    let rest = words(&line).split_off(below.len() + 1);
                    leaf = leaf.example(rest, "an example from board's help".to_owned());
                }
            }
            leaves.push(leaf);
        }
    }
    let command = Cli::command();
    let mut leaves = Vec::new();
    walk(&[], &command, &mut leaves);
    (command, Tool::new().with_runtime_leaves(leaves))
}

/// The fixture board does not enable tasks, whose sweep needs notes repos
/// the fixture does not hold.
const NO_CALL: &str = "the fixture board does not enable the tasks plugin";
/// A pending effect's id hashes the directory of the board that asked, so
/// no fixture holds one with the id an example names.
const NO_ANSWER: &str = "a pending effect's id hashes the asking board's directory, so no fixture holds the example's id";
/// `serve` runs until it is idle, and `ui open` starts a server and a
/// browser; board-serve/src/tests.rs and the ui tests here run both.
const SERVES: &str = "the command starts a server that outlives the example's time limit";
const ALLOW: &[Allow] = &[
    Allow::example(
        "answer",
        &[
            "4b2e0c8a9d1f37b6e5a0c2d84f9b1e7a3c6d0f8b2a5e9c1d4f7a0b3e6c9d2f5a",
            "approve",
        ],
        NO_ANSWER,
    ),
    Allow::example(
        "answer",
        &[
            "4b2e0c8a9d1f37b6e5a0c2d84f9b1e7a3c6d0f8b2a5e9c1d4f7a0b3e6c9d2f5a",
            "refuse",
            "--reason",
            "not this week",
        ],
        NO_ANSWER,
    ),
    Allow::example(
        "answer",
        &[
            "4b2e0c8a9d1f37b6e5a0c2d84f9b1e7a3c6d0f8b2a5e9c1d4f7a0b3e6c9d2f5a",
            "approve",
            "--signature",
            "phone.sig",
        ],
        NO_ANSWER,
    ),
    Allow::example(
        "uri",
        &["/tasks/fix-login"],
        "the fixture's origin is a temporary directory, so no golden output can pin its URI",
    ),
    Allow::example("serve", &[], SERVES),
    Allow::example("serve", &["--idle", "2m"], SERVES),
    Allow::example("ui open", &["/bindings/review"], SERVES),
    Allow::example("call", &["tasks.available"], NO_CALL),
    Allow::example("call", &["tasks.add"], NO_CALL),
    Allow::example("call", &["--capability", "available"], NO_CALL),
    Allow::example(
        "put",
        &["--new", "/tasks/"],
        "the name holds the put's minute and a random suffix, so no golden output can pin it",
    ),
    Allow::example(
        "put",
        &["--new", "--var", "project=board"],
        "the name holds the put's minute and a random suffix, so no golden output can pin it",
    ),
    Allow::example(
        "put",
        &["/tasks/a", "--expect", "none"],
        "the get and refs examples read /tasks/a, so the fixture board holds it",
    ),
];

/// Each run commits with a fixed time, so the commit ids `put` prints stay
/// the same.
const ENV: &[(&str, &str)] = &[
    ("PATH", "/usr/bin:/bin"),
    ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
    ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
];

#[test]
fn every_help_example_runs_on_a_fixture_board_and_prints_its_golden_output() {
    // The mount sits one commit behind `main`, so `mount update` moves it.
    let library = Scenario::new();
    let old = library.commit(&[("a.md", "one")]);
    library.commit(&[("a.md", "two")]);
    let scenario = board();
    let task = |title: &str| {
        format!(r#"{{"$type": "lr/task/v1", "title": "{title}", "project": "/projects/p"}}"#)
    };
    scenario
        .write("tasks/a.json", Item::File(&task("a")))
        .write("tasks/b.json", Item::File(&task("b")))
        .write("tasks/c.json", Item::File(&task("c")))
        .write("README.md", Item::File("# A fixture board\n"))
        .write(
            "notes/plan.md",
            Item::File("Do [a](../tasks/a.json), then [b](/tasks/b#title).\n"),
        )
        .write(
            "vendor/lib.json",
            Item::File(&mount_of(&library.path(), old)),
        )
        .identified();
    // The board follows a remote that holds its files in one commit, which
    // `sync` checks out; the mount pointer names a directory of this run,
    // so it stays out of the commit and the commit id stays the same.
    let remote = Scenario::new();
    let held: Vec<(String, String)> = files_of(scenario.workdir())
        .into_iter()
        .filter(|(path, _)| path != "vendor/lib.json")
        .collect();
    let held: Vec<(&str, &str)> = held
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    remote.commit(&held);
    scenario.write(
        ".git/config",
        Item::Appended(&format!(
            "[remote \"origin\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\
             [branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n",
            remote.path()
        )),
    );
    // tasks is built in, so the plugins directory holds no plugin.
    let plugins = Scenario::new();
    plugins.write("xdg/data/board/.keep", Item::File(""));
    let (binary, fixture) = (binary(), scenario.path());
    let xdg = format!("{}/xdg", plugins.path());
    let goldens = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tests/help/examples");
    let examples = Examples::new(&binary, goldens)
        .env(ENV)
        .fresh_dir(&fixture)
        .xdg(&xdg)
        .allow(ALLOW);
    Suite::new(build).examples(examples).run(Check::Examples);
}

/// Every text file below `root` but git's own, by `/`-separated path.
fn files_of(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != ".git") {
                    pending.push(path);
                }
                continue;
            }
            let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
            files.push((relative.to_owned(), std::fs::read_to_string(&path).unwrap()));
        }
    }
    files
}

/// crumbkit's defaults are 1,500 bytes for the root page, 800 for a group
/// such as `plugin` and 1,200 for each other command.
pub(super) const BUDGETS: Budgets = Budgets::new()
    .root(
        3_050,
        "The root page lists all twenty-one commands under the goal each serves, the commands a newcomer runs first, and what a logical path and a record are, which STANDARDS.md 24 requires on one page. It also lists the global flags -C and --personal, which choose the board a run uses, and one line naming at most six of the plugins the board enables, which run as `board <plugin> <capability>`.",
    )
    .nodes(&[
        Budget::new(
            "get",
            1_400,
            "get has two output formats and --fetch, and a reader needs to know on one page that without --fetch get prints an attachment record and never reaches the store.",
        ),
        Budget::new(
            "put",
            1_620,
            "put has seven options of its own (--new, --var, --expect, --batch, --bulk, --file and --wait), and a reader needs to know on one page that with a store --file may write an attachment record in place of the bytes. Its page also ends with the Effects: line and a global flags line that names --no-input.",
        ),
        Budget::new(
            "refs",
            2_000,
            "refs has five options and two output formats, and a reader needs the from/to line format and the --field encoding on one page to parse its text output. Its page also ends with the Effects: line and a global flags line that names --no-input.",
        ),
        Budget::new(
            "decide",
            1_260,
            "decide says on one page what the owner's approval, refusal and silence each do, its Effects: line says that it adds, and its global flags line names --no-input, which a program adds to every run.",
        ),
        Budget::new(
            "answer",
            1_260,
            "answer says on one page how an approval is signed and checked, its Effects: line says that it is destructive, and its global flags line names --no-input, which a program adds to every run.",
        ),
    ]);
