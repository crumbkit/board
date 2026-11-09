//! Board against crumbkit's conventions: one `Suite` holds the inputs of
//! every check, and `conventions!` expands it to one test for each. A check
//! whose input is a fixture board that must be removed afterwards runs from
//! the test that builds the fixture, and the suite skips it with the name of
//! that test.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use clap::{Command, CommandFactory};
use crumbkit::Tool;
use crumbkit::help::{Example, Help, Leaf};
use crumbkit_test::{Allow, Check, Examples, Rule, Suite};

use super::cli::{examples, words};
use super::describe::EFFECTS;
use super::help::{BUDGETS, WHOLE, binary};
use crate::run::Cli;

/// Board's command tree and its tool record, with the examples that each
/// command's own help ends with added to its leaf. crumbkit runs no shell:
/// an example's `< file` becomes its golden's `.stdin` file, and of a pipe
/// each command belongs to its own leaf.
pub(super) fn build() -> (Command, Tool) {
    let command = Cli::command();
    let mut found = BTreeMap::new();
    collect(&[], &command, &mut found);
    let mut leaves: Vec<Leaf> = crate::render::TOOL.get_help().get_leaves().to_vec();
    for leaf in &mut leaves {
        let path: Vec<String> = leaf.path().iter().map(|word| (*word).to_owned()).collect();
        if let Some(listed) = found.remove(&path) {
            *leaf = leaf.examples(listed.leak());
        }
    }
    for (path, listed) in found {
        let path: Vec<&'static str> = path.into_iter().map(|word| &*word.leak()).collect();
        leaves.push(Leaf::new(path.leak()).examples(listed.leak()));
    }
    let help = crate::render::TOOL.get_help();
    let help = Help::new().groups(help.get_groups()).leaves(leaves.leak());
    (command, crate::render::TOOL.help(help))
}

/// The examples in the help of each leaf below `command`, by the leaf's path.
fn collect(path: &[String], command: &Command, found: &mut BTreeMap<Vec<String>, Vec<Example>>) {
    for sub in command
        .get_subcommands()
        .filter(|sub| sub.get_name() != "help")
    {
        let mut below = path.to_vec();
        below.push(sub.get_name().to_owned());
        if sub.has_subcommands() {
            collect(&below, sub, found);
            continue;
        }
        let prefix = format!("board {} ", below.join(" "));
        let help = sub.get_after_help().map(ToString::to_string);
        let mut listed = Vec::new();
        for line in examples(&help.unwrap_or_default()) {
            if format!("{line} ").starts_with(&prefix) {
                let rest = words(&line).split_off(below.len() + 1);
                let rest: Vec<&'static str> = rest.into_iter().map(|word| &*word.leak()).collect();
                let about = "an example from board's help";
                listed.push(Example::new(rest.leak(), about));
            }
        }
        found.insert(below, listed);
    }
}

/// Every opt-out of the checks the suite runs.
fn allow() -> &'static [Allow] {
    static ALLOW: OnceLock<Vec<Allow>> = OnceLock::new();
    ALLOW.get_or_init(|| {
        let mut allow = EFFECTS.to_vec();
        for command in ["ls", "check", "query", "plugin list", "plugin find"] {
            allow.push(Allow::new(Rule::RowsWhole, command, WHOLE));
        }
        allow
    })
}

/// The suite that `conventions!` expands. The checks that need a scratch
/// board or an output directory run from the tests that build one, and the
/// checks Board has not adopted are skipped with the reason.
fn suite() -> Suite<'static> {
    let binary: &'static str = binary().leak();
    Suite::new(build)
        .allow(allow())
        .budgets(BUDGETS)
        .examples(Examples::new(binary, ""))
        .skip(Check::Grammar, "board's root verbs (get, ls, put, check) are single words by design, so the resource-verb form is not adopted")
        .skip(Check::Filters, "board's repeatable options (-C, --field, --var, --meta) take one value per repeat, so comma lists are not adopted")
        .skip(Check::Help, "board check and board refs print rows that the help rules' column and Bytes-field checks do not match, and the call example in the help is not adopted")
        .skip(Check::PathWeights, "no per-path help weight budget is declared; help_sizes in help.rs pins each page's size")
        .skip(Check::HelpGoldens, "help pages are pinned by the size budgets in help.rs and the command reference in docs/commands.md, not by golden files")
        .skip(Check::SchemaGoldens, "document schemas are pinned by the released.json tests, not by golden files")
        .skip(Check::OutputBudget, "the output budget is tested by crates/board-cli/tests/output_budget.rs")
        .skip(Check::Examples, "the examples test in help.rs runs the examples on a fixture board that it builds and removes")
        .skip(Check::CommandReference, "the command reference test in describe.rs runs it with the binary on a scratch board")
        .skip(Check::Recoveries, "the recoveries test in recoveries.rs runs it with its scratch scenarios")
        .skip(Check::Twins, "the twins test in recoveries.rs runs it with its scratch scenarios")
}

crumbkit_test::conventions!(suite());
