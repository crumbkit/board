//! The `pr` capability: a pull request body from a change, its plan and
//! its records, and the command that would use it.

use std::fmt::Write as _;

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::board;
use crate::change::{read, referenced};
use crate::gates::{Observed, REPORT_TYPE, RUN_TYPE, objects, ran_on, sections};
use crate::phases::{PathInput, plan_text};
use crate::{PLUGIN, to_value};

/// Severities in the order a report's findings are counted.
const SEVERITIES: [&str; 4] = ["critical", "high", "medium", "low"];
/// Outcomes in the order a run's results are counted.
const OUTCOMES: [&str; 4] = ["pass", "fail", "error", "skipped"];
/// Plan sections the body carries, by heading.
const OUTCOME: &str = "Outcome";
const PROBLEM: &str = "Problem";
const OUT_OF_SCOPE: &str = "Out of scope";
const PROVES: &str = "What the tests prove";
const DOES_NOT_PROVE: &str = "What the tests do not prove";
/// Cases a run lists as failed, errored or skipped before the rest are only
/// counted.
const CASES: usize = 20;

/// The pull request drafted for a change.
///
/// `path` is the logical path of `pr.md`, which holds the body. `command`
/// is the `gh pr create` line, for a POSIX shell, that would open the pull
/// request with that body; the plugin never runs it. When the change
/// already records a pull request, `command` is null, `pull_request` holds
/// its URL and `pr.md` is not written. `unreadable` lists the references
/// that reach nothing or that Board marks degraded.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "PrOutput")]
pub(crate) struct Output {
    path: String,
    command: Option<String>,
    pull_request: Option<String>,
    unreadable: Vec<String>,
}

/// `text` quoted for a POSIX shell, as Python's `shlex.quote` quotes it.
pub(crate) fn quote(text: &str) -> String {
    let safe =
        |character: char| character.is_ascii_alphanumeric() || "@%+=:,./-_".contains(character);
    if !text.is_empty() && text.chars().all(safe) {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

/// `value` as Python's `str` shows it.
fn shown(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => "False".to_owned(),
        Some(other) => other.to_string(),
    }
}

/// `value` on one line, so a record's text cannot start a heading or a list
/// item.
fn one_line(value: Option<&Value>) -> String {
    shown(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `value` on one line as inline code, with backticks made quotes; `(empty)`
/// when it holds no text.
fn quoted(value: Option<&Value>) -> String {
    let text = one_line(value);
    if text.is_empty() {
        "(empty)".to_owned()
    } else {
        format!("`{}`", text.replace('`', "'"))
    }
}

fn quoted_text(text: &str) -> String {
    quoted(Some(&Value::from(text)))
}

/// How many of `items` hold each of `names` at `key`, such as `3 pass, 1
/// skipped`; `none` when empty.
fn counted(items: &[&Map<String, Value>], key: &str, names: &[&str]) -> String {
    let counts: Vec<String> = (names.iter())
        .filter_map(|name| {
            let count = (items.iter())
                .filter(|item| item.get(key).and_then(Value::as_str) == Some(name))
                .count();
            (count > 0).then(|| format!("{count} {name}"))
        })
        .collect();
    if counts.is_empty() {
        "none".to_owned()
    } else {
        counts.join(", ")
    }
}

/// What one run proves, as one line, and each limit on it, as lines,
/// naming at most `cases` cases.
pub(crate) fn run_lines(
    path: &str,
    run: &Map<String, Value>,
    head: Option<&str>,
    cases: usize,
) -> (String, Vec<String>) {
    let at = quoted_text(path);
    let mut proves = format!(
        "{at}: {}, {} run, results {}",
        quoted(run.get("suite")),
        one_line(run.get("scope")),
        counted(
            &objects(run, "results").collect::<Vec<_>>(),
            "outcome",
            &OUTCOMES
        )
    );
    if run.get("summary").is_some_and(truthy) {
        let _ = write!(proves, ", summary {}", quoted(run.get("summary")));
    }
    let mut limits = Vec::new();
    if run.get("scope").and_then(Value::as_str) != Some("full") {
        limits.push(format!("{at} ran only targeted cases."));
    }
    if run.get("dirty") != Some(&Value::Bool(false)) {
        limits.push(format!("{at} does not record a clean tree."));
    }
    if let Some(head) = head
        && !ran_on(run, head)
    {
        limits.push(format!(
            "{at} did not run on the head commit {}.",
            quoted_text(head)
        ));
    }
    let unmet: Vec<&Map<String, Value>> = (objects(run, "results"))
        .filter(|result| {
            matches!(
                result.get("outcome").and_then(Value::as_str),
                Some("fail" | "error" | "skipped")
            )
        })
        .collect();
    for result in unmet.iter().take(cases) {
        let note = if result.get("note").is_some_and(truthy) {
            format!(": {}", quoted(result.get("note")))
        } else {
            String::new()
        };
        limits.push(format!(
            "{at} case {} is {}{note}",
            quoted(result.get("case")),
            shown(result.get("outcome"))
        ));
    }
    if unmet.len() > cases {
        limits.push(format!("and {} more cases; see {at}", unmet.len() - cases));
    }
    (proves, limits)
}

fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false))
        && value.as_str().is_none_or(|text| !text.is_empty())
        && value.as_array().is_none_or(|items| !items.is_empty())
        && value.as_object().is_none_or(|fields| !fields.is_empty())
        && value.as_f64().is_none_or(|number| number != 0.0)
}

/// One review report: its verdict, who wrote it, the range it covers and
/// its findings by severity.
fn report_line(path: &str, report: &Map<String, Value>) -> String {
    format!(
        "{}: {} by {} over {}, findings {}",
        quoted_text(path),
        one_line(report.get("verdict")),
        quoted(report.get("by")),
        one_line(report.get("range")),
        counted(
            &objects(report, "findings").collect::<Vec<_>>(),
            "severity",
            &SEVERITIES
        )
    )
}

