//! `changes.approve`: ask the owner to approve the change's current plan
//! with Touch ID, once the owner's attached answer chose an option that
//! names the change. The owner-approval check counts that signed approval,
//! which only the owner's key can make, where an answer's `by` and
//! `written_by` are claims any writer can make. The approval covers one
//! revision of the plan and its content: an edit to that revision after the
//! owner signs voids it, and a later revision keeps it only while it
//! changes nothing the approval covers (`gates::revision`).

use board_model::{Code, Digest, Refusal, canonical_json, parse_json};
use board_ops::{DecideReport, Host, PluginError, Shown};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::board::{get, get_text, logical, refused};
use crate::change::{attached, kind_of, read, referenced};
use crate::gates::kinds::{ANSWER_TYPE, chosen_text};
use crate::gates::{Approval, Observed};
use crate::phases::PathInput;
use crate::{PLUGIN, refusal, to_value};

/// The capability the owner approves.
const APPROVE: &str = "changes.approve";

/// Whether the owner has approved, refused or not yet answered.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum State {
    Asked,
    Approved,
    Refused,
}

/// The change, its plan, the answer the approval follows, the pending or
/// answered effect that asks the owner, and the owner's answer so far.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ApproveOutput")]
pub(crate) struct Output {
    path: String,
    plan: String,
    answer: String,
    /// The effect's id, which `board answer <effect> approve` answers.
    effect: Option<String>,
    state: State,
}

/// The plan as the owner approves it: its title, the first `# ` heading of
/// its text or else the change's title, and the SHA-256 of its record,
/// without the `records` that attaching adds, and of its text.
struct Content {
    title: String,
    digest: String,
}

/// What the Touch ID statement shows: the change, its plan and the plan's
/// content, and the answer the approval follows with the text of the
/// option the owner chose. The effect's id covers all of it.
fn shown(observed: &Observed, plan: &str, content: &Content, answer: &str, chosen: &str) -> Shown {
    let record = json!({
        "change": observed.path, "plan": plan, "plan_title": content.title,
        "plan_digest": content.digest, "answer": answer, "chosen": chosen,
    });
    Shown::Record(serde_json::to_vec(&record).expect("a record of strings serializes"))
}

/// The plan at `plan` as it is now.
fn content(host: &dyn Host, observed: &Observed, plan: &str) -> Result<Content, PluginError> {
    let bytes = get(host, &[plan.to_owned()])?.into_iter().next().flatten();
    let record = (bytes.and_then(|bytes| parse_json(&bytes).ok())).unwrap_or(Value::Null);
    let text = match record.get("text").and_then(Value::as_str) {
        Some(file) => get_text(host, file)?.unwrap_or_default(),
        None => String::new(),
    };
    Ok(content_of(observed, record, &text))
}

/// The plan whose record is `record` and whose text is `text`.
fn content_of(observed: &Observed, mut record: Value, text: &str) -> Content {
    if let Some(fields) = record.as_object_mut() {
        fields.remove("records");
    }
    let title = (text.lines())
        .find_map(|line| line.strip_prefix("# "))
        .map_or_else(
            || observed.change.title.clone(),
            |title| title.trim().to_owned(),
        );
    let whole = canonical_json(&json!({"record": record, "text": text}));
    let digest = Digest::of(whole.as_bytes()).to_string();
    Content { title, digest }
}

/// Each attached answer whose chosen option names the change, with that
/// option's text, in path order.
fn chosen(observed: &Observed) -> Vec<(&str, &str)> {
    (observed.records.of_type(ANSWER_TYPE))
        .filter_map(|(path, body)| Some((path, chosen_text(observed, body)?)))
        .collect()
}

/// Asks the owner, through `Host::decide`, to approve the change's current
/// plan and its content, following the last attached answer that chose an
/// option naming the change. Writes only the pending effect, and nothing
/// when the owner has answered. Refused with `changes::approve::not_chosen`
/// when no such answer is attached.
pub(crate) fn approve(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PathInput { path } = PluginError::read_input(PLUGIN, "approve", input)?;
    let path = path.trim_end_matches('/').to_owned();
    let (change, _) = read(host, &path)?;
    let records = referenced(host, &attached(host, &path)?)?;
    let observed = Observed::new(&path, change, kind_of(host, &path)?, records);
    let (plan, answer, option) = followed(&observed)?;
    let content = content(host, &observed, &plan)?;
    let at = logical(&path, "decide")?;
    let shown = shown(&observed, &plan, &content, answer, option);
    let decided = host.decide(APPROVE, &at, Some(&shown));
    Ok(to_value(outcome(
        path.clone(),
        plan,
        answer.to_owned(),
        decided,
    )?))
}

