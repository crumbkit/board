//! The `next`, `attach`, `advance` and `reflow` capabilities: where a
//! change stands, what it cites, moving it on, and moving it to another
//! flow. `moves` holds `pause`, `resume` and `reopen`.

use std::fmt::Write as _;

use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Prose;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board::{self, get_text, rows, text};
use crate::change::write;
use crate::change::{
    LastSync, attached, branch_of, current, kind_of, last_sync, line, plan_at, pull_of, read,
    referenced, sync_reads, touch,
};
use crate::controls::{listed_by, obligations};
use crate::diff::{edited, outside};
use crate::flow::Kind as CheckKind;
use crate::flow::{
    Actor, FlowSummary, asked_for, blob_at, copy_line, of_change, refuse_invalid,
    refuse_without_repository,
};
use crate::gates::kinds::named_controls;
use crate::gates::remedy::{fill, unfilled, values};
use crate::gates::{Check, Observed, Read, gate, reads};
use crate::records::{
    BOARD_ITSELF, BUILD, ChangeRecord, DONE, DROPPED, Kind, PAUSED, PULL_TYPES, Route, SUITE_TYPE,
    TASK_TYPE, TICKET_TYPES, is_final,
};
use crate::repo::check_suite;
use crate::stop::{Shown, holds, reached, stop_reached};
use crate::wip::{Limit, Move, active_days, admit};
use crate::{PLUGIN, refusal, to_value};

mod moves;

use moves::latest_reason;
pub(crate) use moves::{ReopenInput, pause, reopen, resume};

/// One change, by path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PathInput {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    pub(crate) path: String,
}

/// A change to move on, and why it may go past the WIP limit when that is
/// reached.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdvanceInput {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(
        with = "Option<Prose>",
        length(min = 1),
        description = "Why the change moves into build although the WIP limit is reached; \
                       recorded on the change."
    )]
    over_limit: Option<String>,
}

/// Add a record, ticket or pull reference, or set the task, route or head
/// commit.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("anyOf" = [
    {"required": ["ref"]}, {"required": ["ticket"]}, {"required": ["pull"]},
    {"required": ["task"]}, {"required": ["route"]}, {"required": ["head"]},
    {"required": ["suite"]},
]))]
pub(crate) struct AttachInput {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "A record the change produced, such as /suites/board-tests/runs/<id>; \
                       the plan the change names keeps it."
    )]
    reference: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "A ticket the change works on, a jira/issue/v1 or github/issue/v1 record."
    )]
    ticket: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "A pull request of the change, a github/pull/v1 record, which `board \
                       call github.ref --ref <owner>/<repo>#<number>` writes."
    )]
    pull: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The task the change does, a tasks/task/v2 record."
    )]
    task: Option<String>,
    #[schemars(description = "The route: owner, ask-first or unowned.")]
    route: Option<Route>,
    #[schemars(
        regex(pattern = "^[0-9a-f]{7,40}$"),
        description = "The commit under test and review."
    )]
    head: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The check suite the change's runs belong to, a runs/suite/v1 record, \
                       such as /suites/board-gates."
    )]
    suite: Option<String>,
}

/// Where the change stands: its flow and phase, route, the gate's checks,
/// what to do now and the last sync.
///
/// `flow` names the flow the change follows and its phases, null for a
/// prototype. Each check says who can make it pass (`actor`), whether it
/// refuses `advance` (`blocks`), and when missing what to do (`remedy`).
/// When every missing check that blocks waits on the owner or a reviewer,
/// `waiting_on` names them and `ask` is a question body built from their
/// remedies, which a session running without the owner records as a
/// plans/question/v1 whose `blocks` names the change. `stop` is where the
/// change's run stops, which `changes.run` set, and whether the change has
/// reached it; it is left out when no run set one.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct NextOutput {
    pub(crate) path: String,
    pub(crate) flow: Option<FlowSummary>,
    pub(crate) phase: String,
    pub(crate) route: Option<Route>,
    pub(crate) gate: Vec<Check>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) waiting_on: Vec<Actor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ask: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stop: Option<Shown>,
    pub(crate) instruction: String,
    pub(crate) sync: LastSync,
}

