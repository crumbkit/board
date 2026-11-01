//! `check`: evaluate content against every boundary that covers it, write
//! its crossing, and refuse on a failure the owner has not approved.

use std::collections::{BTreeMap, BTreeSet};

use board_model::LogicalPath;
use board_ops::{Host, PluginError, Shown};
use board_plugin::Target;
use board_plugin::shape::{Ref, Sha256, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::boundaries::{self, Boundary, Globs};
use crate::content::{Content, quoted};
use crate::crossing::{self, output};
use crate::records::{BoundaryRecord, CrossingRecord, Finding, Status, Verdict};
use crate::{personal, refused};

/// The failures a refusal's message names; its `failures` field holds them
/// all.
const NAMED: usize = 20;
/// The finding of a control without a rule, which cannot check content, so
/// its verdict fails the crossing; and the step that fixes it.
const NO_RULE: &str = "this control has no rule, so it cannot check content; give it a rule, \
                       or remove it from the boundary";
const GIVE_RULE: &str = "give each control without a rule a rule, or remove it from the boundary";
/// The op the owner approves to let one failed crossing through, and how
/// Board refuses a decide it has not admitted.
const OVERRIDE: &str = "boundary.override";
const ASKED: &str = "board::settings::asked";
const REFUSED: &str = "board::settings::refused";
const TOO_LARGE: &str = "board::json::too_large";

/// Check the parts against every boundary that matches the action,
/// destination and checkout.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CheckInput")]
pub(crate) struct Input {
    /// The capability that sends the parts, such as github.pr-create.
    action: String,
    /// Where the parts go, such as github:octo/board.
    to: String,
    /// The checkout's path in the personal board, such as /repos/board; left
    /// out for a checkout no board mounts.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    from: Option<String>,
    /// Each part of the content, by its name, such as pr.body.
    #[schemars(extend("minProperties" = 1))]
    parts: BTreeMap<String, String>,
}

/// The crossing written, the boundaries that matched, and every verdict;
/// each check passed or was overridden.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CheckOutput")]
pub(crate) struct Output {
    /// The SHA-256 of the content's action, destination and parts.
    #[schemars(with = "Sha256")]
    digest: String,
    /// The crossing's path in the personal board.
    crossing: String,
    /// The boundaries that matched, by their paths.
    #[schemars(with = "Vec<Ref<BoundaryRecord>>")]
    boundaries: Vec<String>,
    /// Each check's verdict on each part it applies to.
    verdicts: Vec<Verdict>,
    /// The parts no check of a matching boundary names, by name.
    unchecked: Vec<String>,
    /// Why nothing was checked, or null.
    note: Option<String>,
    /// The id of the owner's approved effect that let this content through
    /// its failed checks, or null.
    #[serde(rename = "override")]
    #[schemars(with = "Option<Sha256>")]
    overridden: Option<String>,
}

/// How the owner answered letting a failed crossing through.
enum Answer {
    /// Approved, by this effect.
    Approved(String),
    /// Not answered yet; this effect asks.
    Asked(String),
    /// Refused, by the answer to this effect.
    Refused(String),
    /// Too large to show, so it cannot be asked.
    TooLarge,
}

/// Evaluate the parts against each matching boundary's controls, write the
/// crossing, and refuse on a failure.
pub(crate) fn check(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        action,
        to,
        from,
        parts,
    } = PluginError::read_input("boundary", "check", input)?;
    let personal = personal(host)?;
    let personal = &*personal;
    let boundaries = boundaries::read(personal)?;
    let names: Vec<&String> = parts.keys().collect();
    let asked = (boundaries.iter()).flat_map(|boundary| boundary.asked(&action, &to, &names));
    let globs = Globs::answer(personal, asked)?;
    let matched: Vec<&Boundary> = (boundaries.iter())
        .filter(|boundary| boundary.covers(&globs, &action, &to, from.as_deref()))
        .collect();
    let (verdicts, unchecked) = evaluate(personal, &matched, &globs, &parts)?;
    let failed: Vec<&Verdict> = (verdicts.iter())
        .filter(|verdict| matches!(verdict.status, Status::Failed | Status::Stated))
        .collect();
    let content = Content {
        action: &action,
        to: &to,
        parts: &parts,
    };
    let digest = content.digest();
    // Only a failure asks the owner, so a passing crossing writes no effect.
    let answer = if failed.is_empty() {
        None
    } else {
        Some(ask(personal, &crossing::path(&digest), &content)?)
    };
    let overridden = match &answer {
        Some(Answer::Approved(effect)) => Some(effect.clone()),
        _ => None,
    };
    let passed = failed.is_empty() || overridden.is_some();
    let paths: Vec<String> = matched
        .iter()
        .map(|boundary| boundary.path.clone())
        .collect();
    let prior = crossing::find(personal, &digest)?;
    let crossing = CrossingRecord {
        tag: Tag::default(),
        digest: digest.clone(),
        action: action.clone(),
        to: to.clone(),
        from: from.clone(),
        passed,
        boundaries: paths.clone(),
        verdicts: verdicts.clone(),
        // A re-check that still passes keeps what the destination returned.
        result: (prior.as_ref())
            .filter(|_| passed)
            .and_then(|(prior, _)| prior.result.clone()),
        overridden: overridden.clone(),
    };
    crossing::write(
        personal,
        &crossing,
        prior.as_ref().map(|(_, blob)| blob.as_str()),
    )?;
    if let Some(answer) = answer.filter(|_| !passed) {
        return Err(refusal(&action, &digest, &failed, &answer));
    }
    let note = if matched.is_empty() {
        let from = from.map_or_else(String::new, |from| format!(" from {from}"));
        Some(format!(
            "no boundary matches {action} to {to}{from}; nothing was checked"
        ))
    } else if verdicts.is_empty() {
        Some(
            "the boundaries that match name no control for these parts; nothing was checked"
                .to_owned(),
        )
    } else {
        None
    };
    Ok(output(&Output {
        crossing: crossing::path(&digest),
        digest,
        boundaries: paths,
        verdicts,
        unchecked,
        note,
        overridden,
    }))
}

