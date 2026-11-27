//! Which revision of the change's plan each owner approval approved, and
//! whether the current plan differs from it in substance.
//!
//! Only an explicit approval of the plan counts, and it names the revision
//! it approves: a decision whose refs name the change and that decides a
//! revision; an answer that chose the `approve` option of the approval
//! question `changes ask` filed, which names the change and a revision; or
//! a Touch ID approval of the revision it signed. Any other decision or
//! answer approves nothing. An approval of a revision Board no longer reads
//! lapses. The plan differs in substance when its typed answers differ, or
//! the text of its Outcome, Out of scope, UX or Data section differs once
//! runs of whitespace are collapsed; anything else is routine.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::kinds::{ANSWER_TYPE, QUESTION_TYPE, answers_count, meets, names_whole};
use super::{Approval, HEADING, Observed, Revision, sections};
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
/// The id of the option that approves the plan in the question `changes
/// ask` files.
pub(crate) const APPROVE: &str = "approve";
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
    /// It approved a revision Board no longer reads, `named` as a detail
    /// names it.
    Gone { named: String },
}

impl Standing {
    /// Whether the approval still counts.
    pub(crate) fn counts(&self) -> bool {
        !matches!(self, Self::Lapsed { .. } | Self::Gone { .. })
    }
}

/// The revision the owner's decision or answer `body` approves, the one
/// helper both routes go through. A decision approves the revision its
/// `decides` names, as a `<change>/plans/<id>` record or its `.md` file;
/// a decision that decides anything else approves nothing. An answer
/// approves the revision the `approve` option of the approval question
/// `board changes ask` files names: its question is attached and blocks the
/// change, its choice is that option, and the option names the change and
/// a revision. Any other answer, such as a choice to revise or a choice in
/// a question about the plan's content, approves nothing.
pub(crate) fn approved(
    observed: &Observed,
    body: &Map<String, Value>,
    answer: bool,
) -> Option<String> {
    let field = |key| body.get(key).and_then(Value::as_str);
    if !answer {
        let decides = field("decides")?;
        let revision = named_in(observed, decides)?;
        return (decides.strip_suffix(".md").unwrap_or(decides) == revision).then_some(revision);
    }
    if field("choice")? != APPROVE {
        return None;
    }
    let question = field("question")?;
    let (_, asked) =
        (observed.records.of_type(QUESTION_TYPE)).find(|(path, _)| *path == question)?;
    if asked.get("blocks").and_then(Value::as_str) != Some(observed.path.as_str()) {
        return None;
    }
    (asked.get("options").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter(|option| option.get("id").and_then(Value::as_str) == Some(APPROVE))
        .filter_map(|option| option.get("text").and_then(Value::as_str))
        .filter(|text| names_whole(text, &observed.path))
        .find_map(|text| named_in(observed, text))
}

/// The revision of the change's plan `text` names as a whole path: a
/// `<change>/plans/<id>` record or its `.md` file. Of several, the last.
fn named_in(observed: &Observed, text: &str) -> Option<String> {
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

/// Where an approval of `revision` stands against the current plan.
fn standing(observed: &Observed, revision: String) -> Standing {
    if observed.change.plan.as_deref() == Some(revision.as_str()) {
        return Standing::Current;
    }
    let revisions = observed.revisions.as_deref().unwrap_or_default();
    let Some(earlier) = (revisions.iter())
        .find(|each| each.path == revision)
        .filter(|each| each.text.is_some())
    else {
        return Standing::Gone {
            named: short(observed, &revision).to_owned(),
        };
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

/// Each section of `was` or `now`, as `sections` reads them, other than
/// the title and those an approval covers, whose text differs once
/// whitespace is collapsed.
fn routine_sections(was: &str, now: &str) -> Vec<String> {
    let (was_found, now_found) = (sections(was), sections(now));
    let titles = [title(was), title(now)];
    let text = |found: &BTreeMap<String, String>, key: &str| {
        collapsed(found.get(key).map_or("", String::as_str))
    };
    let mut keys: Vec<&String> = now_found.keys().collect();
    keys.extend(was_found.keys().filter(|key| !now_found.contains_key(*key)));
    keys.sort();
    (keys.into_iter())
        .filter(|key| {
            !SUBSTANCE.iter().any(|(each, _)| each == key)
                && !titles.iter().any(|title| title.as_ref() == Some(*key))
                && text(&was_found, key) != text(&now_found, key)
        })
        .map(|key| {
            let mut letters = key.chars();
            let first = letters
                .next()
                .map(|each| each.to_uppercase().collect::<String>());
            format!(
                "its {}{} section",
                first.unwrap_or_default(),
                letters.as_str()
            )
        })
        .collect()
}

/// The plan's title, its first `# ` heading, in lower case as `sections`
/// keys it.
fn title(text: &str) -> Option<String> {
    let line = text.lines().find(|line| line.starts_with("# "))?;
    let heading = HEADING.captures(line)?.get(1)?;
    Some(heading.as_str().to_lowercase())
}

/// `text` with each run of whitespace one space, trimmed.
fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
/// by path, with whether it is an answer. Such a check counts only a
/// decision that approves the plan explicitly.
pub(super) fn meeting<'a>(
    observed: &'a Observed,
    each: &'a flow::Check,
    rules: &'a Where,
    head: Option<&'a str>,
) -> impl Iterator<Item = (&'a str, &'a Map<String, Value>, bool)> {
    let kind = each.record_type.as_deref().unwrap_or_default();
    let owners = answers_count(each);
    let answers = owners.then(|| observed.records.of_type(ANSWER_TYPE));
    (observed.records.of_type(kind))
        .filter(move |(_, body)| meets(observed, body, rules, head, false))
        .filter(move |(_, body)| !owners || approved(observed, body, false).is_some())
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
        .filter_map(|(path, body, answer)| {
            let revision = approved(observed, body, answer)?;
            Some((path.to_owned(), standing(observed, revision)))
        })
        .collect();
    if let Approval::Signed(signed) = &observed.approval {
        let standing = standing(observed, signed.clone());
        judged.push((TOUCH_ID.to_owned(), standing));
    }
    judged
}

/// What a missing check that counts the owner's answers says when no
/// approval lapsed: the explicit approvals it counts, with why Board could
/// not read the Touch ID approval.
pub(super) fn unmet(observed: &Observed) -> String {
    let path = &observed.path;
    let plan = (observed.change.plan.clone()).unwrap_or_else(|| format!("{path}/plans/<id>"));
    let unread = match &observed.approval {
        Approval::Unread(refusal) => format!(
            "; Board could not read that approval: {} {}",
            refusal.code.as_str(),
            refusal.message
        ),
        Approval::None | Approval::Signed(_) => String::new(),
    };
    format!(
        "no plans/decision/v1 that the owner signed with by.person whose refs name {path} and \
         that decides a revision of the plan, such as {plan}, is attached, nor a {ANSWER_TYPE} \
         from the owner (one that names `by.person`, or was written through the owner's \
         questions pane, Board.app or /ask) that chose the `approve` option of an attached \
         question blocking {path}, whose text names {path} and a revision of the plan, as the \
         question `board changes ask` files does, nor the owner's Touch ID approval of \
         changes.approve for the current plan{unread}"
    )
}

/// What a passing owner-approval check says of the approvals that count,
/// each by the record that holds it: nothing when one approved the current
/// revision, else that the newest earlier one still counts and what
/// changed since.
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
    routine.map_or_else(String::new, |(by, revision, changed)| {
        let (revision, by) = (short(observed, revision), source(by));
        if changed.is_empty() {
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
        }
    })
}

/// What a missing owner-approval check says when approvals lapsed: each
/// revision approved and what the current plan changes of it, newest
/// first, and how to ask the owner again; none when none lapsed.
pub(crate) fn lapsed(observed: &Observed, judged: &[(String, Standing)]) -> Option<String> {
    let mut said: Vec<(&str, String)> = Vec::new();
    for (by, each) in judged {
        let (key, sentence) = match each {
            Standing::Lapsed { revision, changed } => (
                revision.as_str(),
                format!(
                    "the owner approved {} {}, and the current plan changes {}",
                    short(observed, revision),
                    source(by),
                    listed(changed)
                ),
            ),
            Standing::Gone { named } => (
                named.as_str(),
                format!(
                    "the owner approved {named} {}, which Board can no longer read",
                    source(by)
                ),
            ),
            _ => continue,
        };
        if !said.iter().any(|(each, _)| *each == key) {
            said.push((key, sentence));
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