/// The change written, and how many records its plans now hold.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct AttachOutput {
    path: String,
    records: usize,
}

/// The phase the change left and the phase it entered, and the warning
/// when it went past the WIP limit.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Moved {
    path: String,
    from: String,
    to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<Limit>,
}

/// What to do in the change's phase: its flow's instruction for it, with
/// the placeholders filled, or a fixed one for a phase outside the flow.
fn instruction(observed: &Observed) -> String {
    let phase = observed.change.phase.as_str();
    match (observed.flow.as_ref()).and_then(|flow| flow.phase(phase)) {
        Some(found) => with_plan_usage(observed, fill(&found.instruction, &values(observed))),
        None if phase == PAUSED => format!(
            "This change was paused to free its place under the WIP limit on active changes of \
             its kind per repository. Resume it with `board call changes.resume --path {}` when a \
             place is free and the owner's queue allows.",
            observed.path
        ),
        None => "Nothing is left to do.".to_owned(),
    }
}

/// `said` with the fields `changes.plan` takes on stdin, when `said` asks
/// for a plan. `changes.plan` replaces the plan, so for a change that has
/// one, `said` says to resend the current text, uses and paths.
fn with_plan_usage(observed: &Observed, said: String) -> String {
    if !said.contains("changes.plan") {
        return said;
    }
    let usage = if observed.change.plan.is_none() {
        NEW_PLAN_USAGE
    } else {
        REVISED_PLAN_USAGE
    };
    format!("{said} {usage}")
}

/// What an instruction that asks for a new plan ends with.
const NEW_PLAN_USAGE: &str =
    "Pipe `changes.plan` JSON on stdin with the fields `path`, `text`, `uses` and `paths`.";
/// What an instruction that asks for a plan ends with when the change has one.
const REVISED_PLAN_USAGE: &str =
    "`changes.plan` replaces the plan: resend its `text`, `uses` and every path.";

/// The text of the change's plan, or none when it names none or nothing is there.
pub(crate) fn plan_text(
    host: &dyn Host,
    change: &ChangeRecord,
) -> Result<Option<String>, PluginError> {
    Ok(match plan_file(host, change)? {
        Some(file) => get_text(host, &file)?,
        None => None,
    })
}

/// The Markdown file the change's plan record names as its text, or none
/// when it names no plan Board reads.
pub(crate) fn plan_file(
    host: &dyn Host,
    change: &ChangeRecord,
) -> Result<Option<String>, PluginError> {
    let Some(plan) = &change.plan else {
        return Ok(None);
    };
    Ok(plan_at(host, plan)?.map(|(record, _)| record.text))
}

/// What the change's gate reads: its flow, and what the checks of its
/// phase need, by their kinds (`gates::reads`).
pub(crate) fn observe(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
) -> Result<Observed, PluginError> {
    let flow = of_change(host, path, change)?;
    let wanted = reads(
        (flow.as_ref())
            .and_then(|flow| flow.phase(&change.phase))
            .map_or(&[], |phase| phase.checks.as_slice()),
    );
    let cited = if wanted.contains(&Read::References) {
        attached(host, path)?
    } else {
        Vec::new()
    };
    let records = referenced(host, &cited)?;
    let mut found = Observed::new(path, change.clone(), kind_of(host, path)?, records);
    found.flow = flow;
    found.cited = cited;
    found.board = board::resolve(host, "/")?
        .map(|report| report.location.board.repository.display().to_string());
    if wanted.contains(&Read::Plan) {
        found.plan_file = plan_file(host, change)?;
        found.plan = match &found.plan_file {
            Some(file) => get_text(host, file)?,
            None => None,
        };
        found.uses = current(host, change)?.1;
    }
    if wanted.contains(&Read::Suite) {
        found.suite = if change.v1 {
            check_suite(host, &change.mount)?
        } else {
            change.suite.clone()
        };
    }
    if wanted.contains(&Read::Diff) {
        reviewed(host, &mut found)?;
    }
    if wanted.contains(&Read::Pull) {
        let newest = change.pulls().last().map_or("", String::as_str);
        found.pull_read = pull_of(host, newest)?.is_some();
    }
    if wanted.contains(&Read::Obligations) {
        found.obligations = obligations(host, &current(host, change)?.2)?;
    }
    if wanted.contains(&Read::Controlled) {
        let filled = values(&found);
        let named: Vec<String> = (found.checks().iter())
            .flat_map(|each| named_controls(each, &filled))
            .collect();
        for control in named {
            let listed = listed_by(host, &control, &found.edited)?;
            found.controlled.insert(control, listed);
        }
    }
    if wanted.contains(&Read::Tickets) {
        let tickets = change.tickets.clone().unwrap_or_default();
        found.ticket_types = (referenced(host, &tickets)?.records.into_iter())
            .map(|(_, (kind, _))| kind)
            .collect();
    }
    if wanted.contains(&Read::Exists) {
        let filled = values(&found);
        let paths: Vec<String> = (found.checks().iter())
            .filter(|each| each.kind.known() == Some(&CheckKind::Exists))
            .filter_map(|each| each.path.as_deref())
            .filter(|template| unfilled(template, &filled).is_empty())
            .map(|template| fill(template, &filled))
            .collect();
        for path in paths {
            let held = board::resolve(host, &path)?.is_some();
            found.existing.insert(path, held);
        }
    }
    Ok(found)
}

