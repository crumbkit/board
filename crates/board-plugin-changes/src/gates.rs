//! Each phase's gate, computed only from the change and the records it
//! references. The gate interprets the checks the change's flow lists for
//! its phase (`flow.rs`), each by its kind.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use board_plugin::shape::Open;
use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::change::Referenced;
use crate::flow::{self, Field, Kind as CheckKind, Part, Scope};
use crate::records::{ChangeRecord, Kind, Phase, Subject, TASK_TYPE};

pub(crate) const RUN_TYPE: &str = "runs/run/v1";
pub(crate) const DECISION_TYPE: &str = "plans/decision/v1";
pub(crate) const REPORT_TYPE: &str = "review/report/v1";
pub(crate) const UNDERSTANDING_TYPE: &str = "changes/understanding/v1";
/// The plan's sections the plan gate requires, by heading, compared
/// without case.
pub(crate) const SECTIONS: [&str; 5] = ["Problem", "Outcome", "Out of scope", "UX", "Data"];
/// What a check that needs a head commit says when the change has none.
const NO_HEAD: &str = "the change records no head commit; attach one with --head";
/// The fewest hex digits a commit is named by when compared with another.
const SHORT_COMMIT: usize = 7;
/// Lines that open or close a Markdown code fence.
const FENCES: [&str; 2] = ["```", "~~~"];

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("each pattern is a constant; the gate tests compile it")
}

/// How a UX section says it lists no mockups, and a Data section that it
/// changes no data, at its start, without case.
static NO_UI: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^no ui change\b[\s.:,;-]*\w"));
static NO_DATA: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^none\s*(?:[.:]|\n?\z)"));
/// A mockup an item of the UX section's Mockups list names: its first URL,
/// or else its first board path in inline code.
static MOCKUP_URL: LazyLock<Regex> = LazyLock::new(|| pattern(r"https?://[^\s)>\]]+"));
static MOCKUP_PATH: LazyLock<Regex> = LazyLock::new(|| pattern(r"`(/[^`\s]+)`"));
static LIST_ITEM: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?m)^\s*(?:[-*+]|\d+[.)])\s+(\S.*)$"));
pub(crate) static HEADING: LazyLock<Regex> = LazyLock::new(|| pattern(r"^#{1,6}\s+(.*?)\s*#*\s*$"));

/// One requirement of a gate: `pass`, or `missing` with what is missing.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[expect(
    clippy::struct_field_names,
    reason = "a gate's output names each requirement `check`, as the Python plugin did"
)]
pub(crate) struct Check {
    pub(crate) check: &'static str,
    pub(crate) status: Status,
    pub(crate) detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Pass,
    Missing,
}

/// What a gate reads: the change, its records, the plan's text and the
/// suite.
///
/// A referenced record Board marks degraded is left out of `records`, so
/// it satisfies nothing. `plan` is none when no file is at the plan's
/// path. `suite` is the repository's check suite, or none when its
/// contributing docs name none; only the build gate reads it. The review
/// gate reads the rest: `planned`, the paths the change's plan declares;
/// `outside`, each file the change's diff edits that no planned path
/// matches; and `diff_unread`, why the diff could not be read.
#[derive(Debug)]
pub(crate) struct Observed {
    pub(crate) path: String,
    pub(crate) change: ChangeRecord,
    pub(crate) kind: Kind,
    /// The records attached to the change's plans, in order.
    pub(crate) cited: Vec<String>,
    pub(crate) records: Referenced,
    pub(crate) plan: Option<String>,
    /// The Markdown file the change's plan record names as its text.
    pub(crate) plan_file: Option<String>,
    pub(crate) suite: Option<String>,
    pub(crate) planned: Vec<String>,
    pub(crate) outside: Vec<String>,
    pub(crate) diff_unread: Option<String>,
    /// Whether the change's newest pull request reads as a github/pull/v1
    /// record, which the land gate needs.
    pub(crate) pull_read: bool,
}

