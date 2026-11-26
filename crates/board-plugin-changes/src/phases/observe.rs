//! What a change's gate reads beyond the change: `observe` fills the
//! gate's input with what the checks of its phase need, by their kinds.

use std::collections::{BTreeMap, BTreeSet};

use board_ops::{Host, PluginError};
use board_plugin::shape::Record;
use serde_json::Value;

use crate::approve::approval;
use crate::board::{self, get_text};
use crate::change::{
    NOT_FOUND, Referenced, attached, current, kind_of, listed, plan_at, pull_of, referenced,
    whole_record_at,
};
use crate::diff::{Step, accepted, after, edited, mounted, outside};
use crate::flow::{self, Kind as CheckKind, of_change};
use crate::gates::kinds::named_rules;
use crate::gates::remedy::{fill, unfilled, values};
use crate::gates::{Observed, Read, objects, reads, reviewed_head, same_commit};
use crate::lifecycle::LifecycleRecord;
use crate::records::{ChangeRecord, FixRecord, Kind, PlanRecord};
use crate::repo::check_suite;
use crate::rules::{Found, current as current_rule, listed_by, names, obligations_in, on_plan};

use super::plan_file;

/// What the change's gate reads: its flow, and what the checks of its
/// phase need, by their kinds (`gates::reads`).
pub(crate) fn observe(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
) -> Result<Observed, PluginError> {
    let flow = of_change(host, path, change)?;
    observe_in(host, path, change, flow)
}

/// What a caller already read of a change that follows a flow, which
/// [`observe_known`] takes in place of reading it again.
///
/// The change is observed as a build whatever its kind, a prototype
/// included. That is sound only because its one caller, `owed::owing`,
/// judges the change through `interpret`, which never reads the observed
/// kind; `gate` does, so a caller that gates must not pass a `Known`.
pub(crate) struct Known {
    /// The plan record at the change's `plan`, or none when Board reads no
    /// plan record there.
    pub(crate) plan: Option<PlanRecord>,
    /// The board's root, as [`board_root`] gives it, or none when no check
    /// names `{board}`.
    pub(crate) board: Option<String>,
    /// The records attached to the change, when its checks read them.
    pub(crate) cited: Vec<String>,
    /// What Board reads of each of `cited`.
    pub(crate) records: Referenced,
}

/// The board's root, or none when Board names none. It costs a `refs`
/// call that decides the whole board.
pub(crate) fn board_root(host: &dyn Host) -> Result<Option<String>, PluginError> {
    Ok(board::resolve(host, "/")?
        .map(|report| report.location.board.repository.display().to_string()))
}

/// What the gate of the change's phase in `flow` reads.
pub(crate) fn observe_in(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    flow: Option<flow::Flow>,
) -> Result<Observed, PluginError> {
    observe_known(host, path, change, flow, None)
}