/// Every verdict of the matched boundaries' controls on the parts, sorted
/// by control and part, and the parts no check names.
///
/// Parts are grouped by the controls that apply to them, and each group is
/// one `controls.evaluate` in the personal board, so a part's budget and
/// pattern rules run before its judged rules.
fn evaluate(
    host: &dyn Host,
    matched: &[&Boundary],
    globs: &Globs,
    parts: &BTreeMap<String, String>,
) -> Result<(Vec<Verdict>, Vec<String>), PluginError> {
    let mut groups: BTreeMap<Vec<&str>, Map<String, Value>> = BTreeMap::new();
    let mut unchecked = Vec::new();
    for (name, text) in parts {
        let controls: BTreeSet<&str> = (matched.iter())
            .flat_map(|boundary| &boundary.record.checks)
            .filter(|check| check.parts.iter().any(|glob| globs.matches(name, glob)))
            .map(|check| check.control.as_str())
            .collect();
        if controls.is_empty() {
            unchecked.push(name.clone());
        } else {
            let group = groups.entry(controls.into_iter().collect()).or_default();
            group.insert(name.clone(), json!(text));
        }
    }
    let target: Target = "controls.evaluate"
        .parse()
        .expect("controls.evaluate is <plugin>.<capability>");
    let mut verdicts = Vec::new();
    for (controls, texts) in groups {
        let input = json!({"controls": controls, "parts": texts});
        let report = (host.call(&target, &input)).map_err(|refusal| refused("call", &refusal))?;
        let results = Vec::<Verdict>::deserialize(&report.output["results"])
            .expect("controls.evaluate's output schema admits only results a Verdict reads");
        verdicts.extend(results);
    }
    verdicts.sort_by(|a, b| (&a.control, &a.part).cmp(&(&b.control, &b.part)));
    Ok((verdicts, unchecked))
}

