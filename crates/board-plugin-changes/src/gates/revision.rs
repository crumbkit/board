//! Which revision of the change's plan each owner approval approved, and
//! whether the current plan differs from it in substance.
//!
//! A decision whose refs name a revision, or an answer whose chosen option
//! names one, approves that revision. Any other approval approves the
//! earliest revision whose `records` list holds it, the one current when it
//! was attached. A Touch ID approval approves the revision it signed. The
//! plan differs in substance when its typed answers differ, or the text of
//! its Outcome, Out of scope, UX or Data section differs once runs of
//! whitespace are collapsed; anything else is routine.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::kinds::{ANSWER_TYPE, answers_count, chosen_text, meets};
use super::{Approval, FENCES, HEADING, Observed, Revision, sections, strings};
use crate::flow::{self, Where};

/// The sections whose text an approval covers, by heading in lower case,
/// with the heading as a reader writes it.
const SUBSTANCE: [(&str, &str); 4] = [
    ("outcome", "Outcome"),
    ("out of scope", "Out of scope"),
    ("ux", "UX"),
    ("data", "Data"),
];
/// The typed answers an approval covers.
const ANSWERS: [&str; 3] = ["ux", "data", "lifecycle"];
/// The plan record's fields that change routinely.
const ROUTINE: [&str; 5] = ["paths", "owed", "uses", "considered", "lifecycle"];
/// What a detail names the owner's Touch ID approval by.
pub(crate) const TOUCH_ID: &str = "Touch ID";

/// Where one approval stands against the current plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Standing {
    /// It approved the current revision.
    Current,
    /// It approved an earlier revision, and the current plan changes only
    /// the routine parts `changed` since.
    Routine {
        revision: String,
        changed: Vec<String>,
    },
    /// It approved an earlier revision, and the current plan changes the
    /// parts `changed`, which the approval covers.
    Lapsed {
        revision: String,
        changed: Vec<String>,
    },
    /// Board cannot tell which revision it approved.
    Unbound,
    /// It names a revision Board does not read.
    Unread { revision: String },
}

impl Standing {
    /// Whether the approval still counts.
    pub(crate) fn counts(&self) -> bool {
        !matches!(self, Self::Lapsed { .. })
    }
}

/// The revision of the change's plan `text` names as a whole path: a
/// `<change>/plans/<id>` record or its `.md` file. Of several, the last.
pub(crate) fn named_in(observed: &Observed, text: &str) -> Option<String> {
    let prefix = format!("{}/plans/", observed.path);
    let joined = |next: char| next.is_alphanumeric() || "-_/".contains(next);
    (text.match_indices(&prefix))
        .filter(|(at, _)| !text[..*at].ends_with(joined))
        .filter_map(|(at, _)| {
            let rest = &text[at + prefix.len()..];
            let id: String = (rest.chars())
                .take_while(|each| each.is_alphanumeric() || "-_".contains(*each))
                .collect();
            let after = &rest[id.len()..];
            let after = after.strip_prefix(".md").unwrap_or(after);
            let longer = after.starts_with(joined) || after.starts_with('#');
            (!id.is_empty() && !longer).then(|| format!("{prefix}{id}"))
        })
        .last()
}

/// The revision the attached record at `path` approved: the one its refs
/// name for a decision, or the one `chosen`, the text of the option an
/// answer chose, names; otherwise the earliest revision whose `records`
/// list holds it.
pub(crate) fn bound(
    observed: &Observed,
    path: &str,
    body: &Map<String, Value>,
    chosen: Option<&str>,
) -> Option<String> {
    let named = match chosen {
        Some(text) => named_in(observed, text),
        None => strings(body, "refs")
            .filter_map(|each| named_in(observed, each))
            .last(),
    };
    named.or_else(|| {
        let revisions = observed.revisions.as_deref().unwrap_or_default();
        (revisions.iter())
            .find(|each| strings(&each.record, "records").any(|held| held == path))
            .map(|each| each.path.clone())
    })
}