/// [`observe_in`], with what `known` holds in place of reading it.
pub(crate) fn observe_known(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    flow: Option<flow::Flow>,
    known: Option<Known>,
) -> Result<Observed, PluginError> {
    let wanted = reads(
        (flow.as_ref())
            .and_then(|flow| flow.phase(&change.phase))
            .map_or(&[], |phase| phase.checks.as_slice()),
    );
    let mut found = match known {
        Some(known) => given(host, path, change, flow, &wanted, known)?,
        None => unknown(host, path, change, flow, &wanted)?,
    };
    let mut on_planned = None;
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
        found.obligations = obligations(host, &found, &mut on_planned)?;
    }
    if wanted.contains(&Read::Controlled) {
        let filled = values(&found);
        let named: Vec<String> = (found.checks().iter())
            .flat_map(|each| named_rules(each, &filled))
            .collect();
        for rule in named {
            let listed = listed_by(host, &rule, &found.edited)?;
            found.controlled.insert(rule, listed);
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
    if wanted.contains(&Read::Absent) {
        records_held(host, &mut found)?;
    }
    if wanted.contains(&Read::PlannedRules) {
        planned_rules(host, &mut found, &mut on_planned)?;
    }
    if wanted.contains(&Read::Approval) {
        found.approval = approval(host, &found)?;
    }
    if wanted.contains(&Read::AfterReview) {
        let checks = found.checks();
        found.after_review = after_review(host, &found, checks)?;
    }
    Ok(found)
}

/// What the change's gate reads of the change, its attached records, the
/// board's root and, when the checks read the plan, the plan.
fn unknown(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    flow: Option<flow::Flow>,
    wanted: &BTreeSet<Read>,
) -> Result<Observed, PluginError> {
    let cited = if wanted.contains(&Read::References) {
        attached(host, path)?
    } else {
        Vec::new()
    };
    let records = referenced(host, &cited)?;
    let mut found = Observed::new(path, change.clone(), kind_of(host, path)?, records);
    found.flow = flow;
    found.cited = cited;
    found.board = board_root(host)?;
    if wanted.contains(&Read::Plan) {
        found.plan_file = plan_file(host, change)?;
        found.plan = match &found.plan_file {
            Some(file) => get_text(host, file)?,
            None => None,
        };
        let named = (change.plan.as_deref().map(|plan| plan_at(host, plan))).transpose()?;
        if let Some((record, _)) = named.flatten() {
            found.answers = record.answers.unwrap_or_default();
            found.uses = record.uses;
        }
        (found.considered, found.owed) = listed(host, change)?;
        found.surfaced_gone = surfaced_gone(host, change)?;
    }
    Ok(found)
}

/// [`unknown`], from what `known` holds. The plan's text and the change's
/// surfaced records are read only when a check reads more of the plan than
/// the answers its `when` names.
fn given(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    flow: Option<flow::Flow>,
    wanted: &BTreeSet<Read>,
    known: Known,
) -> Result<Observed, PluginError> {
    let mut found = Observed::new(path, change.clone(), Kind::Build, known.records);
    found.flow = flow;
    found.cited = known.cited;
    found.board = known.board;
    if !wanted.contains(&Read::Plan) {
        return Ok(found);
    }
    if reads_past_answers(found.checks()) {
        found.plan = match &known.plan {
            Some(record) => get_text(host, &record.text)?,
            None => None,
        };
        found.surfaced_gone = surfaced_gone(host, change)?;
    }
    if let Some(record) = known.plan {
        found.plan_file = Some(record.text);
        found.answers = record.answers.unwrap_or_default();
        found.uses = record.uses;
        found.considered = (record.considered.into_iter().flatten())
            .flat_map(|each| each.records)
            .collect();
        found.owed = (record.owed.into_iter().flatten())
            .map(|each| current_rule(&each.rule))
            .collect();
    }
    Ok(found)
}

/// For each commit that a record a `records` check of `checks` counts
/// covers, other than the change's head: the commits after it to the head
/// when each edits only paths its `unchanged_by` rules list or the
/// change's lifecycle doc, cleanly merges the branch the mount follows, or
/// is a fix commit a counted `changes/fix/v1` names, or else why not.
pub(crate) fn after_review<'a>(
    host: &dyn Host,
    found: &Observed,
    checks: impl IntoIterator<Item = &'a flow::Check>,
) -> Result<BTreeMap<String, Result<Vec<String>, String>>, PluginError> {
    let mut walked = BTreeMap::new();
    let Some(head) = found.change.head.as_deref() else {
        return Ok(walked);
    };
    let filled = values(found);
    let mut rules = Vec::new();
    let mut kinds = Vec::new();
    let mut covered = BTreeSet::new();
    for each in checks {
        let (Some(conditions), Some(kind)) = (&each.conditions, &each.record_type) else {
            continue;
        };
        let Some(unchanged_by) = &conditions.unchanged_by else {
            continue;
        };
        kinds.push(kind.as_str());
        rules.extend(
            unchanged_by
                .iter()
                .map(|rule| current_rule(&fill(rule, &filled))),
        );
        for (_, body) in found.records.of_type(kind) {
            let reviewed = reviewed_head(body);
            if let Some(commit) = reviewed.as_ref().and_then(|value| value.as_str())
                && !same_commit(reviewed.as_ref(), head)
            {
                covered.insert(commit.to_owned());
            }
        }
    }
    if covered.is_empty() {
        return Ok(walked);
    }
    let at = match mounted(host, &found.change.mount)? {
        Ok(at) => at,
        Err(why) => {
            walked.extend(covered.into_iter().map(|commit| (commit, Err(why.clone()))));
            return Ok(walked);
        }
    };
    let doc = lifecycle_doc(host, &found.path)?;
    for commit in covered {
        let (fixed, refused) = fixes(found, &kinds, &commit);
        let fixed: Vec<String> = fixed.into_iter().map(|(_, commit)| commit).collect();
        let walk = match after(host, &at, &commit, head) {
            Ok(steps) => {
                allowed(host, &rules, doc.as_deref(), &steps).map(|(allowed, absent)| {
                    accepted(&steps, &allowed, &fixed, &at.mount)
                        .map_err(|why| with_absent(with_refused(why, &refused), &absent))
                })?
            }
            Err(why) => Err(why),
        };
        walked.insert(commit, walk);
    }
    Ok(walked)
}

