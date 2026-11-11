//! The `brief` capability: what an agent needs to start a change, in at
//! most 600 words.

use board_ops::{Host, PluginError};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::board::{call_or_refusal, refused};
use crate::change::{current, plan_at, read, referenced};
use crate::controls::{CONTROLS_UNAVAILABLE, listed_by};
use crate::flow::Kind;
use crate::gates::kinds::named_controls;
use crate::gates::remedy::values;
use crate::gates::{HEADING, Observed};
use crate::lifecycle::{LifecycleRecord, says_none};
use crate::phases::{PathInput, plan_text, step_of};
use crate::plan::{Answers, documents, words};
use crate::{PLUGIN, Warning, refusal, to_value};

/// The most words `brief` returns.
const BRIEF_WORDS: usize = 600;
/// The most planned paths a rule lists as matched; it counts the rest, so a
/// rule costs about one line.
const MATCHED_SHOWN: usize = 3;
/// The most rules the brief names in `rules_left_out`; `rules_unnamed`
/// counts the rest.
const LEFT_OUT_NAMED: usize = 20;
/// The share of the word limit kept for the lines of the records the plan
/// uses, when they need it.
const USES_SHARE: usize = 4;
/// The codes Board refuses a call with when the controls plugin or its
/// capability is missing.
const CONTROL_TYPE: &str = "controls/control/v1";
const DECISION_TYPE: &str = "plans/decision/v1";
const EVAL_TYPE: &str = "evals/eval/v1";
/// The command that lists every rule on the planned paths; it reads
/// `for_paths_input` on stdin.
const RULES_ARGV: [&str; 3] = ["board", "call", "controls.for-paths"];

/// One record the plan uses, and one line saying what it holds.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Use {
    #[serde(rename = "ref")]
    reference: String,
    line: String,
}

/// One control that applies to the planned paths, and its summary.
///
/// `matched` lists at most three of the planned paths that reached it, and
/// `matched_left_out` counts the rest; both are left out when every planned
/// path reached it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub(crate) struct Rule {
    control: String,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    matched: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_left_out: Option<usize>,
}

/// Why the brief left a rule out: it did not fit in the word limit, or it
/// is a version control whose capture finds no version in the files the
/// planned paths reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) enum Reason {
    #[serde(rename = "no room in the brief's 600 words")]
    NoRoom,
    #[serde(rename = "no version found in the matched files")]
    NoVersion,
}

/// A rule the brief left out, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct LeftOut {
    control: String,
    reason: Reason,
}

impl LeftOut {
    /// The words the entry takes in the brief.
    fn words(&self) -> usize {
        let reason = serde_json::to_value(self.reason).expect("a reason is a string");
        words(&format!(
            "{} {}",
            self.control,
            reason.as_str().unwrap_or_default()
        ))
    }
}

/// A command to run without a shell: `argv` is the program and its
/// arguments, and `stdin` is its input.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct RulesCommand {
    argv: [&'static str; 3],
    stdin: String,
}

/// What an agent needs to start a change, in at most 600 words.
///
/// `plan` is the path of the text of the plan the change names, or null when the change
/// has no plan record. `answers` holds the plan's typed answers, left out
/// when it gives none. `lifecycle` is the path of the text of its lifecycle
/// record, left out when it has none; the text counts outside the limit, as
/// the plan's does. `warnings` says when a control a `planned-controlled`
/// check of the change's flow names does not exist, left out when none is
/// missing. `rules` lists each control on the plan's paths once,
/// as `controls.for-paths` finds them: those that matched only some planned
/// paths first, then the others, each group by control path. Each rule's
/// path and summary are fitted first, skipping any that do not fit, and its
/// matched paths after, while room remains. A version control whose
/// capture finds no version in the files the planned paths reach is left
/// out of `rules`; a missing or unreadable planned file, or a pattern that
/// matches no file yet, keeps it. `rules_left_out` names up to 20 rules left out, as many
/// as fit, each with its `reason`: those the word limit left out first,
/// then those version controls. `rules_unnamed` counts the rest.
/// `rules_command` is the command that lists every rule, as an `argv` to
/// run and the `stdin` to give it; it is null when the plan has no paths.
/// Rules are counted before uses, but the uses keep up to a quarter of the
/// limit. `uses` gives one line per record it uses, in order, skipping any
/// that do not fit, and `left_out` counts those the word limit left out.
/// `controls` is the command that lists the controls on the change's
/// repository. `brief_line`, `board-brief: <path>`, comes first in the
/// output; a builder's first message starts with it, and the write guard
/// reads it.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "BriefOutput")]
pub(crate) struct Output {
    pub(crate) brief_line: String,
    pub(crate) path: String,
    pub(crate) plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) answers: Option<Answers>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lifecycle: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<Warning>,
    pub(crate) rules: Vec<Rule>,
    pub(crate) rules_left_out: Vec<LeftOut>,
    pub(crate) rules_unnamed: usize,
    pub(crate) rules_command: Option<RulesCommand>,
    pub(crate) uses: Vec<Use>,
    pub(crate) left_out: usize,
    pub(crate) phase: String,
    pub(crate) instruction: String,
    pub(crate) controls: String,
}

