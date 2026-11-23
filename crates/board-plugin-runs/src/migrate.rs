//! `runs.migrate`: gives each suite of this board that has no `kind` the
//! kind its name and its neighbours imply, as `kind` was added to
//! `runs/suite/v1` after suites were written.
//!
//! The first rule that matches a suite chooses its kind:
//!
//! 1. compliance, when a zero rule of this board names the suite, or its
//!    method starts with the apply-standard skill's words;
//! 2. eval, when an eval of this board has the suite's name or lists the
//!    suite in `suites`;
//! 3. compliance, when the name ends `-review`, as the audit skill names
//!    its suites;
//! 4. test, when the name ends as a check suite's does, such as `-gates`;
//! 5. benchmark, when the name starts `bench-` or ends as a timing's does,
//!    such as `-latency`.
//!
//! A suite no rule matches keeps no kind and is listed as unclassified. A
//! suite that has a kind is left as it is.

use board_model::parse_json;
use board_ops::{Bulk, Host, Limit, PluginError, literal};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::{SuiteKind, SuiteRecord};

/// The reason the migration's commit gives in its `Board-Bulk:` trailer.
const BULK: &str = "migrate suites";
/// The most suites each list of the output holds (STANDARDS 23).
const LISTED: usize = 100;
/// The type of an eval, whose name or `suites` may name a suite.
const EVAL: &str = "evals/eval/v1";
/// The type of a rule, whose zero checker may name a suite.
const RULE: &str = "rules/rule/v1";
/// The words the apply-standard skill starts a suite's method with.
const APPLY_STANDARD: &str = "Audited by the apply-standard skill";
/// The endings of a name rule 4 reads as a test suite's.
const TEST_ENDINGS: [&str; 6] = ["-gates", "-ci", "-checks", "-pr-lane", "-ready", "-lane"];
/// The endings of a name rule 5 reads as a benchmark's.
const BENCHMARK_ENDINGS: [&str; 5] = ["-latency", "-timing", "-benchmark", "-overhead", "-cost"];

/// Whether to write the migration.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RunsMigrateInput")]
pub(crate) struct Input {
    /// Write the kind into every suite a rule classifies; without it nothing is written.
    #[serde(default)]
    apply: bool,
}

/// What the migration gave each suite, or would give.
///
/// `counts` says how many suites each kind took, how many no rule matched
/// and how many already had a kind. `unclassified` lists the suites no rule
/// matched, and `suites` each suite a rule classified with its kind and the
/// rule, each at most 100, with `total` and `truncated` for `suites`.
/// `unreadable` lists each suite that is no JSON object.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "RunsMigrateOutput")]
pub(crate) struct Output {
    applied: bool,
    counts: Counts,
    unclassified: Vec<String>,
    suites: Vec<Classified>,
    total: usize,
    truncated: bool,
    unreadable: Vec<String>,
}

/// How many suites took each kind, matched no rule, or already had a kind.
#[derive(Debug, Default, Serialize, JsonSchema)]
struct Counts {
    compliance: usize,
    benchmark: usize,
    eval: usize,
    test: usize,
    unclassified: usize,
    kept: usize,
}

/// A suite a rule classified.
#[derive(Debug, Serialize, JsonSchema)]
struct Classified {
    path: String,
    kind: SuiteKind,
    by: ChosenBy,
}

/// The rule that chose a suite's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum ChosenBy {
    ZeroRule,
    ApplyStandard,
    EvalName,
    EvalSuites,
    ReviewName,
    TestName,
    BenchmarkName,
}