/// The change's plan, and the last attached answer whose chosen option
/// names the change with that option's text; otherwise the refusal. A
/// change with no plan holds no attached answer.
fn followed(observed: &Observed) -> Result<(String, &str, &str), PluginError> {
    let path = observed.path.as_str();
    let last = chosen(observed).pop();
    match (observed.change.plan.clone(), last) {
        (Some(plan), Some((answer, option))) => Ok((plan, answer, option)),
        _ => Err(refusal(
            "changes::approve::not_chosen",
            format!(
                "no attached {ANSWER_TYPE} chose an option, in its attached question, that names \
                 {path}; ask the owner with {path} in the approving option, and attach the \
                 question and the answer with `board changes attach --path {path} --ref <path>`"
            ),
            [("path", json!(path))],
        )),
    }
}

/// What the decide answered: approved, or asked or refused with the
/// effect's id. Any other refusal of Board's is passed on.
fn outcome(
    path: String,
    plan: String,
    answer: String,
    decided: Result<DecideReport, Box<Refusal>>,
) -> Result<Output, PluginError> {
    let (state, effect) = match decided {
        Ok(report) => (State::Approved, report.effect),
        Err(refusal) => {
            let state = match refusal.code.as_str() {
                code if code == Code::SettingsAsked.as_str() => State::Asked,
                code if code == Code::SettingsRefused.as_str() => State::Refused,
                _ => return Err(refused("decide", &refusal)),
            };
            let effect = refusal.fields.get("effect").and_then(Value::as_str);
            (state, effect.map(str::to_owned))
        }
    };
    Ok(Output {
        path,
        plan,
        answer,
        effect,
        state,
    })
}

/// What `Host::decided` finds of the owner's approval of a revision of the
/// change's plan and its content, following any attached answer that chose
/// an option naming the change: the newest revision the owner approved.
/// No personal board means no approval.
pub(crate) fn approval(host: &dyn Host, observed: &Observed) -> Result<Approval, PluginError> {
    let Some(plan) = &observed.change.plan else {
        return Ok(Approval::None);
    };
    let answers = chosen(observed);
    if answers.is_empty() {
        return Ok(Approval::None);
    }
    let at = logical(&observed.path, "decided")?;
    let revisions = revisions(host, observed, plan)?;
    Ok(signed(observed, &revisions, &answers, |shown| {
        host.decided(APPROVE, &at, Some(shown))
    }))
}

/// Each revision of the change's plan with its content, newest first: those
/// observe listed, and the current plan as it is now when they leave it
/// out.
fn revisions(
    host: &dyn Host,
    observed: &Observed,
    plan: &str,
) -> Result<Vec<(String, Content)>, PluginError> {
    let listed = observed.revisions.as_deref().unwrap_or_default();
    let mut found: Vec<(String, Content)> = (listed.iter().rev())
        .map(|each| {
            let text = each.text.as_deref().unwrap_or_default();
            let content = content_of(observed, Value::Object(each.record.clone()), text);
            (each.path.clone(), content)
        })
        .collect();
    if !listed.iter().any(|each| each.path == plan) {
        found.insert(0, (plan.to_owned(), content(host, observed, plan)?));
    }
    Ok(found)
}

/// The newest of `revisions` whose statement, following one of `answers`,
/// `decided` finds the owner signed; none when the owner signed none or
/// has no personal board, or Board's refusal to read the first it cannot.
fn signed(
    observed: &Observed,
    revisions: &[(String, Content)],
    answers: &[(&str, &str)],
    decided: impl Fn(&Shown) -> Result<Option<DecideReport>, Box<Refusal>>,
) -> Approval {
    for (revision, content) in revisions {
        for (answer, option) in answers {
            match decided(&shown(observed, revision, content, answer, option)) {
                Ok(Some(_)) => return Approval::Signed(revision.clone()),
                Ok(None) => {}
                Err(refusal) if refusal.code.as_str() == Code::SettingsNoPersonal.as_str() => {
                    return Approval::None;
                }
                Err(refusal) => return Approval::Unread(refusal),
            }
        }
    }
    Approval::None
}

#[cfg(test)]
mod tests;
