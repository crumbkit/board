//! What to do when a check is missing: the flow's `remedy` for it, or else
//! its kind's own, with the placeholders filled.
//!
//! A flow's instructions and remedies may name `{change}`, `{name}`,
//! `{mount}`, `{repo}`, `{plan}`, `{head}`, `{suite}`, `{draft}` and
//! `{board}`. A placeholder with no value, such as `{head}` before a head
//! is recorded, is left out: the text loses it, an inline code span left
//! empty goes with it, runs of spaces become one, and no space is left
//! before punctuation. Any other text in braces stays as it is.

use std::sync::LazyLock;

use board_plugin::shape::Record;
use regex::Regex;

use super::{Observed, pattern};
use crate::change::name_in;
use crate::flow::{self, Actor, Field, Kind};

/// A placeholder: a lowercase word in braces.
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| pattern(r"\{([a-z]+)\}"));
/// What leaving a placeholder out can leave behind: an empty code span, a
/// run of spaces, or a space before punctuation.
static EMPTY_CODE: LazyLock<Regex> = LazyLock::new(|| pattern(r"``"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| pattern(r" {2,}"));
static BEFORE_PUNCTUATION: LazyLock<Regex> = LazyLock::new(|| pattern(r" ([.,;:)])"));

/// Each placeholder and its value, none when it has none.
pub(crate) type Values = [(&'static str, Option<String>); 9];

/// The placeholders' values for the change `observed` reads.
///
/// `{draft}` is the name of the first attached `changes/flow/v1`, a flow
/// the change drafts, and `{board}` the board's root.
pub(crate) fn values(observed: &Observed) -> Values {
    let change = &observed.change;
    let draft = (observed.records.of_type(flow::Flow::TYPE))
        .find_map(|(_, body)| Some(body.get("name")?.as_str()?.to_owned()));
    [
        ("change", Some(observed.path.clone())),
        ("name", Some(name_in(&observed.path).to_owned())),
        ("mount", Some(change.mount.clone())),
        (
            "repo",
            Some(name_in(&change.mount).to_owned()).filter(|repo| !repo.is_empty()),
        ),
        ("plan", change.plan.clone()),
        ("head", change.head.clone()),
        ("suite", observed.suite.clone()),
        ("draft", draft),
        ("board", observed.board.clone()),
    ]
}

/// `text` with each placeholder replaced by its value, and each with no
/// value left out cleanly.
pub(crate) fn fill(text: &str, values: &Values) -> String {
    let mut left_out = false;
    let filled = PLACEHOLDER.replace_all(text, |captures: &regex::Captures<'_>| {
        let word = &captures[1];
        match values.iter().find(|(name, _)| *name == word) {
            Some((_, Some(value))) => value.clone(),
            Some((_, None)) => {
                left_out = true;
                String::new()
            }
            None => captures[0].to_owned(),
        }
    });
    if !left_out {
        return filled.into_owned();
    }
    let tidied = EMPTY_CODE.replace_all(&filled, "");
    let tidied = SPACES.replace_all(&tidied, " ");
    BEFORE_PUNCTUATION
        .replace_all(&tidied, "$1")
        .trim()
        .to_owned()
}

/// What to do when `each` is missing: the flow's remedy, or else its kind's.
pub(crate) fn of(each: &flow::Check, values: &Values) -> String {
    let text = each.remedy.clone().unwrap_or_else(|| default(each));
    let mut said = fill(&text, values);
    if each.waivable == Some(true) {
        let waiver = format!(
            " The owner may waive this check with a plans/decision/v1 he signs whose refs name \
             {{change}}#{}, attached with `board call changes.attach --path {{change}} --ref \
             <decision>`.",
            each.id
        );
        said.push_str(&fill(&waiver, values));
    }
    said
}

/// What to attach a record with.
const ATTACH: &str = "attach it with `board call changes.attach --path {change} --ref <path>`";

/// A kind's own remedy for `each`.
fn default(each: &flow::Check) -> String {
    let Some(kind) = each.kind.known() else {
        return "Update Board, or ask the owner whether to take the check out of the flow."
            .to_owned();
    };
    let ask = match each.actor {
        Some(Actor::Owner) => "Only the owner can provide this: ask him, then ",
        Some(Actor::Reviewer) => {
            "Only a reviewer can provide this: start one with `board call review.run`, then "
        }
        Some(Actor::Agent) | None => "",
    };
    let said = match kind {
        Kind::Sections => {
            "Write text under each heading the detail names and write the plan again with `board \
             call changes.plan --path {change}`."
                .to_owned()
        }
        Kind::Field => field(each.field),
        Kind::Records => format!("{ask}write the record the detail describes and {ATTACH}."),
        Kind::Run => {
            "Run {suite} on {head} with `board call runs.run` and attach the run with `board call \
             changes.attach --path {change} --ref <run>`. When the change names no suite, name \
             one with `board call changes.attach --path {change} --suite /suites/<name>`."
                .to_owned()
        }
        Kind::Paths => {
            "Add the files to the plan's paths with `board call changes.plan --path {change}`, or \
             revert the edits."
                .to_owned()
        }
        Kind::Tasks => {
            "Finish each open task and close it with `board call tasks.done`, or drop it with \
             `board call tasks.drop`."
                .to_owned()
        }
        Kind::Understood if each.rechecks.is_some() => format!(
            "{ask}ask the owner again, worded differently, the predict or edge questions of each \
             understanding check the detail names, and add them to its rechecks."
        ),
        Kind::Understood => format!(
            "{ask}run the understanding-check skill with the owner, {ATTACH}, and ask the owner to \
             sign a plans/decision/v1 whose refs name it."
        ),
        Kind::Mockups => format!(
            "{ask}list each mockup by URL or board path under ### Mockups in UX, with a ### \
             Neighbours list, and attach the owner's plans/decision/v1 that names each in its \
             urls or refs; or start UX with No UI change and a reason."
        ),
        Kind::Considered => {
            "Cite each surfaced record you build on in the plan's uses, list the rest under ### \
             Considered with one line on why it does not apply, and write the plan again with \
             `board call changes.plan --path {change}`."
                .to_owned()
        }
        Kind::Obligations => {
            "Do the work each obligation names. In the commit that does it, delete the obligation \
             with `board rm <path>` and name the work in the commit message. List an obligation \
             the change leaves owed under the plan's escape heading."
                .to_owned()
        }
        Kind::ControlledPaths => {
            "Revert the edits to the controlled paths, or move the change to a flow that allows \
             them with `board call changes.reflow --path {change} --flow <flow>`."
                .to_owned()
        }
        Kind::FlowValid | Kind::Exists => {
            "This Board does not evaluate the check yet; ask the owner whether to take it out of \
             the flow."
                .to_owned()
        }
    };
    capitalised(&said)
}

/// A `field` check's remedy for the field it reads.
fn field(field: Option<Field>) -> String {
    match field {
        Some(Field::Task) => {
            "Add a task with `board call tasks.add` and set it with `board call changes.attach \
             --path {change} --task <path>`."
        }
        Some(Field::Route) => {
            "Set the route with `board call changes.attach --path {change} --route <route>`."
        }
        Some(Field::Head) => {
            "Commit, then record the head with `board call changes.attach --path {change} --head \
             <sha>`."
        }
        Some(Field::Plan) => "Write the plan with `board call changes.plan --path {change}`.",
        Some(Field::Tickets) => {
            "Record the ticket with `board call jira.ref` or `board call github.ref` and attach it \
             with `board call changes.attach --path {change} --ticket <path>`."
        }
        Some(Field::Pulls) => {
            "Draft the pull request with `board call changes.pr --path {change}`, open it, record \
             it with `board call github.ref` and attach it with `board call changes.attach --path \
             {change} --pull <path>`."
        }
        None => "Add a field to the check in the flow.",
    }
    .to_owned()
}

/// `text` with its first letter in upper case.
fn capitalised(text: &str) -> String {
    let mut letters = text.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

#[cfg(test)]
mod tests;
