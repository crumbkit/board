//! `evaluate`: check named parts of text against controls' budget, pattern
//! and judged rules.

use std::collections::{BTreeMap, BTreeSet};

use board_model::LogicalPath;
use board_ops::{Host, PluginError};
use regex::{Regex, RegexBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::check::measure;
use crate::index;
use crate::logical;
use crate::records::{Budget, Measure, Pattern, Rule, Show};

/// The findings a result shows; the rest are counted in `omitted`, so
/// output stays bounded.
const SHOWN: usize = 20;

/// Check each part of text against each control's rule.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    /// The controls to check the parts against, by their logical paths in the board's own tree.
    #[schemars(length(min = 1), inner(regex(pattern = "^/")))]
    controls: Vec<String>,
    /// Each part of text to check, by its name, such as pr.body.
    #[schemars(extend("minProperties" = 1))]
    parts: BTreeMap<String, String>,
}

/// Each control's verdict on each part, by control as given and then by
/// part; `passed` when none failed.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "EvaluateOutput")]
pub(crate) struct Output {
    passed: bool,
    results: Vec<Evaluated>,
}

/// One control's verdict on one part.
///
/// `status` is `skipped` for a judged rule when the part failed a budget or
/// pattern rule, and `stated` for a control without a rule. `used` and `max`
/// are null except in a budget result. `omitted` counts the findings left out
/// past the first 20, and is null when none is.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Evaluated {
    control: String,
    part: String,
    rule: Option<Kind>,
    status: Status,
    findings: Vec<Finding>,
    used: Option<u64>,
    max: Option<u64>,
    omitted: Option<usize>,
}

/// What a rule found wrong in a part: the text at fault, and its line, or
/// null when it has none.
///
/// A pattern finding quotes what matched, and names the expression as
/// `pattern` only when the rule's `show` is `pattern`; otherwise `pattern`
/// is null. A judged finding carries `rewrite`, the model's instruction for
/// fixing the text, which is null in every other finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Finding {
    text: String,
    line: Option<usize>,
    pattern: Option<String>,
    rewrite: Option<String>,
}

/// The kind of rule a result comes from.
#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Budget,
    Pattern,
    Judged,
}

/// How a part fared against one control.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Status {
    Passed,
    Failed,
    Skipped,
    Stated,
}

/// A pattern file's expressions, each with the line it was read from.
type Expressions = Vec<(String, Regex)>;