impl Observed {
    pub(crate) fn new(path: &str, change: ChangeRecord, kind: Kind, records: Referenced) -> Self {
        Self {
            path: path.to_owned(),
            change,
            kind,
            cited: Vec::new(),
            records,
            plan: None,
            plan_file: None,
            suite: None,
            planned: Vec::new(),
            outside: Vec::new(),
            diff_unread: None,
            pull_read: false,
        }
    }
}

/// The refusal `advance` gives when a check with its own code is the first
/// that is missing; any other is `changes::gate::unmet`.
pub(crate) fn code_of(check: &str) -> &'static str {
    match check {
        "verdict" => "changes::prototype::never_lands",
        "ux" => "changes::ux::not_agreed",
        "neighbours" => "changes::ux::no_neighbours",
        "data" => "changes::data::not_signed",
        "paths" => "changes::paths::outside_plan",
        _ => "changes::gate::unmet",
    }
}

/// The text under each heading of a Markdown document, by heading in lower
/// case, trimmed.
///
/// A section runs to the next heading of the same or a higher level, so it
/// holds its subsections. A line inside a code fence is never a heading.
pub(crate) fn sections(text: &str) -> BTreeMap<String, String> {
    let mut found: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut opened: Vec<(usize, String)> = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let heading = if fenced {
            None
        } else {
            HEADING.captures(line).and_then(|captures| captures.get(1))
        };
        if FENCES
            .iter()
            .any(|fence| line.trim_start().starts_with(fence))
        {
            fenced = !fenced;
        }
        let level = line.len() - line.trim_start_matches('#').len();
        if heading.is_some() {
            while opened.last().is_some_and(|(open, _)| *open >= level) {
                opened.pop();
            }
        }
        for (_, open) in &opened {
            found.entry(open.clone()).or_default().push(line);
        }
        if let Some(heading) = heading {
            let name = heading.as_str().to_lowercase();
            found.entry(name.clone()).or_default();
            opened.push((level, name));
        }
    }
    (found.into_iter())
        .map(|(name, lines)| (name, lines.join("\n").trim().to_owned()))
        .collect()
}

fn check(name: &'static str, missing: Option<String>) -> Check {
    match missing {
        None => Check {
            check: name,
            status: Status::Pass,
            detail: String::new(),
        },
        Some(detail) => Check {
            check: name,
            status: Status::Missing,
            detail,
        },
    }
}

/// Something the gate of a phase reads beyond the change: the records the
/// change references, the plan's text, the repository's check suite, the
/// change's diff, or whether its newest pull request reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Read {
    References,
    Plan,
    Suite,
    Diff,
    Pull,
}

/// What the checks of `phase` in the development flow read, by their
/// kinds. A phase outside the flow reads nothing.
pub(crate) fn reads(phase: Phase) -> BTreeSet<Read> {
    let mut reads = BTreeSet::new();
    let checks = flow::development()
        .phase(phase.as_str())
        .map(|found| &found.checks);
    for each in checks.into_iter().flatten() {
        if (each.when.as_ref())
            .is_some_and(|when| when.section.is_some() || when.section_not.is_some())
        {
            reads.insert(Read::Plan);
        }
        let Some(kind) = each.kind.known() else {
            continue;
        };
        let refs = (each.conditions.as_ref()).is_some_and(|rules| rules.refs.is_some());
        let needs: &[Read] = match kind {
            CheckKind::Sections => &[Read::Plan],
            CheckKind::Mockups => &[Read::Plan, Read::References],
            CheckKind::Understood if each.rechecks.is_none() => &[Read::Plan, Read::References],
            CheckKind::Records if refs => &[Read::Plan, Read::References],
            CheckKind::Understood | CheckKind::Records | CheckKind::Tasks => &[Read::References],
            CheckKind::Run => &[Read::Suite, Read::References],
            CheckKind::Paths => &[Read::Diff],
            CheckKind::Field if each.field == Some(Field::Pulls) => &[Read::Pull],
            _ => &[],
        };
        reads.extend(needs.iter().copied());
    }
    reads
}

