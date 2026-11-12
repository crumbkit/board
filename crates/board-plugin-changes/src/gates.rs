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
use crate::flow::{self, Actor, Field, Kind as CheckKind, Part, Scope};
use crate::plan::answers::{Answers, Ux};
use crate::records::{BUILD, ChangeRecord, Kind, Subject, TASK_TYPE};

pub(crate) const RUN_TYPE: &str = "runs/run/v1";
pub(crate) const DECISION_TYPE: &str = "plans/decision/v1";
pub(crate) const REPORT_TYPE: &str = "review/report/v1";
pub(crate) const UNDERSTANDING_TYPE: &str = "changes/understanding/v1";
/// What a check that needs a head commit says when the change has none.
const NO_HEAD: &str = "the change records no head commit; attach one with --head";
/// The fewest hex digits a commit is named by when compared with another.
const SHORT_COMMIT: usize = 7;
/// Lines that open or close a Markdown code fence.
pub(crate) const FENCES: [&str; 2] = ["```", "~~~"];

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("each pattern is a constant; the gate tests compile it")
}

/// How a UX section says it lists no mockups, at its start, without case.
static NO_UI: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^no ui change\b[\s.:,;-]*\w"));
/// A mockup an item of the UX section's Mockups list names: its first URL,
/// or else its first board path in inline code.
static MOCKUP_URL: LazyLock<Regex> = LazyLock::new(|| pattern(r"https?://[^\s)>\]]+"));
static MOCKUP_PATH: LazyLock<Regex> = LazyLock::new(|| pattern(r"`(/[^`\s]+)`"));
static LIST_ITEM: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?m)^\s*(?:[-*+]|\d+[.)])\s+(\S.*)$"));
pub(crate) static HEADING: LazyLock<Regex> = LazyLock::new(|| pattern(r"^#{1,6}\s+(.*?)\s*#*\s*$"));

/// One check of a gate: `pass`, or `missing` with what is missing, who can
/// make it pass, whether it refuses `advance`, and, when missing, what to
/// do.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[expect(
    clippy::struct_field_names,
    reason = "a gate's output names each requirement `check`, as the Python plugin did"
)]
pub(crate) struct Check {
    pub(crate) check: String,
    pub(crate) status: Status,
    pub(crate) detail: String,
    pub(crate) actor: Actor,
    pub(crate) blocks: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remedy: Option<String>,
    /// The code `advance` refuses with when this is the first missing check
    /// that blocks.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) code: &'static str,
}

