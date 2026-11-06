//! The `field` and `records` check kinds.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{
    NO_HEAD, Observed, objects, reviewed_head, route_missing, same_commit, strings, task_missing,
    truthy, unset,
};
use crate::flow::{self, Field, Named, Target, Where};

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
            each.types.as_ref().map(|_| {
                format!(
                    "this Board does not check the types of a change's tickets yet, so the flow's \
                     check {} cannot pass; ask the owner whether to drop its types",
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

#[cfg(test)]
mod tests;