/// The checks for leaving the change's phase: those the development flow
/// lists for it whose `when` holds, in its order. `done`, `dropped` and
/// `paused` are no phase of the flow and have none. A prototype in build
/// has only its verdict check.
pub(crate) fn gate(observed: &Observed) -> Vec<Check> {
    let phase = observed.change.phase;
    if observed.kind == Kind::Prototype && phase == Phase::Build {
        return vec![check("verdict", Some(verdict_missing(observed)))];
    }
    let Some(found) = flow::development().phase(phase.as_str()) else {
        return Vec::new();
    };
    interpret(observed, &found.checks)
}

/// Each of `checks` whose `when` holds, evaluated by its kind, in order.
pub(crate) fn interpret(observed: &Observed, checks: &'static [flow::Check]) -> Vec<Check> {
    let plan = sections(observed.plan.as_deref().unwrap_or_default());
    (checks.iter())
        .filter_map(|each| match applies(observed, &plan, each) {
            Ok(false) => None,
            Ok(true) => Some(check(&each.id, evaluate(observed, &plan, each))),
            Err(detail) => Some(check(&each.id, Some(detail))),
        })
        .collect()
}

/// Whether every condition of the check's `when` holds, or why a pattern
/// in it cannot be read.
fn applies(
    observed: &Observed,
    plan: &BTreeMap<String, String>,
    each: &flow::Check,
) -> Result<bool, String> {
    let Some(when) = &each.when else {
        return Ok(true);
    };
    if let Some(routes) = &when.route
        && !(observed.change.route).is_some_and(|route| routes.contains(&route))
    {
        return Ok(false);
    }
    for (heading, text) in when.section.iter().flatten() {
        if !matches_section(plan, heading, text)? {
            return Ok(false);
        }
    }
    for (heading, text) in when.section_not.iter().flatten() {
        if matches_section(plan, heading, text)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether the text under the plan's `heading` matches `text`, a pattern,
/// without case.
fn matches_section(
    plan: &BTreeMap<String, String>,
    heading: &str,
    text: &str,
) -> Result<bool, String> {
    let compiled = Regex::new(&format!("(?i){text}")).map_err(|error| {
        format!(
            "the flow's pattern {text} for the {heading} section is not a regular expression \
             ({error}); correct the pattern in the flow"
        )
    })?;
    Ok(compiled.is_match(plan.get(&heading.to_lowercase()).map_or("", String::as_str)))
}

/// What is missing for the check, by its kind, or none when it passes.
///
/// A kind this Board does not evaluate yet, and a kind it does not know,
/// stay missing, so they refuse `advance` rather than let it pass unread.
fn evaluate(
    observed: &Observed,
    plan: &BTreeMap<String, String>,
    each: &flow::Check,
) -> Option<String> {
    let kind = match &each.kind {
        Open::Known(kind) => *kind,
        Open::Other(kind) => {
            return Some(format!(
                "the flow's check {} has the kind {kind}, which this Board does not know, so it \
                 cannot pass; update Board or change the check in the flow",
                each.id
            ));
        }
    };
    match kind {
        CheckKind::Sections => sections_missing(observed, plan, each),
        CheckKind::Field => kinds::field_missing(observed, each),
        CheckKind::Records => kinds::records_missing(observed, each),
        CheckKind::Run => run_missing(observed, each.scope.unwrap_or(Scope::Full)),
        CheckKind::Paths => paths_missing(observed),
        CheckKind::Tasks => tasks_missing(observed),
        CheckKind::Understood => understood_missing(observed, plan, each),
        CheckKind::Mockups => match each.part {
            Some(Part::Agreed) => ux_missing(observed, plan),
            Some(Part::Neighbours) => neighbours_missing(plan),
            None => ux_missing(observed, plan).or_else(|| neighbours_missing(plan)),
        },
        CheckKind::Considered
        | CheckKind::Obligations
        | CheckKind::ControlledPaths
        | CheckKind::FlowValid
        | CheckKind::Exists => Some(format!(
            "this Board does not evaluate {} checks yet, so the flow's check {} cannot pass; ask \
             the owner whether to take it out of the flow",
            kind.as_str(),
            each.id
        )),
    }
}

/// What a check says when the flow leaves out a parameter its kind needs.
fn unset(each: &flow::Check, parameter: &str) -> String {
    format!(
        "the flow's check {} names no {parameter}, so it cannot pass; add {parameter} to the \
         check in the flow",
        each.id
    )
}

fn sections_missing(
    observed: &Observed,
    plan: &BTreeMap<String, String>,
    each: &flow::Check,
) -> Option<String> {
    let Some(wanted) = &each.sections else {
        return Some(unset(each, "sections"));
    };
    let Some(path) = &observed.change.plan else {
        return Some("the change names no plan record".to_owned());
    };
    if observed.plan.is_none() {
        return Some(format!("no plan record and its text are at {path}"));
    }
    let empty: Vec<&str> = (wanted.iter().map(String::as_str))
        .filter(|section| {
            plan.get(&section.to_lowercase())
                .is_none_or(String::is_empty)
        })
        .collect();
    (!empty.is_empty())
        .then(|| format!("the plan at {path} has no text under: {}", empty.join(", ")))
}

fn task_missing(observed: &Observed) -> Option<String> {
    observed.change.task.as_ref().map_or_else(
        || {
            Some(format!(
                "the change names no task; add one with `board call tasks.add` and set it with \
                 `board call changes.attach --path {} --task <path>`",
                observed.path
            ))
        },
        |_| None,
    )
}

fn verdict_missing(observed: &Observed) -> String {
    format!(
        "a prototype never advances or lands; when it has answered its question, end it with \
         `board call changes.verdict --path {} --state settled --verdict '<one sentence>'`",
        observed.path
    )
}

/// The list items under the `heading` subsection of the UX section, or
/// none when UX holds no such heading.
fn under_ux(found: &BTreeMap<String, String>, heading: &str) -> Vec<String> {
    let named = pattern(&format!(r"(?im)^#{{1,6}}\s+{heading}\s*$"));
    if !named.is_match(found.get("ux").map_or("", String::as_str)) {
        return Vec::new();
    }
    let text = found.get(heading).map_or("", String::as_str);
    (LIST_ITEM.captures_iter(text))
        .filter_map(|captures| Some(captures.get(1)?.as_str().trim().to_owned()))
        .collect()
}

/// The artifact a Mockups item names: its first URL, or else its first
/// board path in inline code.
fn named_in(item: &str) -> Option<String> {
    if let Some(url) = MOCKUP_URL.find(item) {
        return Some(
            url.as_str()
                .trim_end_matches(['.', ',', ';', ':'])
                .to_owned(),
        );
    }
    let path = MOCKUP_PATH.captures(item)?.get(1)?;
    Some(path.as_str().to_owned())
}

fn mockups(found: &BTreeMap<String, String>) -> Vec<String> {
    let mut named = Vec::new();
    for item in under_ux(found, "mockups") {
        if let Some(name) = named_in(&item)
            && !named.contains(&name)
        {
            named.push(name);
        }
    }
    named
}

/// The attached decisions the owner signed; a stub a hook wrote names
/// itself in `written_by`.
fn signed_decisions(observed: &Observed) -> impl Iterator<Item = &Map<String, Value>> {
    (observed.records.of_type(DECISION_TYPE))
        .map(|(_, body)| body)
        .filter(|body| !truthy(body.get("written_by")))
}

/// Whether a value is set as Python reads it: not null, false, zero or
/// empty.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(fields)) => !fields.is_empty(),
    }
}

