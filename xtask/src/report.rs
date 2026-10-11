//! The lint's report, and the one place this binary prints.

use std::error::Error;
use std::fmt::Write as _;

use crate::bench::{Timed, Took};
use crate::controls::Divergence;

/// How many parts a report line names before summarising the rest.
const SHOWN_PARTS: usize = 5;

const USAGE: &str =
    "usage: cargo xtask lint\n       cargo xtask bench [--board <dir>] [--records <n>]\n";

#[derive(Debug)]
pub(crate) struct Report {
    pub(crate) checks: Vec<Check>,
    pub(crate) controls: Vec<Divergence>,
}

impl Report {
    pub(crate) fn passed(&self) -> bool {
        self.checks.iter().all(Check::passed) && self.controls.is_empty()
    }
}

/// One measured quantity, its limit, and the files it was measured over.
#[derive(Debug)]
pub(crate) struct Check {
    pub(crate) kind: CheckKind,
    /// The directory a per-directory budget measured, such as `docs/architecture`.
    node: Option<String>,
    pub(crate) limit: Limit,
    pub(crate) used: usize,
    pub(crate) parts: Vec<Part>,
}

impl Check {
    /// A check whose usage is the sum of its parts.
    pub(crate) fn total(kind: CheckKind, limit: Limit, parts: Vec<Part>) -> Self {
        let used = parts.iter().map(|part| part.amount).sum();
        Self::sorted(kind, limit, used, parts)
    }

    /// A check whose usage is its largest part.
    pub(crate) fn largest(kind: CheckKind, limit: Limit, parts: Vec<Part>) -> Self {
        let used = parts.iter().map(|part| part.amount).max().unwrap_or(0);
        Self::sorted(kind, limit, used, parts)
    }

    fn sorted(kind: CheckKind, limit: Limit, used: usize, mut parts: Vec<Part>) -> Self {
        parts.sort_by(|left, right| {
            right
                .amount
                .cmp(&left.amount)
                .then_with(|| left.label.cmp(&right.label))
        });
        Self {
            kind,
            node: None,
            limit,
            used,
            parts,
        }
    }

    /// This check measured only the files directly inside the directory `node`.
    pub(crate) fn in_node(mut self, node: String) -> Self {
        self.node = Some(node);
        self
    }

    /// The directory measured, or else the budget's name.
    pub(crate) fn name(&self) -> &str {
        self.node.as_deref().unwrap_or(self.kind.name())
    }

    pub(crate) fn passed(&self) -> bool {
        match self.limit {
            Limit::AtMost(max) => self.used <= max,
            Limit::Exactly(count) => self.used == count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckKind {
    Context,
    DocsIndex,
    Docs,
    Plan,
    SourceLines,
    Expect,
}

impl CheckKind {
    fn name(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::DocsIndex => "docs-index",
            Self::Docs => "docs",
            Self::Plan => "plan",
            Self::SourceLines => "source",
            Self::Expect => "expect",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::Context | Self::DocsIndex | Self::Docs | Self::Plan => "words",
            Self::SourceLines => "lines",
            Self::Expect => "attributes",
        }
    }

    /// The code a failure of this check carries.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Context | Self::DocsIndex | Self::Docs | Self::Plan => {
                "xtask::budget::over_limit"
            }
            Self::SourceLines => "xtask::source::too_long",
            Self::Expect => "xtask::expect::count_changed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Limit {
    AtMost(usize),
    Exactly(usize),
}

/// One file's share of a check.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Part {
    pub(crate) label: String,
    pub(crate) amount: usize,
}

impl Part {
    pub(crate) fn new(label: String, amount: usize) -> Self {
        Self { label, amount }
    }
}

