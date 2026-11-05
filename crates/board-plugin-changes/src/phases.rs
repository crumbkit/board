//! The `next`, `attach`, `advance`, `pause` and `resume` capabilities:
//! where a change stands, what it cites, and moving it on.

use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Prose;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board::{self, get_text, rows, text};
use crate::change::write;
use crate::change::{
    LastSync, attached, branch_of, current, kind_of, last_sync, line, plan_at, pull_of, read,
    referenced, touch,
};
use crate::diff::{edited, outside};
use crate::gates::{Check, Observed, Status, code_of, gate};
use crate::records::{ChangeRecord, Kind, PULL_TYPES, Phase, Route, TASK_TYPE, TICKET_TYPES};
use crate::repo::check_suite;
use crate::wip::{Limit, Move, active_days, admit};
use crate::{PLUGIN, refusal, to_value};

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
}

/// Where the change stands: its phase, route, the gate's checks, what to do
/// now and the last sync.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct NextOutput {
    pub(crate) path: String,
    pub(crate) phase: Phase,
    pub(crate) route: Option<Route>,
    pub(crate) gate: Vec<Check>,
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
    from: Phase,
    to: Phase,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<Limit>,
}

/// What the builder of each phase does next.
fn instruction(phase: Phase) -> &'static str {
    match phase {
        Phase::Plan => {
            "Interview the owner and fill in the plan's Problem, Outcome, Out of scope, UX and \
             Data sections. UX says No UI change with a reason, or lists the mockups and a \
             Neighbours list; attach the owner's decision naming each mockup. Data says None, \
             or run the understanding-check skill and attach its record and the owner's \
             decision that references it. On the ask-first route, attach the owner's \
             plans/decision/v1 whose refs name the plan."
        }
        Phase::Build => {
            "Build test-first. Attach each task, record the head commit with --head, run the \
             check suite on it and attach the run. Before advancing, ask the owner two of each \
             attached understanding check's predict or edge questions again, worded \
             differently, and add them to its rechecks."
        }
        Phase::Review => {
            "Review the head commit with clean context and attach the review/report/v1. When \
             the plan's UX says No UI change or its Data says None, list the repository's \
             UI-path and data-path controls, /controls/<repo>/ui-paths and \
             /controls/<repo>/data-paths, with `board call controls.check`, and report a diff \
             that touches a path they name as a high finding. Fix blocking findings, record the \
             new head and review again."
        }
        Phase::Land => {
            "Draft the pull request with `board call changes.pr` and open it with the command \
             it returns. Record it with `board call github.ref --ref <owner>/<repo>#<number>` \
             and attach that record with --pull. Merging happens outside Board."
        }
        Phase::Done | Phase::Dropped => "Nothing is left to do.",
        Phase::Paused => {
            "The keeper paused this change to keep the work in progress small. Resume it with \
             `board call changes.resume` when the owner's queue and the cap allow."
        }
    }
}

/// The text of the change's plan, or none when it names none or nothing is
/// there.
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

/// What the change's gate reads: references where the gate uses them, the
/// plan in `plan`, the docs in `build`, and the diff in `review`.
pub(crate) fn observe(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
) -> Result<Observed, PluginError> {
    let phase = change.phase;
    let reads = matches!(phase, Phase::Plan | Phase::Build | Phase::Review);
    let cited = if reads {
        attached(host, path)?
    } else {
        Vec::new()
    };
    let records = referenced(host, &cited)?;
    let mut found = Observed::new(path, change.clone(), kind_of(host, path)?, records);
    found.cited = cited;
    match phase {
        Phase::Plan => {
            found.plan_file = plan_file(host, change)?;
            found.plan = match &found.plan_file {
                Some(file) => get_text(host, file)?,
                None => None,
            };
        }
        Phase::Build => found.suite = check_suite(host, &change.mount)?,
        Phase::Review => reviewed(host, &mut found)?,
        Phase::Land => {
            let newest = change.pulls().last().map_or("", String::as_str);
            found.pull_read = pull_of(host, newest)?.is_some();
        }
        _ => {}
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
        Ok(files) if !found.planned.is_empty() => {
            found.outside = outside(host, &files, &found.planned)?;
        }
        Ok(_) => {}
        Err(unread) => found.diff_unread = Some(unread),
    }
    Ok(())
}

/// What to do now: the prototype's loop, the advance command once the gate
/// passes, or the phase's instruction.
fn instruction_for(observed: &Observed, checks: &[Check]) -> String {
    let (path, change) = (observed.path.as_str(), &observed.change);
    let phase = change.phase;
    if observed.kind == Kind::Prototype && phase == Phase::Build {
        return format!(
            "Iterate with the owner on branch {}, committing and pushing after every round. When \
             the owner has his answer, ask for the verdict in one sentence and record it with \
             `board call changes.verdict`.",
            branch_of(path)
        );
    }
    let open = !phase.is_final() && phase != Phase::Paused;
    if open && checks.iter().all(|found| found.status == Status::Pass) {
        return format!("The gate passes: run `board call changes.advance --path {path}`.");
    }
    let said = instruction(phase);
    match (phase, &change.merge_commit_url, &change.dropped_reason) {
        (Phase::Done, Some(merged), _) => format!("{said} It merged as {merged}."),
        (Phase::Dropped, _, Some(reason)) => format!("{said} It was dropped: {reason}"),
        _ => said.to_owned(),
    }
}