/// The strings of the list at `key`, ignoring anything else.
fn strings<'a>(body: &'a Map<String, Value>, key: &str) -> impl Iterator<Item = &'a str> {
    (body
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(Value::as_str)
}

fn named_by_decisions(observed: &Observed) -> BTreeSet<&str> {
    (signed_decisions(observed))
        .flat_map(|body| strings(body, "urls").chain(strings(body, "refs")))
        .collect()
}

fn ux_missing(observed: &Observed, found: &BTreeMap<String, String>) -> Option<String> {
    if NO_UI.is_match(found.get("ux").map_or("", String::as_str)) {
        return None;
    }
    let items = under_ux(found, "mockups");
    if items.is_empty() {
        return Some(
            "the UX section neither starts with No UI change and a reason nor lists mockups under \
             a ### Mockups heading"
                .to_owned(),
        );
    }
    if let Some(unnamed) = items.iter().find(|item| named_in(item).is_none()) {
        return Some(format!(
            "this item of the Mockups list names no artifact by URL or board path: {unnamed}"
        ));
    }
    let agreed = named_by_decisions(observed);
    let unagreed: Vec<String> = (mockups(found).into_iter())
        .filter(|mockup| !agreed.contains(mockup.as_str()))
        .collect();
    (!unagreed.is_empty()).then(|| {
        format!(
            "these mockups have no attached plans/decision/v1 that names them in its urls or \
             refs: {}",
            unagreed.join(", ")
        )
    })
}