/// Adds the paths of the plan the change names and the files the change's diff edits
/// outside them to `found`.
fn reviewed(host: &dyn Host, found: &mut Observed) -> Result<(), PluginError> {
    let (_, _, planned) = current(host, &found.change)?;
    found.planned = planned;
    let Some(head) = found.change.head.clone() else {
        found.diff_unread = Some(
            "the change records no head commit, so its diff is not known; attach one with --head"
                .to_owned(),
        );
        return Ok(());
    };
    match edited(host, &found.change.mount, &head)? {
        Ok(files) => {
            if !found.planned.is_empty() {
                found.outside = outside(host, &files, &found.planned)?;
            }
            found.edited = files;
        }
        Err(unread) => found.diff_unread = Some(unread),
    }
    Ok(())
}

/// What to do now: the prototype's loop, the advance command once the gate
/// passes, what to do while it waits on the owner or a reviewer, or the
/// phase's instruction.
fn instruction_for(observed: &Observed, checks: &[Check], waiting: bool) -> String {
    let (path, change) = (observed.path.as_str(), &observed.change);
    let phase = change.phase.as_str();
    if observed.kind == Kind::Prototype && phase == BUILD {
        return format!(
            "Iterate with the owner on branch {}, committing and pushing after every round. When \
             the owner has his answer, ask for the verdict in one sentence and record it with \
             `board call changes.verdict`.",
            branch_of(path)
        );
    }
    let open = !is_final(phase) && phase != PAUSED;
    if open && !checks.iter().any(Check::blocking) {
        return format!("The gate passes: run `board call changes.advance --path {path}`.");
    }
    let said = instruction(observed);
    if open && waiting {
        return format!(
            "{said} Only the owner or a reviewer can pass the checks left: ask them now with \
             `ask`, or, running without them, record `ask` as a plans/question/v1 with `board \
             put`, its blocks naming {path} and its mount {}, and stop here.",
            change.mount
        );
    }
    match (phase, &change.merge_commit_url, &change.dropped_reason) {
        (DONE, Some(merged), _) => format!("{said} It merged as {merged}."),
        (DROPPED, _, Some(reason)) => format!("{said} It was dropped: {reason}"),
        _ => said,
    }
}

/// `said` with why the change at `path` was last reopened, while it is in
/// build and `reopen` recorded a reason.
fn with_reopened(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    said: String,
) -> Result<String, PluginError> {
    if change.phase != BUILD {
        return Ok(said);
    }
    Ok(match latest_reason(host, path)? {
        Some(reason) => format!("{said} It was reopened for a fix round: {reason}"),
        None => said,
    })
}

