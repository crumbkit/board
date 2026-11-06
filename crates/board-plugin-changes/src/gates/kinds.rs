//! The `field`, `records`, `understood`, `considered`, `obligations`,
//! `controlled-paths`, `flow-valid` and `exists` check kinds.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::remedy::{Values, fill, unfilled};
use super::{
    NO_HEAD, Observed, UNDERSTANDING_TYPE, objects, pattern, reviewed_head, route_missing,
    same_commit, signed_decisions, strings, task_missing, truthy, unset,
};
use board_plugin::shape::Record;

use crate::flow::{self, Field, Flow, Kind, Named, Target, Where, problems};
use crate::records::Subject;

/// A board path in prose: a `/` and the characters up to a space, a quote,
/// a bracket or a list's punctuation.
static BOARD_PATH: LazyLock<Regex> = LazyLock::new(|| pattern(r#"/[^\s`'")>\],;]+"#));

/// The board paths `text` names, each without a trailing full stop or
/// colon.
pub(super) fn paths_in(text: &str) -> BTreeSet<&str> {
    (BOARD_PATH.find_iter(text))
        .map(|found| found.as_str().trim_end_matches(['.', ':']))
        .collect()
}

/// What a passing check says: for a `controlled-paths` check whose
/// controls the board does not hold, that none exists, and otherwise
/// nothing.
pub(super) fn passed(observed: &Observed, each: &flow::Check, values: &Values) -> String {
    if each.kind.known().copied() != Some(Kind::ControlledPaths) {
        return String::new();
    }
    let named = named_controls(each, values);
    if named.is_empty()
        || named.iter().any(|control| {
            observed
                .controlled
                .get(control)
                .is_some_and(Option::is_some)
        })
    {
        return String::new();
    }
    format!(
        "no control {} exists, so the board names no such paths and the check passes",
        named.join(" or ")
    )
}

/// The controls a `controlled-paths` check names, with their placeholders
/// filled.
pub(crate) fn named_controls(each: &flow::Check, values: &Values) -> Vec<String> {
    (each.controls.iter().flatten())
        .map(|control| fill(control, values))
        .collect()
}

/// Every record the change's `surfaced` lists is in the plan's `uses` or
/// named under a `Considered` heading.
pub(super) fn considered_missing(
    observed: &Observed,
    plan: &BTreeMap<String, String>,
) -> Option<String> {
    let listed = paths_in(plan.get("considered").map_or("", String::as_str));
    let left: Vec<&str> = (observed.change.surfaced.iter().map(String::as_str))
        .filter(|each| !observed.uses.iter().any(|used| used == each) && !listed.contains(each))
        .collect();
    (!left.is_empty()).then(|| {
        format!(
            "the plan neither uses nor lists under ### Considered these records that start \
             surfaced: {}",
            left.join(", ")
        )
    })
}

/// No obligation control applies to the plan's paths, except those named
/// under the plan's `escape_section` heading.
pub(super) fn obligations_missing(
    observed: &Observed,
    plan: &BTreeMap<String, String>,
    each: &flow::Check,
) -> Option<String> {
    let owed = match &observed.obligations {
        Ok(owed) => owed,
        Err(why) => return Some(why.clone()),
    };
    let escaped = (each.escape_section.as_ref())
        .map(|heading| paths_in(plan.get(&heading.to_lowercase()).map_or("", String::as_str)))
        .unwrap_or_default();
    let left: Vec<&str> = (owed.iter().map(String::as_str))
        .filter(|control| !escaped.contains(control))
        .collect();
    (!left.is_empty()).then(|| {
        format!(
            "these obligations apply to the plan's paths: {}",
            left.join(", ")
        )
    })
}

/// The change's diff touches no path the named controls list. A control
/// the board does not hold lists nothing.
pub(super) fn controlled_missing(observed: &Observed, each: &flow::Check) -> Option<String> {
    if each.controls.is_none() {
        return Some(unset(each, "controls"));
    }
    let named = named_controls(each, &super::remedy::values(observed));
    let held: Vec<&Vec<String>> = (named.iter())
        .filter_map(|control| observed.controlled.get(control)?.as_ref())
        .collect();
    if held.is_empty() {
        return None;
    }
    if let Some(unread) = &observed.diff_unread {
        return Some(unread.clone());
    }
    let touched: BTreeSet<&str> = held.into_iter().flatten().map(String::as_str).collect();
    (!touched.is_empty()).then(|| {
        format!(
            "the change's diff edits files that {} list: {}",
            named.join(" or "),
            touched.into_iter().collect::<Vec<_>>().join(", ")
        )
    })
}

pub(super) fn field_missing(observed: &Observed, each: &flow::Check) -> Option<String> {
    let Some(field) = each.field else {
        return Some(unset(each, "field"));
    };
    let change = &observed.change;
    let min = each.min.map_or(1, |min| min as usize);
    match field {
        Field::Task => task_missing(observed),
        Field::Route => route_missing(observed),
        Field::Head => change.head.is_none().then(|| NO_HEAD.to_owned()),
        Field::Plan => {
            (change.plan.is_none()).then(|| "the change names no plan record".to_owned())
        }
        Field::Pulls => pulls_missing(observed, each, min),
        Field::Tickets => {
            let held = change.tickets.as_deref().unwrap_or_default().len();
            if held < min {
                return Some(format!(
                    "the change references {held} tickets, and the flow's check {} needs {min}; \
                     record a ticket with `board call jira.ref` or `board call github.ref` and \
                     attach it with `board call changes.attach --path {} --ticket <path>`",
                    each.id, observed.path
                ));
            }
            let wanted = each.types.as_ref()?;
            let typed = (observed.ticket_types.iter())
                .filter(|found| wanted.contains(found))
                .count();
            (typed < min).then(|| {
                format!(
                    "the change references {typed} tickets of type {}, and the flow's check {} \
                     needs {min}",
                    either(wanted),
                    each.id
                )
            })
        }
    }
}

fn pulls_missing(observed: &Observed, each: &flow::Check, min: usize) -> Option<String> {
    let pulls = observed.change.pulls();
    if pulls.is_empty() {
        return Some(
            "the change references no github/pull/v1 record; record the pull request with `board \
             call github.ref` and attach it with --pull"
                .to_owned(),
        );
    }
    if pulls.len() < min {
        return Some(format!(
            "the change references {} pull requests, and the flow's check {} needs {min}; record \
             each with `board call github.ref` and attach it with --pull",
            pulls.len(),
            each.id
        ));
    }
    let newest = pulls.last().map_or("", String::as_str);
    (each.types.is_some() && !observed.pull_read).then(|| {
        format!(
            "the change's newest pull request {newest} does not read as a github/pull/v1 record; \
             run `board check {newest}`, or record the pull request with `board call github.ref` \
             and attach it with --pull"
        )
    })
}

/// Whether `each`, one of a record's refs, names `target`: the plan names
/// the change's plan record, its text or the change; the change names the
/// change; a type names an attached record of that type.
fn names(observed: &Observed, target: &Target, each: &str) -> bool {
    match target {
        Target::Named(Named::Plan) => {
            each == observed.path
                || Some(each) == observed.change.plan.as_deref()
                || Some(each) == observed.plan_file.as_deref()
        }
        Target::Named(Named::Change) => each == observed.path,
        Target::Type(kind) => (observed.records.of_type(kind)).any(|(path, _)| path == each),
    }
}

/// Whether the record `body` meets every condition of `rules`, with the
/// change's `head` when they name one.
fn meets(
    observed: &Observed,
    body: &Map<String, Value>,
    rules: &Where,
    head: Option<&str>,
) -> bool {
    if let Some(head) = head
        && !same_commit(reviewed_head(body).as_ref(), head)
    {
        return false;
    }
    let values = (rules.values.iter().flatten()).all(|(field, allowed)| {
        (allowed.iter()).any(|value| body.get(field) == Some(&value.json()))
    });
    let severities = rules.no_findings.as_deref().unwrap_or_default();
    let clean = !objects(body, "findings").any(|finding| {
        (finding.get("severity").and_then(Value::as_str))
            .is_some_and(|severity| severities.iter().any(|each| each.as_str() == severity))
    });
    let signed = rules.owner_signed != Some(true) || !truthy(body.get("written_by"));
    let refs = (rules.refs.iter().flatten())
        .all(|target| strings(body, "refs").any(|each| names(observed, target, each)));
    values && clean && signed && refs
}

/// `items` joined as a reader says a choice: `a`, `a or b`, `a, b or c`.
fn either(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// At least `min` attached records of the check's type meet its `where`,
/// each from a different actor when it names `distinct`.
pub(super) fn records_missing(observed: &Observed, each: &flow::Check) -> Option<String> {
    let Some(kind) = &each.record_type else {
        return Some(unset(each, "type"));
    };
    let none = Where {
        at_head: None,
        values: None,
        no_findings: None,
        owner_signed: None,
        refs: None,
    };
    let rules = each.conditions.as_ref().unwrap_or(&none);
    let head = match (rules.at_head, &observed.change.head) {
        (None, _) => None,
        (Some(_), None) => return Some(NO_HEAD.to_owned()),
        (Some(_), Some(head)) => Some(head.as_str()),
    };
    let passing =
        (observed.records.of_type(kind)).filter(|(_, body)| meets(observed, body, rules, head));
    let count = match each.distinct {
        None => passing.count(),
        Some(part) => (passing
            .filter_map(|(_, body)| body.get("by")?.get(part.part())?.as_str())
            .collect::<BTreeSet<_>>())
        .len(),
    };
    let min = each.min.map_or(1, |min| min as usize);
    (count < min).then(|| records_detail(each, kind, rules, head, min))
}

/// What is missing when too few records meet a `records` check.
fn records_detail(
    each: &flow::Check,
    kind: &str,
    rules: &Where,
    head: Option<&str>,
    min: usize,
) -> String {
    let mut with = Vec::new();
    for (field, allowed) in rules.values.iter().flatten() {
        let words: Vec<String> = (allowed.iter())
            .map(|value| match value.json() {
                Value::String(word) => word,
                other => other.to_string(),
            })
            .collect();
        with.push(format!("a {field} of {}", either(&words)));
    }
    if let Some(severities) = &rules.no_findings {
        let words: Vec<String> = severities
            .iter()
            .map(|each| each.as_str().to_owned())
            .collect();
        with.push(format!("no {} finding", either(&words)));
    }
    let with = if with.is_empty() {
        String::new()
    } else {
        format!(" with {}", with.join(" and "))
    };
    let targets: Vec<String> = (rules.refs.iter().flatten())
        .map(|target| match target {
            Target::Named(Named::Plan) => "the plan or the change".to_owned(),
            Target::Named(Named::Change) => "the change".to_owned(),
            Target::Type(kind) => format!("an attached {kind}"),
        })
        .collect();
    let whose = if targets.is_empty() {
        String::new()
    } else {
        format!(" whose refs name {}", targets.join(" and "))
    };
    let signed = if rules.owner_signed == Some(true) {
        " that the owner signed"
    } else {
        ""
    };
    let distinct = each.distinct.map_or(String::new(), |part| {
        format!(" from different {}s", part.part())
    });
    match (min, head) {
        (1, Some(head)) => format!("no attached {kind}{signed} covers {head}{whose}{with}"),
        (1, None) => format!("no {kind}{signed}{whose}{with} is attached"),
        (_, Some(head)) => format!(
            "fewer than {min} attached {kind} records{distinct}{signed} cover {head}{whose}{with}"
        ),
        (_, None) => {
            format!("fewer than {min} {kind} records{distinct}{signed}{whose}{with} are attached")
        }
    }
}

/// Whether an understanding check at `body` is of this change's
/// `subject`.
fn understands(observed: &Observed, body: &Map<String, Value>, subject: Subject) -> bool {
    body.get("change").and_then(Value::as_str) == Some(observed.path.as_str())
        && body.get("subject").and_then(Value::as_str) == Some(subject_word(subject))
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

/// An attached understanding check of the change's `subject` that the
/// owner signed off in a decision referencing it, and with `rechecks`, that
/// many matching rechecks on each attached understanding check of it.
pub(super) fn understood_missing(observed: &Observed, each: &flow::Check) -> Option<String> {
    let Some(subject) = each.subject else {
        return Some(unset(each, "subject"));
    };
    // An understanding check Board does not read can be neither signed off
    // nor counted, so it fails the check rather than drop out of it.
    for (path, code) in &observed.records.unreadable {
        if observed
            .records
            .unreadable_types
            .get(path)
            .map(String::as_str)
            == Some(UNDERSTANDING_TYPE)
        {
            return Some(format!(
                "the understanding check at {path} is degraded with {code}, so it cannot be \
                 read; run `board check {path}` and fix it"
            ));
        }
    }
    let understood: Vec<(&str, &Map<String, Value>)> =
        (observed.records.of_type(UNDERSTANDING_TYPE))
            .filter(|(_, body)| understands(observed, body, subject))
            .collect();
    let signed = (signed_decisions(observed)).any(|body| {
        strings(body, "refs").any(|named| understood.iter().any(|(path, _)| *path == named))
    });
    if !signed {
        return Some(format!(
            "no attached plans/decision/v1 the owner signed references an attached \
             {UNDERSTANDING_TYPE} of this change's {}; run the understanding-check skill, then \
             have the owner sign a decision whose refs name its record",
            subject_word(subject)
        ));
    }
    let wanted = each.rechecks? as usize;
    for (path, body) in understood {
        let matching = objects(body, "rechecks")
            .filter(|recheck| recheck.get("matches") == Some(&Value::Bool(true)))
            .count();
        if matching < wanted {
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

/// Something is at the path the check names, with its placeholders
/// filled; a placeholder with no value yet leaves the path unknown.
pub(super) fn exists_missing(observed: &Observed, each: &flow::Check) -> Option<String> {
    let Some(template) = &each.path else {
        return Some(unset(each, "path"));
    };
    let values = super::remedy::values(observed);
    let empty = unfilled(template, &values);
    if !empty.is_empty() {
        return Some(format!(
            "the path {template} names {}, which has no value yet, so where the record belongs \
             is not known",
            empty
                .iter()
                .map(|word| format!("{{{word}}}"))
                .collect::<Vec<_>>()
                .join(" and ")
        ));
    }
    let path = fill(template, &values);
    (observed.existing.get(&path) != Some(&true)).then(|| format!("nothing is at {path}"))
}

/// Each attached `changes/flow/v1` reads as a flow and has no problem a
/// drafted flow may not have; at least one is attached.
pub(super) fn flow_valid_missing(observed: &Observed) -> Option<String> {
    for (path, code) in &observed.records.unreadable {
        if observed
            .records
            .unreadable_types
            .get(path)
            .map(String::as_str)
            == Some(Flow::TYPE)
        {
            return Some(format!(
                "the drafted flow at {path} is degraded with {code}, so it cannot be read; run \
                 `board check {path}` and fix it"
            ));
        }
    }
    let mut said = Vec::new();
    let mut drafts = 0;
    for (path, body) in observed.records.of_type(Flow::TYPE) {
        drafts += 1;
        match serde_json::from_value::<Flow>(Value::Object(body.clone())) {
            Err(error) => said.push(format!("{path} does not read as a flow: {error}")),
            Ok(draft) => said.extend(
                (problems(&draft, true).iter()).map(|problem| format!("{path}: {problem}")),
            ),
        }
    }
    if drafts == 0 {
        return Some(format!("no {} draft is attached", Flow::TYPE));
    }
    (!said.is_empty()).then(|| said.join("; "))
}

#[cfg(test)]
mod tests;