/// Where an approval of `revision` stands against the current plan.
pub(crate) fn standing(observed: &Observed, revision: Option<String>) -> Standing {
    let Some(revision) = revision else {
        return Standing::Unbound;
    };
    if observed.change.plan.as_deref() == Some(revision.as_str()) {
        return Standing::Current;
    }
    let revisions = observed.revisions.as_deref().unwrap_or_default();
    let Some(earlier) = (revisions.iter())
        .find(|each| each.path == revision)
        .filter(|each| each.text.is_some())
    else {
        return Standing::Unread { revision };
    };
    let (substance, routine) = differences(earlier, &current(observed));
    if substance.is_empty() {
        Standing::Routine {
            revision,
            changed: routine,
        }
    } else {
        Standing::Lapsed {
            revision,
            changed: substance,
        }
    }
}

/// The current revision: the one the change names, as Board listed it, or
/// else the plan text and answers the gate read.
fn current(observed: &Observed) -> Revision {
    let named = observed.change.plan.clone().unwrap_or_default();
    let revisions = observed.revisions.as_deref().unwrap_or_default();
    (revisions.iter().find(|each| each.path == named).cloned()).unwrap_or_else(|| {
        let mut record = Map::new();
        record.insert(
            "answers".to_owned(),
            serde_json::to_value(observed.answers).unwrap_or_default(),
        );
        Revision {
            path: named,
            record,
            text: observed.plan.clone(),
        }
    })
}

/// What `current` changes of `earlier`: the parts an approval covers, and
/// the routine parts, each as a detail names it.
fn differences(earlier: &Revision, current: &Revision) -> (Vec<String>, Vec<String>) {
    let (was, now) = (
        earlier.text.as_deref().unwrap_or_default(),
        current.text.as_deref().unwrap_or_default(),
    );
    let (was_found, now_found) = (sections(was), sections(now));
    let section = |found: &BTreeMap<String, String>, key: &str| {
        collapsed(found.get(key).map_or("", String::as_str))
    };
    let mut substance: Vec<String> = (SUBSTANCE.iter())
        .filter(|(key, _)| section(&was_found, key) != section(&now_found, key))
        .map(|(_, heading)| format!("its {heading} section"))
        .collect();
    let answer = |revision: &Revision, key: &str| {
        (revision.record.get("answers"))
            .and_then(|answers| answers.get(key))
            .cloned()
            .unwrap_or(Value::Null)
    };
    substance.extend(
        (ANSWERS.iter())
            .filter(|key| answer(earlier, key) != answer(current, key))
            .map(|key| format!("its `{key}` answer")),
    );
    let field = |revision: &Revision, key: &str| match revision.record.get(key) {
        None | Some(Value::Null) => Value::Array(Vec::new()),
        Some(value) => value.clone(),
    };
    let mut routine: Vec<String> = (ROUTINE.iter())
        .filter(|key| field(earlier, key) != field(current, key))
        .map(|key| format!("its `{key}`"))
        .collect();
    routine.extend(routine_sections(was, now));
    (substance, routine)
}

/// Each `## ` section of `was` or `now`, other than those an approval
/// covers, whose text differs once whitespace is collapsed, and the title
/// when the text before the first such section differs.
fn routine_sections(was: &str, now: &str) -> Vec<String> {
    let ((was_before, was_parts), (now_before, now_parts)) = (parts(was), parts(now));
    let mut changed = Vec::new();
    if collapsed(&was_before) != collapsed(&now_before) {
        changed.push("its title".to_owned());
    }
    let mut headings: Vec<&String> = now_parts.iter().map(|(heading, _)| heading).collect();
    for (heading, _) in &was_parts {
        if !headings.contains(&heading) {
            headings.push(heading);
        }
    }
    let body = |found: &[(String, String)], heading: &str| {
        (found.iter())
            .find(|(each, _)| each == heading)
            .map_or_else(String::new, |(_, text)| collapsed(text))
    };
    changed.extend(
        (headings.into_iter())
            .filter(|heading| {
                let key = heading.to_lowercase();
                !SUBSTANCE.iter().any(|(each, _)| *each == key)
                    && body(&was_parts, heading) != body(&now_parts, heading)
            })
            .map(|heading| format!("its {heading} section")),
    );
    changed
}