/// The commits the attached `changes/fix/v1` records name that answer an
/// attached report of one of `kinds` covering `commit`, each with the path
/// of the fix record that names it, and why each other such fix record
/// does not count. A fix record counts when its report has no high
/// finding, each finding number it gives is one of the report's, and it
/// records no deviation from the plan.
pub(crate) fn fixes(
    found: &Observed,
    kinds: &[&str],
    commit: &str,
) -> (Vec<(String, String)>, Vec<String>) {
    let mut fixed = Vec::new();
    let mut refused = Vec::new();
    for (path, fix) in found.records.of_type(FixRecord::TYPE) {
        let Some(named) = fix.get("report").and_then(Value::as_str) else {
            continue;
        };
        let report = (kinds.iter())
            .flat_map(|kind| found.records.of_type(kind))
            .find_map(|(each, body)| (each == named).then_some(body));
        let Some(report) =
            report.filter(|report| same_commit(reviewed_head(report).as_ref(), commit))
        else {
            continue;
        };
        let count = objects(report, "findings").count();
        let high = objects(report, "findings")
            .any(|finding| finding.get("severity").and_then(Value::as_str) == Some("high"));
        let unknown = (objects(fix, "commits"))
            .flat_map(|each| {
                each.get("findings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter_map(Value::as_u64)
            .find(|number| *number > count as u64);
        let why = if high {
            Some(format!("{path} answers {named}, which has a high finding"))
        } else if fix.contains_key("deviation") {
            Some(format!("{path} records a deviation from the plan"))
        } else {
            unknown
                .map(|number| format!("{path} names finding {number}, which {named} does not have"))
        };
        match why {
            Some(why) => refused.push(why),
            None => fixed.extend(objects(fix, "commits").filter_map(|each| {
                Some((path.to_owned(), each.get("commit")?.as_str()?.to_owned()))
            })),
        }
    }
    (fixed, refused)
}

/// `why` with the reason each fix record `refused` names does not count.
fn with_refused(why: String, refused: &[String]) -> String {
    if refused.is_empty() {
        return why;
    }
    format!(
        "{why}; {}, so the commits it names need a review of the head",
        refused.join("; ")
    )
}

/// `why` with the rules `absent` names, which the board does not hold, so
/// only clean merges and the lifecycle doc count.
fn with_absent(why: String, absent: &[String]) -> String {
    if absent.is_empty() {
        return why;
    }
    format!(
        "{why}; no rule {} exists, so only clean merges of main and the lifecycle doc count; \
         write the rule",
        absent.join(" or ")
    )
}

/// The files of `steps` the `rules` list, with `doc`, and the rules the
/// board does not hold.
fn allowed(
    host: &dyn Host,
    rules: &[String],
    doc: Option<&str>,
    steps: &[Step],
) -> Result<(BTreeSet<String>, Vec<String>), PluginError> {
    let files: Vec<String> = (steps.iter())
        .flat_map(|step| match step {
            Step::Edit { files, .. } => files.clone(),
            Step::Merge { .. } => Vec::new(),
        })
        .collect();
    let mut allowed: BTreeSet<String> = doc.map(str::to_owned).into_iter().collect();
    let mut absent = Vec::new();
    for rule in rules {
        match listed_by(host, rule, &files)? {
            Some(listed) => allowed.extend(listed),
            None => absent.push(rule.clone()),
        }
    }
    Ok((allowed, absent))
}

/// The `doc` of the change's lifecycle record, when it has one Board reads.
fn lifecycle_doc(host: &dyn Host, path: &str) -> Result<Option<String>, PluginError> {
    let record = format!("{path}/lifecycle");
    let found = referenced(host, std::slice::from_ref(&record))?;
    Ok((found.of_type(LifecycleRecord::TYPE))
        .find_map(|(_, body)| body.get("doc")?.as_str().map(str::to_owned)))
}

/// Adds to `found` each path a check's `when.absent` names that holds the
/// record it means.
fn records_held(host: &dyn Host, found: &mut Observed) -> Result<(), PluginError> {
    let filled = values(found);
    let asked: Vec<(String, String)> = (found.checks().iter())
        .filter_map(|each| each.when.as_ref()?.absent.as_ref())
        .filter(|absent| unfilled(&absent.path, &filled).is_empty())
        .map(|absent| (fill(&absent.path, &filled), absent.record_type.clone()))
        .collect();
    for (path, record_type) in asked {
        if board::resolve(host, &path)?.is_none() {
            continue;
        }
        if whole_record_at(host, &path, &record_type)? {
            found.records_held.insert(path);
        }
    }
    Ok(())
}

/// The answer of the one `rules.for-paths` call on the paths of the change's
/// plan, which `answer` holds once a read has asked for it.
fn answered<'a>(
    host: &dyn Host,
    change: &ChangeRecord,
    answer: &'a mut Option<Result<Vec<Found>, PluginError>>,
) -> Result<&'a Result<Vec<Found>, PluginError>, PluginError> {
    let asked = match answer.take() {
        Some(asked) => asked,
        None => on_plan(host, &current(host, change)?.2),
    };
    Ok(answer.insert(asked))
}

/// The obligations on the plan's paths and on each file the diff edits
/// outside them, which the change adds to its paths, so an added file's
/// obligation counts as a planned path's does. The diff is read first.
/// After the call on the plan's paths, the added files go [`BATCH`] to a
/// call, and each obligation is named once.
fn obligations(
    host: &dyn Host,
    found: &Observed,
    answer: &mut Option<Result<Vec<Found>, PluginError>>,
) -> Result<Vec<String>, PluginError> {
    let mut rules = (answered(host, &found.change, answer)?.as_ref())
        .map_err(Clone::clone)?
        .clone();
    for batch in found.outside.chunks(BATCH) {
        let added: Vec<String> = batch.iter().map(|file| glob_literal(file)).collect();
        rules.extend(on_plan(host, &added)?);
    }
    rules.sort_by(|one, other| one.rule.cmp(&other.rule));
    rules.dedup_by(|one, other| one.rule == other.rule);
    Ok((obligations_in(host, &rules)?.into_iter())
        .map(|each| each.rule)
        .collect())
}

/// The change's surfaced paths that no longer reach anything.
fn surfaced_gone(host: &dyn Host, change: &ChangeRecord) -> Result<Vec<String>, PluginError> {
    Ok((referenced(host, &change.surfaced)?.unreadable.into_iter())
        .filter(|(_, code)| code == NOT_FOUND)
        .map(|(path, _)| path)
        .collect())
}

/// Whether `checks` read the plan's text or the change's surfaced records:
/// a `when` on the plan's sections, or a kind that reads the plan, as
/// opposed to a `when` that reads only the plan's typed answers.
fn reads_past_answers(checks: &[flow::Check]) -> bool {
    checks.iter().any(|each| {
        let sections = (each.when.as_ref())
            .is_some_and(|when| when.section.is_some() || when.section_not.is_some());
        sections || reads(&[whenless(each)]).contains(&Read::Plan)
    })
}

/// `each` with no `when`.
fn whenless(each: &flow::Check) -> flow::Check {
    flow::Check {
        when: None,
        ..each.clone()
    }
}

/// The most added files one `rules.for-paths` call names.
const BATCH: usize = 100;

/// `file` as a SQLite GLOB pattern that matches only itself, since
/// `rules.for-paths` reads `*`, `?` and `[` as pattern characters and an
/// added file is a real name, such as `app/[id]/page.tsx`.
pub(crate) fn glob_literal(file: &str) -> String {
    let mut pattern = String::with_capacity(file.len());
    for character in file.chars() {
        match character {
            '[' | '*' | '?' => {
                pattern.push('[');
                pattern.push(character);
                pattern.push(']');
            }
            other => pattern.push(other),
        }
    }
    pattern
}

/// What `planned-rule-paths` checks read: which rules they name the board
/// holds, and the rules on the plan's paths, or why those cannot be read.
fn planned_rules(
    host: &dyn Host,
    found: &mut Observed,
    answer: &mut Option<Result<Vec<Found>, PluginError>>,
) -> Result<(), PluginError> {
    let filled = values(found);
    let named: Vec<String> = (found.checks().iter())
        .filter(|each| each.kind.known() == Some(&CheckKind::PlannedRulePaths))
        .flat_map(|each| named_rules(each, &filled))
        .collect();
    for rule in named {
        if listed_by(host, &rule, &[])?.is_some() {
            found.held.insert(rule);
        }
    }
    match answered(host, &found.change, answer)? {
        Ok(rules) => found.on_planned = names(rules),
        Err(error) => found.on_planned_unread = Some(error.message.clone()),
    }
    Ok(())
}

/// Adds the paths of the plan the change names and the files the change's diff edits
/// outside them to `found`, or why the diff cannot be read. The gate's `paths`
/// check and the pull request body both read them from here.
pub(crate) fn reviewed(host: &dyn Host, found: &mut Observed) -> Result<(), PluginError> {
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

#[cfg(test)]
mod tests;