/// The input `controls.for-paths` takes for `paths`, as JSON written as
/// Python's `json.dumps` writes it, with a space after each separator.
fn for_paths_input(paths: &[String]) -> String {
    let listed: Vec<String> = paths
        .iter()
        .map(|each| Value::from(each.as_str()).to_string())
        .collect();
    format!("{{\"paths\": [{}]}}", listed.join(", "))
}

/// The string at `key`, or empty.
fn field<'a>(body: &'a Map<String, Value>, key: &str) -> &'a str {
    body.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// One line saying what a used record or document holds; `text` is a
/// document's text.
fn line_of(found: Option<&(String, Map<String, Value>)>, text: Option<&str>) -> String {
    let Some((found, body)) = found else {
        return "unreadable: reaches nothing or is degraded".to_owned();
    };
    match found.as_str() {
        CONTROL_TYPE => {
            let summary = field(body, "summary");
            if summary.is_empty() {
                field(body, "requirement").to_owned()
            } else {
                summary.to_owned()
            }
        }
        DECISION_TYPE => field(body, "decision").to_owned(),
        EVAL_TYPE => {
            let question = field(body, "question");
            match body.get("verdict").and_then(Value::as_str) {
                Some(verdict) if !verdict.is_empty() => format!("{question} Verdict: {verdict}."),
                _ => question.to_owned(),
            }
        }
        _ => match text {
            Some(text) => {
                let lines: Vec<&str> = (text.lines().map(str::trim))
                    .filter(|line| !line.is_empty())
                    .collect();
                let headings: Vec<&str> = (lines.iter())
                    .filter_map(|line| Some(HEADING.captures(line)?.get(1)?.as_str()))
                    .collect();
                let first = if headings.is_empty() {
                    &lines
                } else {
                    &headings
                };
                first.first().copied().unwrap_or_default().to_owned()
            }
            None => found.clone(),
        },
    }
}