/// `text` with each run of whitespace one space, trimmed.
fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text before a plan's first `## ` heading, and each `## ` section by
/// its heading, with its subsections. A line in a code fence is never a
/// heading.
fn parts(text: &str) -> (String, Vec<(String, String)>) {
    let mut before = Vec::new();
    let mut found: Vec<(String, Vec<&str>)> = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let heading = (!fenced && line.starts_with("## "))
            .then(|| HEADING.captures(line).and_then(|captures| captures.get(1)))
            .flatten();
        if FENCES
            .iter()
            .any(|fence| line.trim_start().starts_with(fence))
        {
            fenced = !fenced;
        }
        match (heading, found.last_mut()) {
            (Some(heading), _) => found.push((heading.as_str().to_owned(), Vec::new())),
            (None, Some((_, lines))) => lines.push(line),
            (None, None) => before.push(line),
        }
    }
    let found = (found.into_iter())
        .map(|(heading, lines)| (heading, lines.join("\n")))
        .collect();
    (before.join("\n"), found)
}

/// `items` joined as a reader lists them: `a`, `a and b`, `a, b and c`.
fn listed(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// `revision` as a detail names it, relative to the change.
fn short<'a>(observed: &Observed, revision: &'a str) -> &'a str {
    (revision.strip_prefix(observed.path.as_str()))
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(revision)
}

/// How a detail says where the approval `by` is: in the record that holds
/// it, or with Touch ID.
fn source(by: &str) -> String {
    if by == TOUCH_ID {
        "with Touch ID".to_owned()
    } else {
        format!("in {by}")
    }
}

/// The `where` of a check that names none.
pub(super) const NONE: Where = Where {
    at_head: None,
    values: None,
    no_findings: None,
    owner_signed: None,
    refs: None,
    unchanged_by: None,
};

/// Each attached record of the check's type, and for a check that counts
/// the owner's answers each such answer, that meets its `where` at `head`,
/// by path, with whether it is an answer.
pub(super) fn meeting<'a>(
    observed: &'a Observed,
    each: &'a flow::Check,
    rules: &'a Where,
    head: Option<&'a str>,
) -> impl Iterator<Item = (&'a str, &'a Map<String, Value>, bool)> {
    let kind = each.record_type.as_deref().unwrap_or_default();
    let answers = answers_count(each).then(|| observed.records.of_type(ANSWER_TYPE));
    (observed.records.of_type(kind))
        .filter(move |(_, body)| meets(observed, body, rules, head, false))
        .map(|(path, body)| (path, body, false))
        .chain(
            (answers.into_iter().flatten())
                .filter(move |(_, body)| meets(observed, body, rules, head, true))
                .map(|(path, body)| (path, body, true)),
        )
}

/// Where each approval a check that counts the owner's answers reads
/// stands against the current plan, by the record that holds it or
/// [`TOUCH_ID`]: each record that meets its `where`, and the owner's Touch
/// ID approval.
pub(super) fn approvals(
    observed: &Observed,
    each: &flow::Check,
    rules: &Where,
    head: Option<&str>,
) -> Vec<(String, Standing)> {
    let mut judged: Vec<(String, Standing)> = meeting(observed, each, rules, head)
        .map(|(path, body, answer)| {
            let chosen = answer.then(|| chosen_text(observed, body)).flatten();
            let bound = bound(observed, path, body, chosen);
            (path.to_owned(), standing(observed, bound))
        })
        .collect();
    if let Approval::Signed(signed) = &observed.approval {
        let standing = standing(observed, Some(signed.clone()));
        judged.push((TOUCH_ID.to_owned(), standing));
    }
    judged
}

/// What a passing check that counts the owner's answers says of the
/// approvals that count: see [`passed`].
pub(super) fn passed_approval(observed: &Observed, each: &flow::Check) -> String {
    let rules = each.conditions.as_ref().unwrap_or(&NONE);
    let head = rules.at_head.and(observed.change.head.as_deref());
    let counted: Vec<(String, Standing)> = (approvals(observed, each, rules, head).into_iter())
        .filter(|(_, standing)| standing.counts())
        .collect();
    passed(observed, &counted)
}