/// How the owner answered letting the failed crossing at `path` through.
///
/// The owner is shown the content, so the signed approval names what it
/// lets through, and the path's digest binds the approval to that content.
/// Board refuses to show more than 1 MiB, so such content cannot be
/// overridden (a soft limit; showing only the parts the checks failed on
/// would raise it).
fn ask(host: &dyn Host, path: &str, content: &Content<'_>) -> Result<Answer, PluginError> {
    let path: LogicalPath = path
        .parse()
        .expect("/crossings/<hex digest> is a logical path");
    let shown = Shown::Text(content.shown().into_bytes());
    let refusal = match host.decide(OVERRIDE, &path, Some(&shown)) {
        Ok(report) => {
            let effect = (report.effect)
                .expect("Host::decide asks as the plugin, so Board names the effect it admits");
            return Ok(Answer::Approved(effect));
        }
        Err(refusal) => refusal,
    };
    let effect = (refusal.fields.get("effect").and_then(Value::as_str)).map(str::to_owned);
    match (refusal.code.as_str(), effect) {
        (ASKED, Some(effect)) => Ok(Answer::Asked(effect)),
        (REFUSED, Some(effect)) => Ok(Answer::Refused(effect)),
        (TOO_LARGE, _) => Ok(Answer::TooLarge),
        _ => Err(refused("decide", &refusal)),
    }
}

/// `boundary::check::failed`, naming each failing control, part and line,
/// with the next step and what the owner may do about it.
fn refusal(action: &str, digest: &str, failed: &[&Verdict], answer: &Answer) -> PluginError {
    let mut failures = Vec::new();
    let mut named = Vec::new();
    let no_rule = [Finding {
        text: NO_RULE.to_owned(),
        line: None,
        pattern: None,
        rewrite: None,
    }];
    for verdict in failed {
        let findings = if verdict.findings.is_empty() {
            &no_rule[..]
        } else {
            &verdict.findings[..]
        };
        for finding in findings {
            let mut failure = json!({
                "control": verdict.control,
                "part": verdict.part,
                "text": finding.text,
            });
            if let Some(line) = finding.line {
                failure["line"] = json!(line);
            }
            if let Some(rewrite) = &finding.rewrite {
                failure["rewrite"] = json!(rewrite);
            }
            let line = (finding.line).map_or_else(String::new, |line| format!(" line {line}"));
            let rewrite = (finding.rewrite.as_ref())
                .map_or_else(String::new, |rewrite| format!(" ({rewrite})"));
            failures.push(failure);
            named.push(format!(
                "{} on {}{line}: {}{rewrite}",
                verdict.control,
                verdict.part,
                quoted(&finding.text)
            ));
        }
    }
    let more = (named.len().checked_sub(NAMED))
        .filter(|more| *more > 0)
        .map_or_else(String::new, |more| format!("; and {more} more"));
    let mut steps = Vec::new();
    if failed
        .iter()
        .any(|verdict| verdict.status == Status::Stated)
    {
        steps.push(GIVE_RULE.to_owned());
    }
    let parts: BTreeSet<&str> = (failed.iter())
        .filter(|verdict| verdict.status == Status::Failed)
        .map(|verdict| verdict.part.as_str())
        .collect();
    if !parts.is_empty() {
        steps.push(format!(
            "change {}: remove or reword each quoted match, shorten each part over its budget, \
             and follow each rewrite",
            parts.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    let (owner, effect) = match answer {
        Answer::TooLarge => (
            "This content is over 1 MiB as the owner would see it, so the owner cannot approve \
             it; shorten it."
                .to_owned(),
            None,
        ),
        Answer::Refused(effect) => (
            format!(
                "The owner refused to let this exact content through ({effect}); the owner can \
                 ask again by deleting /effects/seen/{effect} in the personal board."
            ),
            Some(effect),
        ),
        Answer::Asked(effect) | Answer::Approved(effect) => (
            format!(
                "The owner may approve this exact content instead with \
                 `board answer {effect} approve`; then check again."
            ),
            Some(effect),
        ),
    };
    let mut fields = Map::from_iter([
        ("digest".to_owned(), json!(digest)),
        ("crossing".to_owned(), json!(crossing::path(digest))),
        ("failures".to_owned(), Value::Array(failures)),
    ]);
    if let Some(effect) = effect {
        fields.insert("effect".to_owned(), json!(effect));
    }
    PluginError {
        code: "boundary::check::failed".to_owned(),
        message: format!(
            "{} finding(s) stop this content: {}{more}",
            named.len(),
            named[..named.len().min(NAMED)].join("; ")
        ),
        fields,
        hint: Some(format!(
            "Next: {}. Then run `board call {action}` again, or \
             `board --personal call boundary.check` with the changed parts on stdin. {owner}",
            steps.join("; ")
        )),
    }
}