/// Each control's verdict on each part: budget and pattern rules first,
/// then judged rules.
///
/// A judged rule skips a part that failed a budget or pattern rule. A
/// control named twice is checked once. Each pattern file and rubric is
/// read once, through one `get_each`, and each pattern file is compiled
/// once per control.
pub(crate) fn evaluate(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { controls, parts } = PluginError::read_input("controls", "evaluate", input)?;
    let mut seen = BTreeSet::new();
    let named: Vec<String> = (controls.into_iter())
        .filter(|control| seen.insert(control.clone()))
        .collect();
    let mut known = index::controls(host)?;
    let missing: Vec<&String> = (named.iter())
        .filter(|path| !known.contains_key(*path))
        .collect();
    if !missing.is_empty() {
        return Err(not_found(&missing));
    }
    let rules: Vec<(String, Option<Rule>)> = (named.into_iter())
        .map(|path| {
            let rule = known.remove(&path).and_then(|control| control.rule);
            (path, rule)
        })
        .collect();
    let sources = sources(host, &rules)?;
    let mut results: BTreeMap<(&str, &str), Evaluated> = BTreeMap::new();
    for (control, rule) in &rules {
        let expressions = match rule {
            Some(Rule::Pattern(pattern)) => expressions(control, pattern, &sources)?,
            _ => Vec::new(),
        };
        for (part, text) in &parts {
            let result = match rule {
                None => Some(stated(control, part)),
                Some(Rule::Budget(budget)) => Some(budgeted(control, part, text, budget)),
                Some(Rule::Pattern(pattern)) => {
                    let (found, total) = matches(&expressions, pattern.show == Show::Pattern, text);
                    let status = failed_if(total > 0);
                    Some(result(
                        control,
                        part,
                        Some(Kind::Pattern),
                        status,
                        found,
                        total,
                    ))
                }
                Some(Rule::Judged(_)) => None,
            };
            if let Some(result) = result {
                results.insert((control, part), result);
            }
        }
    }
    let failed: BTreeSet<&str> = (results.iter())
        .filter(|(_, result)| result.status == Status::Failed)
        .map(|((_, part), _)| *part)
        .collect();
    for (control, rule) in &rules {
        let Some(Rule::Judged(judged)) = rule else {
            continue;
        };
        for (part, text) in &parts {
            let kind = Some(Kind::Judged);
            let result = if failed.contains(part.as_str()) {
                result(control, part, kind, Status::Skipped, Vec::new(), 0)
            } else {
                let findings = judge(control, text, &sources[&judged.judged])?;
                let total = findings.len();
                result(control, part, kind, failed_if(total > 0), findings, total)
            };
            results.insert((control, part), result);
        }
    }
    let ordered: Vec<Evaluated> = (rules.iter())
        .flat_map(|(control, _)| {
            parts
                .keys()
                .map(move |part| (control.as_str(), part.as_str()))
        })
        .filter_map(|key| results.remove(&key))
        .collect();
    let output = Output {
        passed: ordered.iter().all(|result| result.status != Status::Failed),
        results: ordered,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and lists"))
}

/// The bytes of each pattern file and rubric the rules name, by the path
/// the rule gives, or `controls::rule::unreadable`.
fn sources(
    host: &dyn Host,
    rules: &[(String, Option<Rule>)],
) -> Result<BTreeMap<String, Vec<u8>>, PluginError> {
    let mut named: BTreeMap<&str, &str> = BTreeMap::new();
    for (control, rule) in rules {
        let file = match rule {
            Some(Rule::Pattern(pattern)) => &pattern.pattern,
            Some(Rule::Judged(judged)) => &judged.judged,
            _ => continue,
        };
        named.entry(file).or_insert(control);
    }
    if named.is_empty() {
        return Ok(BTreeMap::new());
    }
    let paths = (named.keys())
        .map(|file| logical(file))
        .collect::<Result<Vec<LogicalPath>, PluginError>>()?;
    let answers = (host.get_each(&paths))
        .map_err(|error| PluginError::board_refused("controls", "get", &error.refusal("get")))?;
    let mut sources = BTreeMap::new();
    for ((file, control), answer) in named.into_iter().zip(answers) {
        match answer {
            Ok(report) => {
                sources.insert(file.to_owned(), report.bytes);
            }
            Err(error) => return Err(unreadable(control, file, error.code().as_str())),
        }
    }
    Ok(sources)
}

/// A result with the first `SHOWN` of `findings`, which are the first of
/// `total`, and how many were left out.
fn result(
    control: &str,
    part: &str,
    rule: Option<Kind>,
    status: Status,
    mut findings: Vec<Finding>,
    total: usize,
) -> Evaluated {
    findings.truncate(SHOWN);
    Evaluated {
        control: control.to_owned(),
        part: part.to_owned(),
        rule,
        status,
        findings,
        used: None,
        max: None,
        omitted: (total > SHOWN).then(|| total - SHOWN),
    }
}

/// `failed` when a rule found something, else `passed`.
fn failed_if(found: bool) -> Status {
    if found {
        Status::Failed
    } else {
        Status::Passed
    }
}

/// The result for a control without a rule, which never fails.
fn stated(control: &str, part: &str) -> Evaluated {
    result(control, part, None, Status::Stated, Vec::new(), 0)
}

/// The verdict of a budget on `text`, with what it used.
fn budgeted(
    control: &str,
    part: &str,
    text: &str,
    &Budget { budget, max, .. }: &Budget,
) -> Evaluated {
    let used = measure(text.as_bytes(), budget);
    let unit = match budget {
        Measure::Words => "words",
        Measure::Bytes => "bytes",
    };
    let over: Vec<Finding> = (used > max)
        .then(|| Finding {
            text: format!("{used} {unit}, more than the {max} allowed"),
            line: None,
            pattern: None,
            rewrite: None,
        })
        .into_iter()
        .collect();
    let total = over.len();
    Evaluated {
        used: Some(used),
        max: Some(max),
        ..result(
            control,
            part,
            Some(Kind::Budget),
            failed_if(total > 0),
            over,
            total,
        )
    }
}

/// The expressions in a pattern rule's file, one per line, blank lines
/// skipped, compiled once for every part with `^` and `$` matching at each
/// line.
///
/// An invalid one is `controls::pattern::invalid`. With `show` `match-only`
/// the error names the line and, for a syntax error, the column of the
/// fault, and nothing the expression holds, since the parser's own message
/// can quote part of it.
fn expressions(
    control: &str,
    rule: &Pattern,
    sources: &BTreeMap<String, Vec<u8>>,
) -> Result<Expressions, PluginError> {
    let file = &rule.pattern;
    let text = String::from_utf8_lossy(&sources[file]);
    let mut expressions = Vec::new();
    for (number, line) in (1..).zip(text.lines()) {
        if line.trim().is_empty() {
            continue;
        }
        let checked = regex_syntax::ParserBuilder::new()
            .multi_line(true)
            .build()
            .parse(line);
        let fault = match checked {
            Ok(_) => match RegexBuilder::new(line).multi_line(true).build() {
                Ok(expression) => {
                    expressions.push((line.to_owned(), expression));
                    continue;
                }
                // Past the parser only a size limit is left, which names no column.
                Err(error) => (None, error.to_string()),
            },
            Err(error) => syntax(&error),
        };
        return Err(invalid(control, file, number, rule.show, line, fault));
    }
    Ok(expressions)
}

/// The column of a syntax error and the parser's message for it, which
/// may quote the expression.
fn syntax(error: &regex_syntax::Error) -> (Option<usize>, String) {
    match error {
        regex_syntax::Error::Parse(error) => {
            (Some(error.span().start.column), error.kind().to_string())
        }
        regex_syntax::Error::Translate(error) => {
            (Some(error.span().start.column), error.kind().to_string())
        }
        other => (None, other.to_string()),
    }
}

/// The first `SHOWN` non-empty matches in `text` of the expressions, with
/// the line each starts on and the expression if `shown`, ordered by line,
/// and how many there are in all.
fn matches(expressions: &Expressions, shown: bool, text: &str) -> (Vec<Finding>, usize) {
    let (mut hits, scanned) = hits(expressions, text);
    debug_assert!(scanned <= text.len() * expressions.len());
    // Stable, so matches on one line keep the expressions' order.
    hits.sort_by_key(|hit| hit.line);
    let findings = (hits.iter().take(SHOWN))
        .map(|hit| Finding {
            text: text[hit.start..hit.end].to_owned(),
            line: Some(hit.line),
            pattern: shown.then(|| expressions[hit.expression].0.clone()),
            rewrite: None,
        })
        .collect();
    (findings, hits.len())
}

/// Where a non-empty match lies in a part.
struct Hit {
    line: usize,
    expression: usize,
    start: usize,
    end: usize,
}

/// Each expression's non-empty matches in `text`, in order, and how many
/// bytes Board scanned to number their lines: each expression's matches come
/// in increasing order, so the line count runs on from the last match and
/// scans no byte twice for one expression.
fn hits(expressions: &Expressions, text: &str) -> (Vec<Hit>, usize) {
    let mut hits = Vec::new();
    let mut scanned = 0;
    for (index, (_, expression)) in expressions.iter().enumerate() {
        let (mut at, mut line) = (0, 1);
        for matched in expression.find_iter(text) {
            if matched.is_empty() {
                continue;
            }
            let gap = &text[at..matched.start()];
            scanned += gap.len();
            line += gap.matches('\n').count();
            at = matched.start();
            hits.push(Hit {
                line,
                expression: index,
                start: matched.start(),
                end: matched.end(),
            });
        }
    }
    (hits, scanned)
}

/// The model's findings on `text` against `rubric`. Board cannot ask a
/// model yet, so a judged rule with a part to read is
/// `controls::judged::unavailable`; the runner lands in a follow-up.
fn judge(control: &str, _text: &str, _rubric: &[u8]) -> Result<Vec<Finding>, PluginError> {
    Err(PluginError {
        code: "controls::judged::unavailable".to_owned(),
        message: format!(
            "{control} has a judged rule, which this Board cannot apply yet: the agent that \
             judges text lands in a follow-up change; leave {control} out of `controls` until then"
        ),
        fields: Map::from_iter([("control".to_owned(), Value::from(control))]),
    })
}

fn not_found(missing: &[&String]) -> PluginError {
    let names: Vec<&str> = missing.iter().map(|path| path.as_str()).collect();
    PluginError {
        code: "controls::control::not_found".to_owned(),
        message: format!(
            "no valid control is in the board's own tree at {}; run `board get` on the path and \
             `board check` to see what is there",
            names.join(", ")
        ),
        fields: Map::from_iter([("controls".to_owned(), Value::from(names))]),
    }
}

fn unreadable(control: &str, file: &str, code: &str) -> PluginError {
    PluginError {
        code: "controls::rule::unreadable".to_owned(),
        message: format!(
            "{control} names {file} in its rule, which Board cannot read as a file ({code}); write \
             the file, or change the control's rule"
        ),
        fields: Map::from_iter([
            ("control".to_owned(), Value::from(control)),
            ("file".to_owned(), Value::from(file)),
            ("code".to_owned(), Value::from(code)),
        ]),
    }
}

/// `controls::pattern::invalid` for line `number` of `file`. Only with
/// `show` `pattern` does it quote the line and the parser's message.
fn invalid(
    control: &str,
    file: &str,
    number: usize,
    show: Show,
    line: &str,
    (column, why): (Option<usize>, String),
) -> PluginError {
    let detail = match (show, column) {
        (Show::Pattern, _) => format!(" ({line:?}): {why}"),
        (Show::MatchOnly, Some(column)) => format!(" at column {column}"),
        (Show::MatchOnly, None) => String::new(),
    };
    PluginError {
        code: "controls::pattern::invalid".to_owned(),
        message: format!(
            "line {number} of {file}, which {control} names, is not a valid regular expression\
             {detail}; fix that line"
        ),
        fields: Map::from_iter([
            ("control".to_owned(), Value::from(control)),
            ("file".to_owned(), Value::from(file)),
            ("line".to_owned(), Value::from(number)),
        ]),
    }
}

#[cfg(test)]
mod tests;