/// The rows of this board's own records of `kind`: path, blob and body.
fn own(host: &dyn Host, kind: &str) -> Result<Vec<(String, String, String)>, PluginError> {
    let sql = format!(
        "SELECT e.logical, e.blob, b.body FROM entries AS e JOIN bodies AS b ON b.blob = e.blob \
         WHERE e.type = {} AND e.source = 'worktree' ORDER BY e.logical",
        literal(kind)
    );
    let report = (host.query(&sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("runs", "query", &error.refusal()))?;
    if report.truncated {
        return Err(PluginError {
            code: "runs::migrate::over_budget".to_owned(),
            message: "the board's records passed the output budget of one board run; migrate a \
                      smaller board, or raise the budget"
                .to_owned(),
            fields: Map::new(),
            hint: None,
        });
    }
    Ok((report.rows.into_iter())
        .filter_map(|row| match <[Value; 3]>::try_from(row) {
            Ok(
                [
                    Value::String(path),
                    Value::String(blob),
                    Value::String(body),
                ],
            ) => Some((path, blob, body)),
            _ => None,
        })
        .collect())
}

/// What this board's evals and rules say about its suites.
struct Neighbours {
    /// The path of each eval.
    evals: Vec<String>,
    /// Each suite an eval lists in `suites`, or a zero rule names.
    listed: Vec<String>,
    zeroed: Vec<String>,
}

impl Neighbours {
    fn read(host: &dyn Host) -> Result<Self, PluginError> {
        let body = |text: &str| parse_json(text.as_bytes()).unwrap_or(Value::Null);
        let (mut evals, mut listed, mut zeroed) = (Vec::new(), Vec::new(), Vec::new());
        for (path, _, text) in own(host, EVAL)? {
            let eval = body(&text);
            let suites = eval.get("suites").and_then(Value::as_array);
            listed.extend(
                (suites.into_iter().flatten())
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            );
            evals.push(path);
        }
        for (_, _, text) in own(host, RULE)? {
            let rule = body(&text);
            let checker = rule.get("checker");
            if checker.and_then(|checker| checker.get("check")) == Some(&json!("zero"))
                && let Some(suite) = checker
                    .and_then(|checker| checker.get("suite"))
                    .and_then(Value::as_str)
            {
                zeroed.push(suite.to_owned());
            }
        }
        Ok(Self {
            evals,
            listed,
            zeroed,
        })
    }

    /// The kind of the suite at `path` whose method is `method`, and the
    /// rule that chose it, or `None` when no rule matches.
    fn classify(&self, path: &str, method: &str) -> Option<(SuiteKind, ChosenBy)> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let named = |names: &[String]| names.iter().any(|named| named == path);
        if named(&self.zeroed) {
            return Some((SuiteKind::Compliance, ChosenBy::ZeroRule));
        }
        if method.starts_with(APPLY_STANDARD) {
            return Some((SuiteKind::Compliance, ChosenBy::ApplyStandard));
        }
        if self
            .evals
            .iter()
            .any(|eval| eval == &format!("/evals/{name}"))
        {
            return Some((SuiteKind::Eval, ChosenBy::EvalName));
        }
        if named(&self.listed) {
            return Some((SuiteKind::Eval, ChosenBy::EvalSuites));
        }
        if name.ends_with("-review") {
            return Some((SuiteKind::Compliance, ChosenBy::ReviewName));
        }
        if TEST_ENDINGS.iter().any(|ending| name.ends_with(ending)) {
            return Some((SuiteKind::Test, ChosenBy::TestName));
        }
        if name.starts_with("bench-")
            || BENCHMARK_ENDINGS
                .iter()
                .any(|ending| name.ends_with(ending))
        {
            return Some((SuiteKind::Benchmark, ChosenBy::BenchmarkName));
        }
        None
    }
}

pub(crate) fn migrate(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { apply } = PluginError::read_input("runs", "migrate", input)?;
    let neighbours = Neighbours::read(host)?;
    let mut counts = Counts::default();
    let (mut suites, mut unclassified, mut unreadable) = (Vec::new(), Vec::new(), Vec::new());
    let mut lines = Vec::new();
    for (path, blob, text) in own(host, SuiteRecord::TYPE)? {
        let Ok(Value::Object(mut body)) = parse_json(text.as_bytes()) else {
            unreadable.push(path);
            continue;
        };
        if body.contains_key("kind") {
            counts.kept += 1;
            continue;
        }
        let method = body
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some((kind, by)) = neighbours.classify(&path, method) else {
            counts.unclassified += 1;
            unclassified.push(path);
            continue;
        };
        *match kind {
            SuiteKind::Compliance => &mut counts.compliance,
            SuiteKind::Benchmark => &mut counts.benchmark,
            SuiteKind::Eval => &mut counts.eval,
            SuiteKind::Test => &mut counts.test,
        } += 1;
        body.insert("kind".to_owned(), json!(kind));
        lines.push(json!({"path": path, "expect": blob, "record": Value::Object(body)}));
        suites.push(Classified { path, kind, by });
    }
    if apply && !lines.is_empty() {
        write(host, &lines)?;
    }
    let total = suites.len();
    suites.truncate(LISTED);
    unclassified.truncate(LISTED);
    let output = Output {
        applied: apply,
        counts,
        unclassified,
        suites,
        total,
        truncated: total > LISTED,
        unreadable,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

/// Writes `lines` in one commit with the migration's `Board-Bulk:` trailer.
fn write(host: &dyn Host, lines: &[Value]) -> Result<(), PluginError> {
    let refused = |refusal: &board_model::Refusal| {
        PluginError::board_refused("runs", "put --batch --bulk", refusal)
    };
    let bulk = Bulk::new(BULK).map_err(|error| refused(&error.refusal()))?;
    (host.put_bulk(lines, &bulk)).map_err(|error| refused(&error.refusal()))?;
    Ok(())
}

#[cfg(test)]
mod tests;