/// Each control that applies to `paths`, once, as `controls.for-paths`
/// answers; none for no paths.
///
/// A Board without the controls plugin enabled is
/// `changes::brief::controls_missing`.
fn rules_for(host: &dyn Host, paths: &[String]) -> Result<Vec<Map<String, Value>>, PluginError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let output = call_or_refusal(host, "controls.for-paths", &json!({"paths": paths})).map_err(
        |refusal_of| {
            let error = refused("call", &refusal_of);
            if CONTROLS_UNAVAILABLE.contains(&refusal_of.code.as_str()) {
                refusal(
                    "changes::brief::controls_missing",
                    format!(
                        "the brief lists the rules on the planned paths with controls.for-paths, \
                         which this Board refused: {}; enable it with `board plugin enable \
                         controls`, or run a Board that offers it",
                        error.message
                    ),
                    [],
                )
            } else {
                error
            }
        },
    )?;
    Ok((output
        .get("controls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|control| control.as_object().cloned())
    .collect())
}

/// `rule` as the brief lists it: `matched` left out when it is every
/// planned path, and cut to `MATCHED_SHOWN`.
fn rule_line(rule: &Map<String, Value>, paths: &[String]) -> Rule {
    let matched: Vec<String> = (rule
        .get("matched")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|each| each.as_str().map(str::to_owned))
    .collect();
    let mut line = Rule {
        control: field(rule, "control").to_owned(),
        summary: field(rule, "summary").to_owned(),
        matched: None,
        matched_left_out: None,
    };
    let every = (matched.iter().all(|each| paths.contains(each)))
        && paths.iter().all(|each| matched.contains(each));
    if every {
        return line;
    }
    line.matched_left_out = (matched.len() > MATCHED_SHOWN).then(|| matched.len() - MATCHED_SHOWN);
    line.matched = Some(matched.into_iter().take(MATCHED_SHOWN).collect());
    line
}

/// The rules that fit in `room` words, up to 20 rules left out with their
/// reasons, how many more were, and the words used.
///
/// Rules that matched only some of the planned paths come first, then the
/// others, each group by control path. `dropped` names the version controls
/// left out because they find no version; they are named after the rules
/// the word limit left out. When nothing is left out, the rules take the
/// whole room. Otherwise the words the entries need when every rule has
/// the whole room are kept back first, and the entries are named in order
/// while they fit, so the rules and entries never pass `room`.
pub(crate) fn fit_rules(
    found: &[Rule],
    dropped: &[String],
    room: i64,
) -> (Vec<Rule>, Vec<LeftOut>, usize, usize) {
    let mut ordered = found.to_vec();
    ordered.sort_by(|one, other| {
        (one.matched.is_none(), &one.control).cmp(&(other.matched.is_none(), &other.control))
    });
    let entries = |left_out: Vec<String>| -> Vec<LeftOut> {
        (left_out.into_iter())
            .map(|control| LeftOut {
                control,
                reason: Reason::NoRoom,
            })
            .chain(dropped.iter().map(|control| LeftOut {
                control: control.clone(),
                reason: Reason::NoVersion,
            }))
            .collect()
    };
    let (rules, left_out, spent) = fill(&ordered, room);
    let first = entries(left_out);
    if first.is_empty() {
        return (rules, Vec::new(), 0, spent);
    }
    let needed: usize = (first.iter().take(LEFT_OUT_NAMED))
        .map(LeftOut::words)
        .sum();
    let kept = i64::try_from(needed).unwrap_or(i64::MAX).min(room.max(0));
    let (rules, left_out, mut spent) = fill(&ordered, room - kept);
    let left_out = entries(left_out);
    let mut named = Vec::new();
    for entry in left_out.iter().take(LEFT_OUT_NAMED) {
        if i64::try_from(spent + entry.words()).unwrap_or(i64::MAX) > room {
            break;
        }
        spent += entry.words();
        named.push(entry.clone());
    }
    let unnamed = left_out.len() - named.len();
    (rules, named, unnamed, spent)
}

/// The rules of `ordered` that fit in `room` words, the paths of those left
/// out, and the words they take.
///
/// Each rule's path and summary come first, in order, skipping any that do
/// not fit, so one long rule leaves room for the shorter ones after it.
/// Matched paths are added after, in order, while room remains.
fn fill(ordered: &[Rule], room: i64) -> (Vec<Rule>, Vec<String>, usize) {
    let fits = |spent: usize, cost: usize| i64::try_from(spent + cost).unwrap_or(i64::MAX) <= room;
    let mut spent = 0;
    let mut fitted: Vec<(Rule, &Rule)> = Vec::new();
    let mut left_out = Vec::new();
    for rule in ordered {
        let cost = words(&format!("{} {}", rule.control, rule.summary));
        if !fits(spent, cost) {
            left_out.push(rule.control.clone());
            continue;
        }
        let bare = Rule {
            matched: None,
            matched_left_out: None,
            ..rule.clone()
        };
        fitted.push((bare, rule));
        spent += cost;
    }
    for (line, rule) in &mut fitted {
        let cost = rule.matched.as_ref().map_or(0, Vec::len);
        if cost > 0 && fits(spent, cost) {
            line.matched.clone_from(&rule.matched);
            line.matched_left_out = rule.matched_left_out;
            spent += cost;
        }
    }
    (
        fitted.into_iter().map(|(line, _)| line).collect(),
        left_out,
        spent,
    )
}

/// The path of the text of the lifecycle record the plan at `plan` names, or
/// none when it names none Board reads.
fn lifecycle_text(host: &dyn Host, plan: Option<&str>) -> Result<Option<String>, PluginError> {
    let Some(plan) = plan else {
        return Ok(None);
    };
    let Some(record) = plan_at(host, plan)?.and_then(|(record, _)| record.lifecycle) else {
        return Ok(None);
    };
    let found = referenced(host, std::slice::from_ref(&record))?;
    Ok(found
        .get(&record)
        .filter(|(kind, _)| kind == LifecycleRecord::TYPE)
        .and_then(|(_, body)| Some(field(body, "text").to_owned()).filter(|text| !text.is_empty())))
}

/// The warning for each control a `planned-controlled` check of the change's
/// flow names and the board does not hold, in which case the gate has only
/// the section heading to go on.
fn missing_controls(host: &dyn Host, observed: &Observed) -> Result<Vec<Warning>, PluginError> {
    let filled = values(observed);
    let mut said = Vec::new();
    for check in (observed.flow.iter())
        .flat_map(|flow| &flow.phases)
        .flat_map(|phase| &phase.checks)
        .filter(|each| each.kind.known() == Some(&Kind::PlannedControlled))
    {
        let headings: Vec<&str> = (check.when.iter())
            .flat_map(|when| when.section.iter().flatten())
            .map(|(heading, _)| heading.as_str())
            .collect();
        let guard = if headings.is_empty() {
            "typed answers guard".to_owned()
        } else {
            format!("{} heading guards", headings.join(" and "))
        };
        for control in named_controls(check, &filled) {
            if listed_by(host, &control, &[])?.is_none() {
                said.push(Warning {
                    code: "changes::brief::no_control",
                    detail: format!(
                        "no control {control} exists, so only the plan's {guard} what it would \
                         list; write the control to name those paths"
                    ),
                });
            }
        }
    }
    Ok(said)
}

pub(crate) fn brief(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PathInput { path } = PluginError::read_input(PLUGIN, "brief", input)?;
    Ok(to_value(brief_of(host, &path)?))
}

/// The brief of the change at `path`.
pub(crate) fn brief_of(host: &dyn Host, path: &str) -> Result<Output, PluginError> {
    let (change, _) = read(host, path)?;
    let (text, uses, paths) = current(host, &change)?;
    let records = referenced(host, &uses)?;
    let texts = documents(host, &uses, &records)?;
    let (step, observed) = step_of(host, path)?;
    let answers = match &change.plan {
        Some(plan) => plan_at(host, plan)?.and_then(|(record, _)| record.answers),
        None => None,
    };
    let said = answers.and_then(|answers| answers.lifecycle);
    let lifecycle =
        if (plan_text(host, &change)?.as_deref()).is_some_and(|text| says_none(said, text)) {
            None
        } else {
            lifecycle_text(host, change.plan.as_deref())?
        };
    let warnings = missing_controls(host, &observed)?;
    let controls = format!("board controls check --path {}", change.mount);
    let brief_line = format!("board-brief: {path}");
    let mut spent = words(
        &[
            brief_line.as_str(),
            path,
            text.as_deref().unwrap_or_default(),
            step.phase.as_str(),
            &step.instruction,
            &controls,
            lifecycle.as_deref().unwrap_or_default(),
        ]
        .join(" "),
    );
    spent += (warnings.iter())
        .map(|warning| words(&format!("{} {}", warning.code, warning.detail)))
        .sum::<usize>();
    let rules_command = (!paths.is_empty()).then(|| RulesCommand {
        argv: RULES_ARGV,
        stdin: for_paths_input(&paths),
    });
    if let Some(command) = &rules_command {
        spent += words(&format!("{} {}", RULES_ARGV.join(" "), command.stdin));
    }
    let lines: Vec<(String, String)> = (uses.iter())
        .map(|each| {
            let document = (texts.iter())
                .find(|(named, _)| named == each)
                .map(|(_, text)| text.as_str());
            (each.clone(), line_of(records.get(each), document))
        })
        .collect();
    // The uses keep up to a quarter of the limit from the rules, so many
    // rules cannot drop every line.
    let needed: usize = (lines.iter())
        .map(|(each, line)| words(&format!("{each} {line}")))
        .sum();
    let kept = needed.min(BRIEF_WORDS / USES_SHARE);
    // A version control that finds no version in the matched files does
    // not apply there, so it is named as dropped and costs no rule line.
    let (found, dropped): (Vec<_>, Vec<_>) = (rules_for(host, &paths)?.into_iter())
        .partition(|rule| rule.get("versions_found").and_then(Value::as_u64) != Some(0));
    let found: Vec<Rule> = (found.iter()).map(|rule| rule_line(rule, &paths)).collect();
    let dropped: Vec<String> = (dropped.iter())
        .map(|rule| field(rule, "control").to_owned())
        .collect();
    let signed = |count: usize| i64::try_from(count).unwrap_or(i64::MAX);
    let room = signed(BRIEF_WORDS) - signed(spent) - signed(kept);
    let (rules, named, unnamed, cost) = fit_rules(&found, &dropped, room);
    spent += cost;
    let mut shown = Vec::new();
    for (each, line) in lines {
        let cost = words(&format!("{each} {line}"));
        if spent + cost > BRIEF_WORDS {
            continue;
        }
        spent += cost;
        shown.push(Use {
            reference: each,
            line,
        });
    }
    Ok(Output {
        brief_line,
        path: path.to_owned(),
        plan: text,
        answers,
        lifecycle,
        warnings,
        rules,
        rules_left_out: named,
        rules_unnamed: unnamed,
        rules_command,
        left_out: uses.len() - shown.len(),
        uses: shown,
        phase: step.phase,
        instruction: step.instruction,
        controls,
    })
}

#[cfg(test)]
mod tests;