impl Check {
    /// Whether the check is missing and refuses `advance`.
    pub(crate) fn blocking(&self) -> bool {
        self.status == Status::Missing && self.blocks
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Pass,
    Missing,
}

/// What a gate reads: the change, its flow, its records, the plan and the
/// suite, the diff and the rules that bear on them. Every rule's path is in
/// its `/rules/` form.
///
/// A referenced record Board marks degraded is left out of `records`, so
/// it satisfies nothing. `plan` is none when no file is at the plan's
/// path, and `uses` lists what the plan relies on. `suite` is the change's
/// check suite: the one it stores, or for a v1 change the one its
/// repository's contributing docs name. `planned` is the paths the change's
/// plan declares, `edited` each file the change's diff edits, `outside`
/// those no planned path matches, and `diff_unread` why the diff could not
/// be read. `obligations` lists the obligations on the planned paths, none
/// when the rules plugin is not enabled; `controlled` maps each rule a
/// `rule-paths` check names to the edited files it lists, or none
/// when no such rule exists. `ticket_types` holds the type of each of
/// the change's tickets Board reads, `board` the board's root, and
/// `existing` whether something is at each path an `exists` check names,
/// with its placeholders filled.
#[derive(Debug)]
pub(crate) struct Observed {
    pub(crate) path: String,
    pub(crate) change: ChangeRecord,
    pub(crate) kind: Kind,
    pub(crate) flow: Option<flow::Flow>,
    /// The records attached to the change's plans, in order.
    pub(crate) cited: Vec<String>,
    pub(crate) records: Referenced,
    pub(crate) plan: Option<String>,
    /// The Markdown file the change's plan record names as its text.
    pub(crate) plan_file: Option<String>,
    pub(crate) uses: Vec<String>,
    /// The typed answers the plan record gives.
    pub(crate) answers: Answers,
    /// The records the plan's `considered` field lists.
    pub(crate) considered: Vec<String>,
    /// The change's surfaced paths that no longer reach anything.
    pub(crate) surfaced_gone: Vec<String>,
    /// The obligations the plan's `owed` field lists.
    pub(crate) owed: Vec<String>,
    pub(crate) suite: Option<String>,
    pub(crate) planned: Vec<String>,
    pub(crate) edited: Vec<String>,
    pub(crate) outside: Vec<String>,
    pub(crate) diff_unread: Option<String>,
    /// Whether the change's newest pull request reads as a github/pull/v1
    /// record, which the land gate needs.
    pub(crate) pull_read: bool,
    pub(crate) obligations: Vec<String>,
    pub(crate) controlled: BTreeMap<String, Option<Vec<String>>>,
    pub(crate) ticket_types: Vec<String>,
    pub(crate) board: Option<String>,
    pub(crate) existing: BTreeMap<String, bool>,
    /// The paths a check's `when.absent` names, filled, that hold the record
    /// it means, so the check does not apply.
    pub(crate) records_held: BTreeSet<String>,
    /// The rules `rules.for-paths` finds on the plan's paths, which a
    /// `planned-rule-paths` check reads.
    pub(crate) on_planned: Vec<String>,
    /// Why `rules.for-paths` could not be read for the plan's paths.
    pub(crate) on_planned_unread: Option<String>,
    /// Which rules a `planned-rule-paths` check names the board holds.
    pub(crate) held: BTreeSet<String>,
}

impl Observed {
    pub(crate) fn new(path: &str, change: ChangeRecord, kind: Kind, records: Referenced) -> Self {
        Self {
            path: path.to_owned(),
            change,
            kind,
            flow: None,
            cited: Vec::new(),
            records,
            plan: None,
            plan_file: None,
            uses: Vec::new(),
            answers: Answers::default(),
            considered: Vec::new(),
            surfaced_gone: Vec::new(),
            owed: Vec::new(),
            suite: None,
            planned: Vec::new(),
            edited: Vec::new(),
            outside: Vec::new(),
            diff_unread: None,
            pull_read: false,
            obligations: Vec::new(),
            controlled: BTreeMap::new(),
            ticket_types: Vec::new(),
            board: None,
            existing: BTreeMap::new(),
            records_held: BTreeSet::new(),
            on_planned: Vec::new(),
            on_planned_unread: None,
            held: BTreeSet::new(),
        }
    }

