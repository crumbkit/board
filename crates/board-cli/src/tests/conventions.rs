//! Board against crumbkit's conventions: one `Suite` holds the inputs of
//! every check, and `conventions!` expands it to one test for each. A check
//! whose input is a fixture that its test must assert on or remove runs from
//! the test that builds the fixture, and the suite skips it with the name of
//! that test.

use clap::CommandFactory;
use crumbkit_test::{Allow, Check, Examples, Rule, Suite};

use super::help::{BUDGETS, binary};
use crate::run::Cli;

/// Why a write that commits takes no `--yes`: it asks no question, and git
/// keeps the commit that a revert undoes.
const COMMITTED: &str = "the command asks no question, so it has no confirmation to skip, and git keeps each commit it makes, which a revert undoes";

/// Why `put` and `rm` keep their own `--expect`.
const BLOB: &str = "Board's --expect names the git blob the file must hold, or none for a new file, which crumbkit's Expect does not take";

/// Board's opt-outs from crumbkit's rules.
const ALLOW: &[Allow] = &[
    Allow::new(Rule::Yes, "put", COMMITTED),
    Allow::new(Rule::Yes, "rm", COMMITTED),
    Allow::new(Rule::Yes, "mount update", COMMITTED),
    Allow::new(
        Rule::Yes,
        "call",
        "the capability decides what it writes, and a plugin's capability that the owner must admit already waits for the owner through decide",
    ),
    Allow::new(
        Rule::Yes,
        "answer",
        "an approval waits for the owner's Touch ID unless --signature gives a signature made elsewhere, --no-input refuses that wait, and a refusal writes only the answer",
    ),
    Allow::new(
        Rule::Yes,
        "skills install",
        "it replaces and removes only folders that hold Board's marker, which a second install writes again, and a folder without the marker only with --force",
    ),
    Allow::new(Rule::ExpectFlag, "put", BLOB),
    Allow::new(Rule::ExpectFlag, "rm", BLOB),
    Allow::root_verbs(ROOT_VERBS, ROOT),
    Allow::option(Rule::RepeatsAndCommas, "", "-C", COMMAS),
    Allow::option(Rule::RepeatsAndCommas, "refs", "--field", COMMAS),
    Allow::option(Rule::RepeatsAndCommas, "put", "--var", COMMAS),
    Allow::option(Rule::RepeatsAndCommas, "plugin find", "--meta", COMMAS),
    Allow::new(Rule::RowsWhole, "ls", WHOLE),
    Allow::new(Rule::RowsWhole, "check", WHOLE),
    Allow::new(Rule::RowsWhole, "query", WHOLE),
    Allow::new(Rule::RowsWhole, "plugin list", WHOLE),
    Allow::new(Rule::RowsWhole, "plugin find", WHOLE),
];

/// Why a root command is one word.
const ROOT: &str = "board's root commands are the verbs an agent types most, each acting on the board or a record, and a resource word before them would lengthen every call";

/// Board's single-word root commands.
const ROOT_VERBS: &[&str] = &[
    "init", "check", "get", "ls", "refs", "uri", "put", "rm", "sync", "call", "decide", "answer",
    "query", "serve",
];

/// The verbs below a resource that crumbkit's own list does not hold.
const VERBS: &[&str] = &[
    "update", "find", "enable", "add", "kinds", "template", "open", "install",
];

/// Why an option takes repeats but not commas.
const COMMAS: &str = "a value may hold a comma, so one repeat carries one value";

/// Why a listing prints its rows whole.
const WHOLE: &str = "an agent that asks for --json gets every field without needing a second flag";

/// The suite that `conventions!` expands. The checks that need a scratch
/// board or an output directory run from the tests that build one.
fn suite() -> Suite<'static> {
    let binary: &'static str = binary().leak();
    Suite::new(|| (Cli::command(), crate::render::TOOL))
        .allow(ALLOW)
        .verbs(VERBS)
        .budgets(BUDGETS)
        .examples(Examples::new(binary, ""))
        .skip(Check::Help, "board check and board refs print rows that the help rules' column and Bytes-field checks do not match, and the call example in the help is not adopted")
        .skip(Check::PathWeights, "no per-path help weight budget is declared; every_help_page_fits_its_size_budget checks each page against BUDGETS in help.rs")
        .skip(Check::HelpGoldens, "the text of a -h page is not pinned by a golden file; the size budgets limit only its bytes and docs/commands.md is rendered from `commands list`")
        .skip(Check::SchemaGoldens, "no golden file pins the schemas; released.json lists none until a release, and one schema's text is pinned by the example golden for `schemas show board/check-report/v2`")
        .skip(Check::OutputBudget, "not adopted: at half its printed bytes, `board check --json` and `board plugin find --meta category=tasks --json` still print their whole result and exit 0")
        .skip(Check::Examples, "the examples test in help.rs runs the examples on a fixture board that it builds and removes")
        .skip(Check::CommandReference, "the command reference test in describe.rs runs it with the binary on a scratch board")
        .skip(Check::Recoveries, "the recoveries test in recoveries.rs runs it with its scratch scenarios and asserts on them afterwards")
        .skip(Check::Twins, "the twins test in recoveries.rs runs it with its scratch scenarios")
}

crumbkit_test::conventions!(suite());
