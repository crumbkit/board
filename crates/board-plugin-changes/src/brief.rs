//! The `brief` capability: what an agent needs to start a change, in at
//! most 600 words.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::board::{call_or_refusal, refused};
use crate::change::{current, read, referenced};
use crate::controls::CONTROLS_UNAVAILABLE;
use crate::gates::HEADING;
use crate::phases::{PathInput, next_of};
use crate::plan::{documents, words};
use crate::{PLUGIN, refusal, to_value};

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
/// has no plan record. `rules` lists each control on the plan's paths once,
/// as `controls.for-paths` finds them: those that matched only some planned
/// paths first, then the others, each group by control path. Each rule's
/// path and summary are fitted first, skipping any that do not fit, and its
/// matched paths after, while room remains. `rules_left_out` names up to 20
/// rules the word limit left out, as many as fit, and `rules_unnamed`
/// counts the rest. `rules_command` is the command that lists every rule,
/// as an `argv` to run and the `stdin` to give it; it is null when the plan
/// has no paths. Rules are counted before uses, but the uses keep up to a
/// quarter of the limit. `uses` gives one line per record it uses, in
/// order, skipping any that do not fit, and `left_out` counts those the
/// word limit left out. `controls` is the command that lists the controls
/// on the change's repository. `brief_line`, `board-brief: <path>`, comes
/// first in the output; a builder's first message starts with it, and the
/// write guard reads it.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "BriefOutput")]
pub(crate) struct Output {
    pub(crate) brief_line: String,
    pub(crate) path: String,
    pub(crate) plan: Option<String>,
    pub(crate) rules: Vec<Rule>,
    pub(crate) rules_left_out: Vec<String>,
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

/// The rules that fit in `room` words, the names of up to 20 left out, how
/// many more were, and the words used.
///
/// Rules that matched only some of the planned paths come first, then the
/// others, each group by control path. When every rule fits, they take the
/// whole room. Otherwise the words for the left-out names that fit are kept
/// back first, so the rules and names never pass `room`.
pub(crate) fn fit_rules(found: &[Rule], room: i64) -> (Vec<Rule>, Vec<String>, usize, usize) {
    let mut ordered = found.to_vec();
    ordered.sort_by(|one, other| {
        (one.matched.is_none(), &one.control).cmp(&(other.matched.is_none(), &other.control))
    });
    let (rules, left_out, spent) = fill(&ordered, room);
    if left_out.is_empty() {
        return (rules, Vec::new(), 0, spent);
    }
    let kept = LEFT_OUT_NAMED.min(usize::try_from(room.max(0)).unwrap_or(0));
    let (rules, left_out, spent) = fill(&ordered, room - i64::try_from(kept).unwrap_or(0));
    let free =
        usize::try_from((room - i64::try_from(spent).unwrap_or(i64::MAX)).max(0)).unwrap_or(0);
    let named: Vec<String> = left_out
        .iter()
        .take(LEFT_OUT_NAMED.min(free))
        .cloned()
        .collect();
    let unnamed = left_out.len() - named.len();
    let spent = spent + named.len();
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
    let step = next_of(host, path)?;
    let controls = format!("board call controls.check --path {}", change.mount);
    let brief_line = format!("board-brief: {path}");
    let mut spent = words(
        &[
            brief_line.as_str(),
            path,
            text.as_deref().unwrap_or_default(),
            step.phase.as_str(),
            &step.instruction,
            &controls,
        ]
        .join(" "),
    );
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
    let found: Vec<Rule> = (rules_for(host, &paths)?.iter())
        .map(|rule| rule_line(rule, &paths))
        .collect();
    let signed = |count: usize| i64::try_from(count).unwrap_or(i64::MAX);
    let room = signed(BRIEF_WORDS) - signed(spent) - signed(kept);
    let (rules, named, unnamed, cost) = fit_rules(&found, room);
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