/// `lines` as a Markdown list.
fn bullets(lines: &[String]) -> String {
    let items: Vec<String> = lines.iter().map(|line| format!("- {line}")).collect();
    items.join("\n")
}

/// The pull request's body in Markdown.
///
/// The plan's Outcome says what changes and its Problem why. Its "What the
/// tests prove" and "What the tests do not prove" sections lead those parts
/// of the body, followed by what each attached run shows. Every reference
/// is listed as evidence.
pub(crate) fn body(observed: &Observed) -> String {
    let change = &observed.change;
    let plan = sections(observed.plan.as_deref().unwrap_or_default());
    let section = |name: &str| -> Option<String> {
        (plan.get(&name.to_lowercase()))
            .filter(|text| !text.is_empty())
            .cloned()
    };
    let missing = |name: &str| format!("_The plan's {name} section is empty._");
    let head = change.head.as_deref();
    let runs: Vec<(&str, &Map<String, Value>)> = observed.records.of_type(RUN_TYPE).collect();
    let mut proves: Vec<String> = section(PROVES).into_iter().collect();
    let mut limits: Vec<String> = section(DOES_NOT_PROVE).into_iter().collect();
    let mut shown_runs = Vec::new();
    for (path, run) in &runs {
        let (line, limited) = run_lines(path, run, head, CASES);
        shown_runs.push(line);
        if !limited.is_empty() {
            limits.push(bullets(&limited));
        }
    }
    if !shown_runs.is_empty() {
        proves.push(bullets(&shown_runs));
    }
    if runs.is_empty() {
        limits.push(bullets(&[
            "No test run is attached to this change.".to_owned()
        ]));
    }
    let or_missing = |parts: Vec<String>, name: &str| {
        if parts.is_empty() {
            missing(name)
        } else {
            parts.join("\n\n")
        }
    };
    let mut parts = vec![
        section(OUTCOME).unwrap_or_else(|| missing(OUTCOME)),
        format!(
            "## Why\n\n{}",
            section(PROBLEM).unwrap_or_else(|| missing(PROBLEM))
        ),
    ];
    if let Some(scope) = section(OUT_OF_SCOPE) {
        parts.push(format!("## Out of scope\n\n{scope}"));
    }
    parts.push(format!("## {PROVES}\n\n{}", or_missing(proves, PROVES)));
    parts.push(format!(
        "## {DOES_NOT_PROVE}\n\n{}",
        or_missing(limits, DOES_NOT_PROVE)
    ));
    let reports: Vec<String> = (observed.records.of_type(REPORT_TYPE))
        .map(|(path, report)| report_line(path, report))
        .collect();
    if !reports.is_empty() {
        parts.push(format!("## Review\n\n{}", bullets(&reports)));
    }
    parts.push(format!("## Evidence\n\n{}", bullets(&evidence(observed))));
    if let Some(issue) = &change.issue
        && !issue.is_empty()
        && !issue.chars().any(char::is_whitespace)
    {
        parts.push(format!("Closes {issue}"));
    }
    parts.join("\n\n") + "\n"
}

/// Each reference, in the change's order, with its type or why it could not
/// be read, after the change and plan.
fn evidence(observed: &Observed) -> Vec<String> {
    let mut lines = vec![format!("{}: the change", quoted_text(&observed.path))];
    if let Some(plan) = observed
        .change
        .plan
        .as_deref()
        .filter(|plan| !plan.is_empty())
    {
        let gone = if observed.plan.is_some() {
            ""
        } else {
            ", missing"
        };
        lines.push(format!("{}: the plan{gone}", quoted_text(plan)));
    }
    for path in observed.change.records() {
        let cited = if let Some((found, _)) = observed.records.get(path) {
            found.clone()
        } else {
            let code = (observed.records.unreadable.iter())
                .find(|(each, _)| each == path)
                .map_or("None", |(_, code)| code.as_str());
            format!("unreadable, {code}")
        };
        lines.push(format!("{}: {cited}", quoted_text(path)));
    }
    lines
}

/// Writes `<change>/pr.md` and returns its path and the `gh pr create` line
/// that would use it. A change that records a pull request gets its URL
/// instead, and no pr.md is written.
pub(crate) fn pr(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PathInput { path } = PluginError::read_input(PLUGIN, "pr", input)?;
    let path = path.trim_end_matches('/').to_owned();
    let (change, _) = read(host, &path)?;
    let records = referenced(host, change.records())?;
    let left: Vec<String> = (records.unreadable.iter())
        .map(|(each, _)| each.clone())
        .collect();
    let target = format!("{path}/pr.md");
    if let Some(opened) = change.pull_request.clone() {
        return Ok(to_value(Output {
            path: target,
            command: None,
            pull_request: Some(opened),
            unreadable: left,
        }));
    }
    let plan = plan_text(host, &change)?;
    let title = one_line(Some(&Value::from(change.title.as_str())));
    let mut observed = Observed::new(&path, change, records);
    observed.plan = plan;
    board::put(host, &[json!({"path": target, "text": body(&observed)})])?;
    let written = board::resolve(host, &target)?;
    let file = written.map_or_else(String::new, |answer| {
        let location = answer.location;
        format!("{}/{}", location.board.repository.display(), location.path)
    });
    let command = format!(
        "gh pr create --title {} --body-file {}",
        quote(&title),
        quote(&file)
    );
    Ok(to_value(Output {
        path: target,
        command: Some(command),
        pull_request: None,
        unreadable: left,
    }))
}

#[cfg(test)]
mod tests;
