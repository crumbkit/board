//! What a change's gate reads beyond the change: `observe` fills the
//! gate's input with what the checks of its phase need, by their kinds.

use board_ops::{Host, PluginError};

use crate::approve::approval;
use crate::board::{self, get_text};
use crate::change::{
    NOT_FOUND, attached, current, kind_of, listed, plan_at, pull_of, referenced, whole_record_at,
};
use crate::diff::{edited, outside};
use crate::flow::Kind as CheckKind;
use crate::flow::of_change;
use crate::gates::kinds::named_rules;
use crate::gates::remedy::{fill, unfilled, values};
use crate::gates::{Observed, Read, reads};
use crate::records::ChangeRecord;
use crate::repo::check_suite;
use crate::rules::{listed_by, obligations, on_paths};

use super::plan_file;

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
        let named = (change.plan.as_deref().map(|plan| plan_at(host, plan))).transpose()?;
        if let Some((record, _)) = named.flatten() {
            found.answers = record.answers.unwrap_or_default();
            found.uses = record.uses;
        }
        (found.considered, found.owed) = listed(host, change)?;
        found.surfaced_gone = (referenced(host, &change.surfaced)?.unreadable.into_iter())
            .filter(|(_, code)| code == NOT_FOUND)
            .map(|(path, _)| path)
            .collect();
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
        planned_rules(host, &mut found)?;
    }
    if wanted.contains(&Read::Approval) {
        found.approval = approval(host, &found)?;
    }
    Ok(found)
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

/// What `planned-rule-paths` checks read: which rules they name the board
/// holds, and the rules on the plan's paths, or why those cannot be read.
fn planned_rules(host: &dyn Host, found: &mut Observed) -> Result<(), PluginError> {
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
    match on_paths(host, &current(host, &found.change)?.2) {
        Ok(rules) => found.on_planned = rules,
        Err(error) => found.on_planned_unread = Some(error.message),
    }
    Ok(())
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