/// The change's phase and route, its gate's checks, and the next
/// instruction.
pub(crate) fn next_of(host: &dyn Host, path: &str) -> Result<NextOutput, PluginError> {
    let (change, _) = read(host, path)?;
    let observed = observe(host, path, &change)?;
    let checks = gate(&observed);
    Ok(NextOutput {
        path: path.to_owned(),
        phase: change.phase,
        route: change.route,
        instruction: instruction_for(&observed, &checks),
        gate: checks,
        sync: last_sync(host)?,
    })
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
        let (plan_path, mut plan, plan_blob) = newest_plan(host, &path, &change)?;
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
    change.route = asked.route.or(change.route);
    change.head = asked.head.or(change.head);
    let change = touch(host, change);
    lines.push(line(&path, &blob, &change));
    board::put(host, &lines)?;
    Ok(json!({"path": path, "records": attached(host, &path)?.len()}))
}

/// The plan record the change at `path` names as its plan, its blob, or
/// `changes::attach::no_plan` when it names none that Board reads.
fn newest_plan(
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

/// `changes::phase::final` for a change in `phase`, saying `why`.
pub(crate) fn final_phase(path: &str, phase: Phase, why: String) -> PluginError {
    refusal(
        "changes::phase::final",
        why,
        [("path", json!(path)), ("phase", json!(phase))],
    )
}

pub(crate) fn advance(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let AdvanceInput { path, over_limit } = PluginError::read_input(PLUGIN, "advance", input)?;
    let (mut change, blob) = read(host, &path)?;
    let phase = change.phase;
    if phase.is_final() {
        return Err(final_phase(
            &path,
            phase,
            format!(
                "the change at {path} is {}, which has no next phase; list the open changes with \
                 `board call changes.open`, or start one with `board call changes.start`",
                phase.as_str()
            ),
        ));
    }
    if phase == Phase::Paused {
        return Err(refusal(
            "changes::phase::paused",
            format!(
                "the change at {path} is paused, which has no gate; resume it with `board call \
                 changes.resume --path {path}`"
            ),
            [("path", json!(path)), ("phase", json!(phase))],
        ));
    }
    let missing: Vec<Check> = (gate(&observe(host, &path, &change)?).into_iter())
        .filter(|found| found.status == Status::Missing)
        .collect();
    if let Some(first) = missing.first() {
        let said: Vec<String> = (missing.iter())
            .map(|found| format!("{}: {}", found.check, found.detail))
            .collect();
        let names: Vec<&str> = missing.iter().map(|found| found.check).collect();
        return Err(refusal(
            code_of(first.check),
            format!(
                "the {} gate of {path} is unmet: {}; run `board call changes.next --path {path}` \
                 for the next step",
                phase.as_str(),
                said.join("; ")
            ),
            [
                ("path", json!(path)),
                ("phase", json!(phase)),
                ("missing", json!(names)),
            ],
        ));
    }
    let Some(to) = phase.next() else {
        return Err(final_phase(
            &path,
            phase,
            format!("the change at {path} has no next phase"),
        ));
    };
    change.phase = to;
    let mut limit = None;
    if to == Phase::Build {
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

pub(crate) fn pause(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PathInput { path } = PluginError::read_input(PLUGIN, "pause", input)?;
    let (mut change, blob) = read(host, &path)?;
    if change.phase != Phase::Build {
        return Err(refusal(
            "changes::pause::not_building",
            format!(
                "the change at {path} is in {}, and only a change in build is paused; list the \
                 changes in build with `board call changes.open`",
                change.phase.as_str()
            ),
            [("path", json!(path)), ("phase", json!(change.phase))],
        ));
    }
    change.phase = Phase::Paused;
    write(host, &path, &blob, &change)?;
    Ok(to_value(Moved {
        path,
        from: Phase::Build,
        to: Phase::Paused,
        warning: None,
    }))
}

pub(crate) fn resume(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let AdvanceInput { path, over_limit } = PluginError::read_input(PLUGIN, "resume", input)?;
    let (mut change, blob) = read(host, &path)?;
    if change.phase != Phase::Paused {
        return Err(refusal(
            "changes::resume::not_paused",
            format!(
                "the change at {path} is in {}, and only a paused change is resumed; run `board \
                 call changes.next --path {path}` for its next step",
                change.phase.as_str()
            ),
            [("path", json!(path)), ("phase", json!(change.phase))],
        ));
    }
    change.phase = Phase::Build;
    let moving = Move {
        over: over_command("resume", &path),
        reason: over_limit.as_deref(),
        days: active_days(host)?,
    };
    let limit = admit(host, &path, &mut change, kind_of(host, &path)?, &moving)?;
    let change = touch(host, change);
    write(host, &path, &blob, &change)?;
    Ok(to_value(Moved {
        path,
        from: Phase::Paused,
        to: Phase::Build,
        warning: limit,
    }))
}

/// The command that moves the change at `path` into build past the WIP
/// limit, recording why.
fn over_command(capability: &str, path: &str) -> String {
    format!("board call changes.{capability} --path {path} --over_limit '<why>'")
}