/// Whom the change waits on: the actors of its missing checks that block,
/// when every one is the owner or a reviewer, and the question to ask
/// them, built from those checks' remedies.
fn waiting(observed: &Observed, checks: &[Check]) -> (Vec<Actor>, Option<String>) {
    let blocking: Vec<&Check> = checks.iter().filter(|each| each.blocking()).collect();
    if blocking.is_empty() || blocking.iter().any(|each| each.actor == Actor::Agent) {
        return (Vec::new(), None);
    }
    let mut actors: Vec<Actor> = blocking.iter().map(|each| each.actor).collect();
    actors.sort_by_key(|actor| *actor == Actor::Reviewer);
    actors.dedup();
    let mut ask = format!(
        "The change {} ({}) waits in its {} phase on these checks:",
        observed.path, observed.change.title, observed.change.phase
    );
    for each in blocking {
        let remedy = each.remedy.as_deref().unwrap_or_default();
        let _ = write!(ask, "\n- {}: {} {remedy}", each.check, each.detail);
    }
    (actors, Some(ask))
}

/// The change's flow and phase, route, its gate's checks, whom it waits
/// on, and the next instruction.
pub(crate) fn next_of(host: &dyn Host, path: &str) -> Result<NextOutput, PluginError> {
    Ok(step_of(host, path)?.0)
}

/// What `next` says of the change at `path`, and what its gate read.
pub(crate) fn step_of(host: &dyn Host, path: &str) -> Result<(NextOutput, Observed), PluginError> {
    let (change, _) = read(host, path)?;
    let observed = observe(host, path, &change)?;
    let checks = gate(&observed);
    let (waiting_on, ask) = waiting(&observed, &checks);
    let passes = !checks.iter().any(Check::blocking);
    let stop = match (&change.stop, &observed.flow) {
        (Some(stop), Some(flow)) => Some(Shown {
            reached: reached(flow, &change.phase, stop, passes),
            stop: stop.clone(),
        }),
        _ => None,
    };
    let instruction = match &stop {
        Some(shown) if shown.reached => format!(
            "The change has reached the stop of its run, {}: stop here. Running unattended, \
             record it with `board call changes.stop --path {path} --reason reached --summary \
             '<one line>'` and exit.",
            shown.stop
        ),
        _ => with_reopened(
            host,
            path,
            &change,
            instruction_for(&observed, &checks, ask.is_some()),
        )?,
    };
    let next = NextOutput {
        path: path.to_owned(),
        flow: observed.flow.as_ref().map(FlowSummary::from),
        phase: change.phase.clone(),
        route: change.route,
        instruction,
        gate: checks,
        waiting_on,
        ask,
        stop,
        sync: last_sync(host, sync_reads(path, &change))?,
    };
    Ok((next, observed))
}

pub(crate) fn next(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PathInput { path } = PluginError::read_input(PLUGIN, "next", input)?;
    Ok(to_value(next_of(host, &path)?))
}

/// Adds `reference` to `listed` once.
fn add(listed: &mut Option<Vec<String>>, reference: &str) {
    let references = listed.get_or_insert_with(Vec::new);
    if !references.iter().any(|each| each == reference) {
        references.push(reference.to_owned());
    }
}

pub(crate) fn attach(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: AttachInput = PluginError::read_input(PLUGIN, "attach", input)?;
    let path = asked.path;
    let (mut change, blob) = read(host, &path)?;
    let mut lines = Vec::new();
    if let Some(reference) = &asked.reference {
        check_target(host, reference, &[])?;
        let (plan_path, mut plan, plan_blob) = current_plan(host, &path, &change)?;
        add(&mut plan.records, reference);
        lines.push(json!({"path": plan_path, "expect": plan_blob, "record": plan}));
    }
    if let Some(ticket) = &asked.ticket {
        check_target(host, ticket, &TICKET_TYPES)?;
        add(&mut change.tickets, ticket);
    }
    if let Some(pull) = &asked.pull {
        check_target(host, pull, &PULL_TYPES)?;
        add(&mut change.pulls, pull);
    }
    if let Some(task) = asked.task {
        check_target(host, &task, &[TASK_TYPE])?;
        change.task = Some(task);
    }
    if let Some(suite) = asked.suite {
        refuse_v1(&path, &change, "a suite")?;
        check_target(host, &suite, &[SUITE_TYPE])?;
        change.suite = Some(suite);
    }
    change.route = asked.route.or(change.route);
    change.head = asked.head.or(change.head);
    let change = touch(host, change);
    lines.push(line(&path, &blob, &change));
    board::put(host, &lines)?;
    Ok(json!({"path": path, "records": attached(host, &path)?.len()}))
}