pub(crate) enum Output<'a> {
    Report(&'a Report),
    Bench(&'a [Timed]),
    Failure {
        code: &'static str,
        error: &'a dyn Error,
    },
    Usage,
}

#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "this module is the binary's one printer"
)]
pub(crate) fn emit(output: &Output<'_>) {
    match output {
        Output::Report(report) => print!("{}", render(report)),
        Output::Bench(timed) => print!("{}", render_bench(timed)),
        Output::Failure { code, error } => eprint!("{}", render_failure(code, *error)),
        Output::Usage => eprint!("{USAGE}"),
    }
}

/// One line per operation: its median, its budget and their ratio, or why
/// it was skipped.
pub(crate) fn render_bench(timed: &[Timed]) -> String {
    let mut text = String::new();
    for operation in timed {
        let median = match &operation.took {
            Took::Median(median) => median,
            Took::Skipped(reason) => {
                let _ = writeln!(text, "skip  {reason}  {}", operation.operation);
                continue;
            }
        };
        let status = if operation.passed() { "pass" } else { "FAIL" };
        let budget = operation.budget;
        let _ = writeln!(
            text,
            "{status}  {median:>8.1} ms  budget {millis:>6} ms  {ratio:>4.1}x  {name}",
            median = median.as_secs_f64() * 1_000.0,
            millis = budget.as_millis(),
            ratio = median.as_secs_f64() / budget.as_secs_f64(),
            name = operation.operation,
        );
    }
    text
}

pub(crate) fn render(report: &Report) -> String {
    let mut text = String::new();
    let width = report
        .checks
        .iter()
        .map(|check| check.name().len())
        .max()
        .unwrap_or(0);
    for check in &report.checks {
        let status = if check.passed() { "pass" } else { "FAIL" };
        let limit = match check.limit {
            Limit::AtMost(max) => format!("/ {max}"),
            Limit::Exactly(count) => format!("= {count}"),
        };
        let _ = writeln!(
            text,
            "{status}  {name:<width$} {used:>6} {limit:<7} {unit:<10}  {parts}",
            name = check.name(),
            used = check.used,
            unit = check.kind.unit(),
            parts = summarise(&check.parts),
        );
        if !check.passed() {
            let _ = writeln!(text, "      {}: {}", check.kind.code(), advice(check));
        }
    }
    if report.controls.is_empty() {
        text.push_str("pass  controls   controls/budgets/ agrees with lint.toml\n");
    }
    for divergence in &report.controls {
        let _ = writeln!(text, "FAIL  controls   {}: {divergence}", Divergence::CODE);
    }
    text
}

fn summarise(parts: &[Part]) -> String {
    let mut shown: Vec<String> = parts
        .iter()
        .take(SHOWN_PARTS)
        .map(|part| format!("{} {}", part.label, part.amount))
        .collect();
    if parts.len() > SHOWN_PARTS {
        shown.push(format!("and {} more", parts.len() - SHOWN_PARTS));
    }
    shown.join(", ")
}

fn advice(check: &Check) -> String {
    match (check.kind, check.limit) {
        (CheckKind::Expect, Limit::Exactly(count)) if check.used > count => format!(
            "set expect.count = {} in lint.toml in the same change, and give each new #[expect] its reason",
            check.used
        ),
        (CheckKind::Expect, _) => format!("lower expect.count to {} in lint.toml", check.used),
        (CheckKind::SourceLines, _) => "split the files above".to_owned(),
        (CheckKind::Docs, Limit::AtMost(max)) => format!(
            "{node} holds {used} words in its own .md files, over its limit of {max}; \
             move detail into a subdirectory of {node}, or raise docs.max_words in lint.toml \
             in a change that says why",
            node = check.name(),
            used = check.used,
        ),
        _ => format!(
            "trim the files above, or raise {}.max_words in lint.toml in a change that says why",
            check.kind.name().replace('-', "_")
        ),
    }
}

pub(crate) fn render_failure(code: &str, error: &dyn Error) -> String {
    let mut text = format!("error: {code}: {error}");
    let mut source = error.source();
    while let Some(cause) = source {
        let _ = write!(text, ": {cause}");
        source = cause.source();
    }
    text.push('\n');
    text
}

#[cfg(test)]
mod tests;