fn neighbours_missing(found: &BTreeMap<String, String>) -> Option<String> {
    if under_ux(found, "mockups").is_empty() || !under_ux(found, "neighbours").is_empty() {
        return None;
    }
    Some(
        "the UX section lists mockups but has no ### Neighbours heading under it with a list: \
         name the open changes and artifacts in the same area, and the components and flows the \
         design reuses"
            .to_owned(),
    )
}

/// Whether an understanding check at `body` is of this change, and of
/// `subject` when one is given.
fn understands(observed: &Observed, body: &Map<String, Value>, subject: Option<Subject>) -> bool {
    body.get("change").and_then(Value::as_str) == Some(observed.path.as_str())
        && subject.is_none_or(|subject| {
            body.get("subject").and_then(Value::as_str) == Some(subject_word(subject))
        })
}

fn subject_word(subject: Subject) -> &'static str {
    match subject {
        Subject::Data => "data",
        Subject::Ux => "ux",
    }
}

/// A count as a word, for the small counts a flow sets.
fn count_word(count: usize) -> String {
    match count {
        1 => "one".to_owned(),
        2 => "two".to_owned(),
        3 => "three".to_owned(),
        _ => count.to_string(),
    }
}

/// With `rechecks`, each attached understanding check of the change holds
/// that many matching rechecks; without, the plan's section for the
/// subject says it changes nothing, or the owner signed a decision that
/// references an attached understanding check of it.
fn understood_missing(
    observed: &Observed,
    plan: &BTreeMap<String, String>,
    each: &flow::Check,
) -> Option<String> {
    if let Some(wanted) = each.rechecks {
        return recheck_missing(observed, each.subject, wanted as usize);
    }
    let Some(subject) = each.subject else {
        return Some(unset(each, "subject"));
    };
    let (heading, nothing, said, noun) = match subject {
        Subject::Data => ("data", &NO_DATA, "the Data section is not None", "data"),
        Subject::Ux => (
            "ux",
            &NO_UI,
            "the UX section does not say No UI change",
            "UX",
        ),
    };
    if nothing.is_match(plan.get(heading).map_or("", String::as_str)) {
        return None;
    }
    let understood: BTreeSet<&str> = (observed.records.of_type(UNDERSTANDING_TYPE))
        .filter(|(_, body)| understands(observed, body, Some(subject)))
        .map(|(path, _)| path)
        .collect();
    let signed = (signed_decisions(observed))
        .any(|body| strings(body, "refs").any(|each| understood.contains(each)));
    (!signed).then(|| {
        format!(
            "{said}, and no attached plans/decision/v1 references an attached \
             {UNDERSTANDING_TYPE} of this change's {noun}; run the understanding-check skill, \
             then have the owner sign a decision whose refs name its record"
        )
    })
}