/// The plan record the change at `path` names as its plan, its blob, or
/// `changes::attach::no_plan` when it names none that Board reads.
fn current_plan(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
) -> Result<(String, crate::records::PlanRecord, String), PluginError> {
    let found = match &change.plan {
        Some(plan) => plan_at(host, plan)?.map(|(record, blob)| (plan.clone(), record, blob)),
        None => None,
    };
    found.ok_or_else(|| {
        refusal(
            "changes::attach::no_plan",
            format!(
                "the change at {path} names no plan record to keep the reference; write its plan \
                 with `board call changes.plan --path {path}`, then attach it again"
            ),
            [("path", json!(path))],
        )
    })
}

/// Refuses `reference` when nothing is there, or when `types` names some
/// and it is a record of none of them.
pub(crate) fn check_target(
    host: &dyn Host,
    reference: &str,
    types: &[&str],
) -> Result<(), PluginError> {
    let Some(answer) = board::resolve(host, reference)? else {
        return Err(refusal(
            "changes::ref::dangling",
            format!(
                "nothing is at {reference}, so it cannot be attached; check the path with `board ls`"
            ),
            [("ref", json!(reference))],
        ));
    };
    if types.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "SELECT type FROM entries WHERE logical = {}",
        literal(answer.display.as_str())
    );
    let found = rows(host, &sql)?;
    let found = found
        .first()
        .and_then(|row| text(row.first()))
        .map(str::to_owned);
    if found.as_deref().is_some_and(|found| types.contains(&found)) {
        return Ok(());
    }
    let kind = found.as_ref().map_or_else(
        || "is not a record".to_owned(),
        |found| format!("has type {found}"),
    );
    Err(refusal(
        "changes::ref::wrong_type",
        format!(
            "{reference} {kind}, and this reference accepts only {} records; point it at one of \
             those, or check the target's `$type` with `board get {reference}`",
            types.join(" or ")
        ),
        [
            ("ref", json!(reference)),
            ("type", json!(found)),
            ("accepts", json!(types)),
        ],
    ))
}

/// `changes::change::not_migrated` for a v1 change, which holds no `what`.
pub(crate) fn refuse_v1(path: &str, change: &ChangeRecord, what: &str) -> Result<(), PluginError> {
    if !change.v1 {
        return Ok(());
    }
    Err(refusal(
        "changes::change::not_migrated",
        format!(
            "the change at {path} is a {}, which holds no {what}; rewrite it as a {} with `board \
             call changes.migrate --apply true`, then try again",
            crate::CHANGE_V1,
            crate::CHANGE_TYPE
        ),
        [("path", json!(path))],
    ))
}

/// `changes::phase::final` for a change in `phase`, saying `why`.
pub(crate) fn final_phase(path: &str, phase: &str, why: String) -> PluginError {
    refusal(
        "changes::phase::final",
        why,
        [("path", json!(path)), ("phase", json!(phase))],
    )
}