    /// The checks of the change's phase in its flow, none when its flow has
    /// no such phase.
    pub(crate) fn checks(&self) -> &[flow::Check] {
        (self.flow.as_ref())
            .and_then(|flow| flow.phase(&self.change.phase))
            .map_or(&[], |phase| phase.checks.as_slice())
    }
}

/// The refusal `advance` gives when `found` is the first missing check that
/// blocks and its kind has a code of its own; any other is
/// `changes::gate::unmet`.
fn code_of(
    observed: &Observed,
    plan: &BTreeMap<String, String>,
    each: &flow::Check,
) -> &'static str {
    match each.kind.known() {
        Some(CheckKind::Mockups) => match each.part {
            Some(Part::Agreed) => "changes::ux::not_agreed",
            None if ux_missing(observed, plan).is_some() => "changes::ux::not_agreed",
            Some(Part::Neighbours) | None => "changes::ux::no_neighbours",
        },
        Some(CheckKind::Understood)
            if each.rechecks.is_none() && each.subject == Some(Subject::Data) =>
        {
            "changes::data::not_signed"
        }
        Some(CheckKind::Paths) => "changes::paths::outside_plan",
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

/// Something the gate of a phase reads beyond the change: the records the
/// change references, the plan, the change's check suite, the change's
/// diff, whether its newest pull request reads, the obligations on its
/// paths, the rules a check names, its tickets' types, the paths an
/// `exists` check names, or the rules on the plan's paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Read {
    References,
    Plan,
    Suite,
    Diff,
    Pull,
    Obligations,
    Controlled,
    Tickets,
    Exists,
    Absent,
    PlannedRules,
}

/// What `checks` read, by their kinds. A waivable check reads the
/// references, where its waiver would be.
pub(crate) fn reads(checks: &[flow::Check]) -> BTreeSet<Read> {
    let mut reads = BTreeSet::new();
    for each in checks {
        if (each.when.as_ref()).is_some_and(|when| {
            when.section.is_some() || when.section_not.is_some() || when.answers.is_some()
        }) {
            reads.insert(Read::Plan);
        }
        if let Some(absent) = each.when.as_ref().and_then(|when| when.absent.as_ref()) {
            reads.insert(Read::Absent);
            reads.insert(Read::References);
            if absent.path.contains("{suite}") {
                reads.insert(Read::Suite);
            }
        }
        if each.waivable == Some(true) {
            reads.insert(Read::References);
        }
        let Some(kind) = each.kind.known() else {
            continue;
        };
        let refs = (each.conditions.as_ref()).is_some_and(|rules| rules.refs.is_some());
        let needs: &[Read] = match kind {
            CheckKind::Sections | CheckKind::Considered => &[Read::Plan],
            CheckKind::Mockups => &[Read::Plan, Read::References],
            CheckKind::Records if refs => &[Read::Plan, Read::References],
            CheckKind::Understood
            | CheckKind::Records
            | CheckKind::Tasks
            | CheckKind::FlowValid => &[Read::References],
            CheckKind::Exists => &[Read::References, Read::Exists],
            CheckKind::Run => &[Read::Suite, Read::References],
            CheckKind::Paths => &[Read::Diff],
            CheckKind::Obligations => &[Read::Plan, Read::Obligations],
            CheckKind::RulePaths => &[Read::Diff, Read::Controlled],
            CheckKind::PlannedRulePaths => &[Read::PlannedRules],
            CheckKind::Field if each.field == Some(Field::Pulls) => &[Read::Pull],
            CheckKind::Field if each.field == Some(Field::Tickets) && each.types.is_some() => {
                &[Read::Tickets]
            }
            CheckKind::Field => &[],
        };
        reads.extend(needs.iter().copied());
    }
    reads
}

/// The checks for leaving the change's phase: those its flow lists for it
/// whose `when` holds, in its order. `done`, `dropped` and `paused` are no
/// phase of a flow and have none. A prototype in build has only its
/// verdict check.
pub(crate) fn gate(observed: &Observed) -> Vec<Check> {
    if observed.kind == Kind::Prototype && observed.change.phase == BUILD {
        return vec![Check {
            check: "verdict".to_owned(),
            status: Status::Missing,
            detail: verdict_missing(observed),
            actor: Actor::Owner,
            blocks: true,
            remedy: None,
            code: "changes::prototype::never_lands",
        }];
    }
    interpret(observed, observed.checks())
}

/// Each of `checks` whose `when` holds, evaluated by its kind, in order.
pub(crate) fn interpret(observed: &Observed, checks: &[flow::Check]) -> Vec<Check> {
    let plan = sections(observed.plan.as_deref().unwrap_or_default());
    let values = remedy::values(observed);
    (checks.iter())
        .filter_map(|each| {
            // A check that reads an answer the plan does not give is the
            // agent's to clear, by writing the plan again.
            let left_out = (each.when.as_ref().and_then(|when| when.answers))
                .map_or_else(Vec::new, |wanted| observed.answers.left_out(wanted));
            let missing = match applies(observed, &plan, each) {
                Ok(false) => return None,
                Ok(true) if !left_out.is_empty() => Some(format!(
                    "the plan gives no typed answer for {}, which this check reads",
                    left_out.join(" and ")
                )),
                Ok(true) => evaluate(observed, &plan, each),
                Err(detail) => Some(detail),
            };
            let waiver = missing.as_ref().and_then(|_| waived_by(observed, each));
            let (status, detail) = match (missing, waiver) {
                (Some(_), Some(decision)) => {
                    (Status::Pass, format!("the owner waived it in {decision}"))
                }
                (Some(detail), None) => (Status::Missing, detail),
                (None, _) => (Status::Pass, kinds::passed(observed, each, &values)),
            };
            let (code, actor, said) = if left_out.is_empty() {
                let (code, said) = (code_of(observed, &plan, each), remedy::of(each, &values));
                (code, each.actor.unwrap_or(Actor::Agent), said)
            } else {
                let said = remedy::fill(remedy::ANSWER, &values);
                ("changes::plan::no_answers", Actor::Agent, said)
            };
            Some(Check {
                check: each.id.clone(),
                code,
                status,
                detail,
                actor,
                blocks: each.blocks.unwrap_or(true),
                remedy: (status == Status::Missing).then_some(said),
            })
        })
        .collect()
}

/// The attached decision the owner signed whose refs name
/// `<change>#<id>`, when the check is waivable.
fn waived_by<'a>(observed: &'a Observed, each: &flow::Check) -> Option<&'a str> {
    if each.waivable != Some(true) {
        return None;
    }
    let named = format!("{}#{}", observed.path, each.id);
    (observed.records.of_type(DECISION_TYPE))
        .filter(|(_, body)| !truthy(body.get("written_by")))
        .find(|(_, body)| strings(body, "refs").any(|each| each == named))
        .map(|(path, _)| path)
}

