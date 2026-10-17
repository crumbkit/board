//! Board's help, run as written: each example in a command's help runs on a
//! fixture board and prints its golden output, and each `-h` page fits a
//! byte budget (STANDARDS.md 24).

use clap::CommandFactory;
use crumbkit::help::{Guide, RuntimeLeaf};
use crumbkit_test::{Allow, Budget, Budgets, Examples, Pages, Rule};

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

/// A guide whose leaves hold the examples each command's own help ends
/// with. crumbkit runs no shell: an example's `< file` becomes its golden's
/// `.stdin` file, and of a pipe each command belongs to its own leaf.
pub(super) fn guide() -> Guide {
    fn walk(path: &[String], command: &clap::Command, leaves: &mut Vec<RuntimeLeaf>) {
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
    let mut leaves = Vec::new();
    walk(&[], &Cli::command(), &mut leaves);
    Guide::new().with_runtime_leaves(leaves)
}

/// The tasks plugin runs through `uv` from its checkout, which its README
/// installs as a symbolic link, and crumbkit copies no symbolic link into a
/// run's XDG directories.
const NO_CALL: &str = "the tasks plugin runs only from a symbolic link to its checkout";
const ALLOW: &[Allow] = &[
    Allow::example("call", &["tasks.due"], NO_CALL),
    Allow::example("call", &["--capability", "due"], NO_CALL),
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
            "vendor/lib.json",
            Item::File(&mount_of(&library.path(), old)),
        )
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        );
    let plugin = concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/tasks");
    let plugins = Scenario::new();
    for file in ["plugin.json", "types/tasks/task/v1.schema.json"] {
        let text = std::fs::read_to_string(format!("{plugin}/{file}")).unwrap();
        plugins.write(
            &format!("xdg/data/board/plugins/tasks/{file}"),
            Item::File(&text),
        );
    }
    let (binary, fixture) = (binary(), scenario.path());
    let xdg = format!("{}/xdg", plugins.path());
    let goldens = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tests/help/examples");
    let examples = Examples::new(&binary, goldens)
        .env(ENV)
        .fresh_dir(&fixture)
        .xdg(&xdg)
        .allow(ALLOW);
    crumbkit_test::check_examples(&Cli::command(), &guide(), &examples);
}

/// crumbkit's defaults are 1,500 bytes for the root page, 800 for a group
/// such as `plugin` and 1,200 for each other command.
const BUDGETS: Budgets = Budgets::new()
    .root(
        2_000,
        "The root page lists all ten commands, the commands a newcomer runs first, and what a logical path and a record are, which STANDARDS.md 24 requires on one page.",
    )
    .nodes(&[Budget::new(
        "refs",
        1_950,
        "refs has five options and two output formats, and a reader needs the from/to line format and the --field encoding on one page to parse its text output.",
    )]);

/// Why each document's rows print whole: `--json` gives every field without
/// `--full`.
const WHOLE: &str = "an agent that asks for --json gets every field without needing a second flag";

/// Each document Board prints has its id, and each rule it breaks has an
/// opt-out with a reason.
#[test]
fn each_document_declares_its_id_and_its_opt_outs() {
    let guide = Guide::new().leaves(crate::render::LEAVES);
    let allow = [
        Allow::new(Rule::RowsWhole, "ls", WHOLE),
        Allow::new(Rule::RowsWhole, "check", WHOLE),
        Allow::new(Rule::RowsWhole, "query", WHOLE),
        Allow::new(Rule::RowsWhole, "plugin list", WHOLE),
        Allow::new(Rule::RowsWhole, "plugin find", WHOLE),
    ];
    crumbkit_test::check_document_ids(&Cli::command(), &guide, &allow);
}

#[test]
fn every_dash_h_page_fits_its_byte_budget() {
    let binary = binary();
    let (examples, guide) = (Examples::new(&binary, ""), Guide::new());
    let pages = Pages::binary(&guide, &examples);
    crumbkit_test::check_help_sizes(&Cli::command(), pages, &BUDGETS);
}