pub(crate) fn advance(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let AdvanceInput { path, over_limit } = PluginError::read_input(PLUGIN, "advance", input)?;
    let (mut change, blob) = read(host, &path)?;
    let phase = change.phase.clone();
    if is_final(&phase) {
        return Err(final_phase(
            &path,
            &phase,
            format!(
                "the change at {path} is {phase}, which has no next phase; list the open changes \
                 with `board call changes.open`, or start one with `board call changes.start`"
            ),
        ));
    }
    if phase == PAUSED {
        return Err(refusal(
            "changes::phase::paused",
            format!(
                "the change at {path} is paused, which has no gate; resume it with `board call \
                 changes.resume --path {path}`"
            ),
            [("path", json!(path)), ("phase", json!(phase))],
        ));
    }
    let observed = observe(host, &path, &change)?;
    if let (Some(stop), Some(flow)) = (&change.stop, &observed.flow)
        && holds(flow, &phase, stop)
    {
        return Err(stop_reached(&path, &phase, stop));
    }
    let missing: Vec<Check> = (gate(&observed).into_iter())
        .filter(Check::blocking)
        .collect();
    if let Some(first) = missing.first() {
        let said: Vec<String> = (missing.iter())
            .map(|found| {
                let remedy = found.remedy.as_deref().unwrap_or_default();
                format!("{}: {} ({remedy})", found.check, found.detail)
            })
            .collect();
        let names: Vec<&str> = missing.iter().map(|found| found.check.as_str()).collect();
        let remedies: Vec<&str> = (missing.iter())
            .map(|found| found.remedy.as_deref().unwrap_or_default())
            .collect();
        return Err(refusal(
            first.code,
            format!(
                "the {phase} gate of {path} is unmet: {}; run `board call changes.next --path \
                 {path}` for the next step",
                said.join("; ")
            ),
            [
                ("path", json!(path)),
                ("phase", json!(phase)),
                ("missing", json!(names)),
                ("remedies", json!(remedies)),
            ],
        ));
    }
    let to = (observed.flow.as_ref())
        .and_then(|flow| flow.after(&phase))
        .map(str::to_owned);
    let Some(to) = to else {
        return Err(final_phase(
            &path,
            &phase,
            format!("the change at {path} has no next phase"),
        ));
    };
    change.phase.clone_from(&to);
    let mut limit = None;
    if to == BUILD {
        let moving = Move {
            over: over_command("advance", &path),
            reason: over_limit.as_deref(),
            days: active_days(host)?,
        };
        limit = admit(host, &path, &mut change, kind_of(host, &path)?, &moving)?;
    }
    let change = touch(host, change);
    write(host, &path, &blob, &change)?;
    Ok(to_value(Moved {
        path,
        from: phase,
        to,
        warning: limit,
    }))
}

/// A change and the flow to move it to.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReflowInput {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(
        regex(pattern = "^[a-z0-9]+(-[a-z0-9]+)*$"),
        description = "The flow to follow from now on: the board's own at /flows/<name>, or a \
                       shipped one, development or small."
    )]
    flow: String,
}

/// The change, the flow it follows now and the phase it stands in.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Reflowed {
    path: String,
    flow: FlowSummary,
    phase: String,
}

/// Replaces the change's copy of its flow with a copy of the flow named,
/// which must hold the phase the change stands in; a paused change stands
/// in build.
pub(crate) fn reflow(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let ReflowInput { path, flow } = PluginError::read_input(PLUGIN, "reflow", input)?;
    let (mut change, blob) = read(host, &path)?;
    refuse_v1(&path, &change, "copy of a flow")?;
    if is_final(&change.phase) {
        return Err(final_phase(
            &path,
            &change.phase,
            format!(
                "the change at {path} is {}, so it follows no flow any more; list the open changes \
                 with `board call changes.open`",
                change.phase
            ),
        ));
    }
    let chosen = asked_for(host, &flow)?;
    refuse_invalid(&chosen, "reflow")?;
    if change.mount == BOARD_ITSELF {
        refuse_without_repository(&chosen, &change.mount)?;
    }
    let standing = if change.phase == PAUSED {
        BUILD
    } else {
        change.phase.as_str()
    };
    if chosen.phase(standing).is_none() {
        return Err(refusal(
            "changes::reflow::no_phase",
            format!(
                "the flow {flow} has no phase {standing}, where the change at {path} stands, so \
                 the change would have nowhere to be; choose a flow with a {standing} phase, \
                 whose phases `board call changes.next` lists"
            ),
            [
                ("path", json!(path)),
                ("flow", json!(flow)),
                ("phase", json!(standing)),
                ("phases", json!(chosen.names())),
            ],
        ));
    }
    let copy = format!("{path}/flow");
    let (at, copied) = copy_line(&path, &blob_at(host, &copy)?, &chosen);
    change.flow = Some(at);
    let change = touch(host, change);
    board::put(host, &[copied, line(&path, &blob, &change)])?;
    Ok(to_value(Reflowed {
        path,
        flow: FlowSummary::from(&chosen),
        phase: change.phase,
    }))
}

/// The command that moves the change at `path` into build past the WIP
/// limit, recording why.
fn over_command(capability: &str, path: &str) -> String {
    format!("board call changes.{capability} --path {path} --over_limit '<why>'")
}