fn recheck_missing(observed: &Observed, subject: Option<Subject>, wanted: usize) -> Option<String> {
    // An understanding check Board does not read holds no rechecks it can
    // count, so it fails the check rather than drop out of it.
    for (path, code) in &observed.records.unreadable {
        if observed
            .records
            .unreadable_types
            .get(path)
            .map(String::as_str)
            == Some(UNDERSTANDING_TYPE)
        {
            return Some(format!(
                "the understanding check at {path} is degraded with {code}, so its rechecks \
                 cannot be counted; run `board check {path}` and fix it"
            ));
        }
    }
    for (path, body) in observed.records.of_type(UNDERSTANDING_TYPE) {
        let matching = (body
            .get("rechecks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten())
        .filter(|recheck| recheck.get("matches") == Some(&Value::Bool(true)))
        .count();
        if understands(observed, body, subject) && matching < wanted {
            let wanted = count_word(wanted);
            return Some(format!(
                "the understanding check at {path} holds fewer than {wanted} rechecks that match \
                 the plan; before review, ask {wanted} of its predict or edge questions again, \
                 worded differently, and add them to its rechecks"
            ));
        }
    }
    None
}

fn route_missing(observed: &Observed) -> Option<String> {
    observed.change.route.is_none().then(|| {
        format!(
            "the change records no route; set one with `board call changes.attach --path {} \
             --route <route>`",
            observed.path
        )
    })
}

/// Whether a task's `status` leaves it open.
pub(crate) fn open_task(body: &Map<String, Value>) -> bool {
    let status = body.get("status").and_then(Value::as_str);
    !matches!(status, Some("done" | "dropped"))
}

fn tasks_missing(observed: &Observed) -> Option<String> {
    let left: Vec<&str> = (observed.records.of_type(TASK_TYPE))
        .filter(|(_, body)| open_task(body))
        .map(|(path, _)| path)
        .collect();
    (!left.is_empty()).then(|| format!("these tasks are not done or dropped: {}", left.join(", ")))
}

/// Whether `one` names the commit `other` names, either possibly
/// abbreviated.
pub(crate) fn same_commit(one: Option<&Value>, other: &str) -> bool {
    let Some(one) = one.and_then(Value::as_str) else {
        return false;
    };
    if one.chars().count().min(other.chars().count()) < SHORT_COMMIT {
        return false;
    }
    one.starts_with(other) || other.starts_with(one)
}

/// Whether any of a run's `versions` names `head`.
pub(crate) fn ran_on(run: &Map<String, Value>, head: &str) -> bool {
    (run.get("versions")
        .and_then(Value::as_object)
        .into_iter()
        .flatten())
    .any(|(_, version)| same_commit(Some(version), head))
}

/// The objects of the list at `key`.
pub(crate) fn objects<'a>(
    body: &'a Map<String, Value>,
    key: &str,
) -> impl Iterator<Item = &'a Map<String, Value>> {
    (body
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(Value::as_object)
}

fn run_missing(observed: &Observed, scope: Scope) -> Option<String> {
    let Some(suite) = &observed.suite else {
        return Some(format!(
            "no contributing doc in {} names a runs/suite/v1 path as its check suite",
            observed.change.mount
        ));
    };
    let Some(head) = &observed.change.head else {
        return Some(NO_HEAD.to_owned());
    };
    let runs = format!("{suite}/runs/");
    let passed = (observed.records.of_type(RUN_TYPE)).any(|(path, body)| {
        path.starts_with(&runs)
            && body.get("scope").and_then(Value::as_str) == Some(scope.as_str())
            && body.get("dirty") == Some(&Value::Bool(false))
            && ran_on(body, head)
            && !objects(body, "results").any(|result| {
                matches!(
                    result.get("outcome").and_then(Value::as_str),
                    Some("fail" | "error")
                )
            })
    });
    (!passed).then(|| format!("no attached run of {suite} on {head} is full, clean and passing"))
}

fn paths_missing(observed: &Observed) -> Option<String> {
    if observed.planned.is_empty() {
        return Some(
            "the change's plan declares no paths, so every edited file is outside them; write the \
             plan again with `board call changes.plan` and its paths"
                .to_owned(),
        );
    }
    if let Some(unread) = &observed.diff_unread {
        return Some(unread.clone());
    }
    (!observed.outside.is_empty()).then(|| {
        format!(
            "the change's diff edits files outside the plan's paths, so their rules were not in \
             the brief: {}; add them to the paths with `board call changes.plan`, or revert the \
             edit",
            observed.outside.join(", ")
        )
    })
}

/// The head commit a report covers: the right side of its `base..head`
/// range.
fn reviewed_head(report: &Map<String, Value>) -> Option<Value> {
    let covered = report.get("range")?.as_str()?;
    let (_, head) = covered.rsplit_once("..")?;
    Some(Value::from(head.trim_start_matches('.')))
}

mod kinds;

#[cfg(test)]
mod tests;