/// Whether every condition of the check's `when` holds, or why a pattern
/// in it cannot be read. An answer the plan leaves out counts as holding.
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
    if let Some(wanted) = when.answers
        && !observed.answers.meet(wanted)
    {
        return Ok(false);
    }
    if let Some(absent) = &when.absent
        && observed
            .records_held
            .contains(&remedy::fill(&absent.path, &remedy::values(observed)))
    {
        return Ok(false);
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
/// A kind this Board does not know stays missing, so it refuses `advance`
/// rather than let it pass unread.
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
        CheckKind::Understood => kinds::understood_missing(observed, each),
        CheckKind::Mockups => match each.part {
            Some(Part::Agreed) => ux_missing(observed, plan),
            Some(Part::Neighbours) => neighbours_missing(plan),
            None => ux_missing(observed, plan).or_else(|| neighbours_missing(plan)),
        },
        CheckKind::Considered => kinds::considered_missing(observed),
        CheckKind::Obligations => kinds::obligations_missing(observed),
        CheckKind::RulePaths => kinds::controlled_missing(observed, each),
        CheckKind::PlannedRulePaths => kinds::planned_controlled_missing(observed, each),
        CheckKind::FlowValid => kinds::flow_valid_missing(observed),
        CheckKind::Exists => kinds::exists_missing(observed, each),
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
                "the change names no task; add one with `board tasks add` and set it with \
                 `board changes attach --path {} --task <path>`",
                observed.path
            ))
        },
        |_| None,
    )
}

fn verdict_missing(observed: &Observed) -> String {
    format!(
        "a prototype never advances or lands; when it has answered its question, end it with \
         `board changes verdict --path {} --state settled --verdict '<one sentence>'`",
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
    // A plan whose typed answer is mockups lists them, whatever its text says.
    if observed.answers.ux != Some(Ux::Mockups)
        && NO_UI.is_match(found.get("ux").map_or("", String::as_str))
    {
        return None;
    }
    let items = under_ux(found, "mockups");
    if items.is_empty() {
        return Some(if observed.answers.ux == Some(Ux::Mockups) {
            "the plan answers `ux` mockups and its UX section lists none under a ### Mockups \
             heading"
        } else {
            "the UX section neither starts with No UI change and a reason nor lists mockups under \
             a ### Mockups heading"
        }
        .to_owned());
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

fn route_missing(observed: &Observed) -> Option<String> {
    observed.change.route.is_none().then(|| {
        format!(
            "the change records no route; set one with `board changes attach --path {} \
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

/// Every attached task is done or dropped, except the change's own, which
/// closes only when the change lands.
fn tasks_missing(observed: &Observed) -> Option<String> {
    let own = observed.change.task.as_deref();
    let left: Vec<&str> = (observed.records.of_type(TASK_TYPE))
        .filter(|(path, body)| Some(*path) != own && open_task(body))
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
        if !observed.change.v1 {
            return Some("the change names no check suite".to_owned());
        }
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
             plan again with `board changes plan` and its paths"
                .to_owned(),
        );
    }
    if let Some(unread) = &observed.diff_unread {
        return Some(unread.clone());
    }
    (!observed.outside.is_empty()).then(|| {
        format!(
            "the change's diff edits files outside the plan's paths, so their rules were not in \
             the brief: {}; add them to the paths with `board changes plan`, or revert the \
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

pub(crate) mod kinds;
pub(crate) mod remedy;

#[cfg(test)]
mod tests;