/// What a missing check that counts the owner's answers says when no
/// approval lapsed: `detail`, the records it lacks, and the answers and
/// Touch ID approval it also counts, with why Board could not read that
/// approval.
pub(super) fn unmet(observed: &Observed, detail: &str) -> String {
    let path = &observed.path;
    let unread = match &observed.approval {
        Approval::Unread(refusal) => format!(
            "; Board could not read that approval: {} {}",
            refusal.code.as_str(),
            refusal.message
        ),
        Approval::None | Approval::Signed(_) => String::new(),
    };
    format!(
        "{detail}, nor a {ANSWER_TYPE} from the owner (one that names `by.person`, or was written \
         through the owner's questions pane, Board.app or /ask) whose chosen option, in its \
         attached question, names {path}, nor the owner's Touch ID approval of changes.approve \
         for the current plan{unread}"
    )
}

/// What a passing owner-approval check says of the approvals that count,
/// each by the record that holds it: nothing when one approved the current
/// revision, else that the newest earlier one still counts and what
/// changed since, else why Board could not tell.
pub(crate) fn passed(observed: &Observed, counted: &[(String, Standing)]) -> String {
    if counted.iter().any(|(_, each)| *each == Standing::Current) {
        return String::new();
    }
    let routine = (counted.iter())
        .filter_map(|(by, each)| match each {
            Standing::Routine { revision, changed } => Some((by, revision, changed)),
            _ => None,
        })
        .max_by(|one, other| one.1.cmp(other.1));
    if let Some((by, revision, changed)) = routine {
        let (revision, by) = (short(observed, revision), source(by));
        return if changed.is_empty() {
            format!(
                "the owner approved {revision} {by}, and the current plan changes nothing that \
                 needs a new approval"
            )
        } else {
            format!(
                "the owner approved {revision} {by}, and the current plan changes only {} since, \
                 which needs no new approval",
                listed(changed)
            )
        };
    }
    for (by, each) in counted {
        if let Standing::Unread { revision } = each {
            return format!(
                "{by} approved {}, which Board could not read, so Board cannot tell whether the \
                 plan changed since; it counts the approval",
                short(observed, revision)
            );
        }
    }
    (counted.iter())
        .find(|(_, each)| *each == Standing::Unbound)
        .map_or_else(String::new, |(by, _)| {
            format!(
                "Board could not tell which plan revision {by} approved, so it counts it as \
                 approving the current plan"
            )
        })
}

/// What a missing owner-approval check says when approvals lapsed: each
/// revision approved and what the current plan changes of it, newest
/// first, and how to ask the owner again; none when none lapsed.
pub(crate) fn lapsed(observed: &Observed, judged: &[(String, Standing)]) -> Option<String> {
    let mut said: Vec<(&str, String)> = Vec::new();
    for (by, each) in judged {
        if let Standing::Lapsed { revision, changed } = each
            && !said.iter().any(|(each, _)| each == revision)
        {
            let sentence = format!(
                "the owner approved {} {}, and the current plan changes {}",
                short(observed, revision),
                source(by),
                listed(changed)
            );
            said.push((revision, sentence));
        }
    }
    if said.is_empty() {
        return None;
    }
    said.sort_by(|one, other| other.0.cmp(one.0));
    let sentences: Vec<String> = said.into_iter().map(|(_, sentence)| sentence).collect();
    Some(format!(
        "{}; ask the owner again with the question `board changes ask` files, or with `board \
         changes approve` and Touch ID",
        sentences.join("; ")
    ))
}

/// What a missing owner-approval check says when Board refused to list
/// the plan's revisions.
pub(crate) fn unlisted(observed: &Observed, refusal: &str) -> String {
    format!(
        "Board could not list the revisions of the plan of {}, so it cannot tell which one the \
         owner approved: {refusal}",
        observed.path
    )
}
