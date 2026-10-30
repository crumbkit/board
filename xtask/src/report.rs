//! The lint's report, and the one place this binary prints.

use std::error::Error;
use std::fmt::Write as _;
use std::time::Duration;

use crate::bench::against::{Arm, Compared, Verdict};
use crate::bench::{Timed, Took};
use crate::controls::Divergence;
use crate::timing::Summary;

/// How many parts a report line names before summarising the rest.
const SHOWN_PARTS: usize = 5;

const USAGE: &str = "usage: cargo xtask lint
       cargo xtask bench [--board <dir>] [--records <n>] [--against <board> [--runs <n>]]
       cargo xtask timing <command> [--last <n>]
       cargo xtask profile [--save <file>] -- <board arguments>
";

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
    PlanLeaf,
    PlanLinks,
    SourceLines,
    Expect,
    Signing,
}

impl CheckKind {
    fn name(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::DocsIndex => "docs-index",
            Self::Docs => "docs",
            Self::Plan => "plan",
            Self::PlanLeaf => "plan-leaves",
            Self::PlanLinks => "plan-links",
            Self::SourceLines => "source",
            Self::Expect => "expect",
            Self::Signing => "signing",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::Context | Self::DocsIndex | Self::Docs | Self::Plan | Self::PlanLeaf => "words",
            Self::PlanLinks => "problems",
            Self::SourceLines => "lines",
            Self::Expect => "attributes",
            Self::Signing => "literals",
        }
    }

    /// The code a failure of this check carries.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Context | Self::DocsIndex | Self::Docs | Self::Plan | Self::PlanLeaf => {
                "xtask::budget::over_limit"
            }
            Self::PlanLinks => "xtask::plan::index_differs",
            Self::SourceLines => "xtask::source::too_long",
            Self::Expect => "xtask::expect::count_changed",
            Self::Signing => "xtask::signing::outside_owner",
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
    Against(&'a [String; 2], &'a [Compared]),
    Timing(&'a str, &'a Summary),
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
        Output::Against(versions, compared) => {
            print!(
                "{}",
                render_against(&[&versions[0], &versions[1]], compared)
            );
        }
        Output::Timing(command, summary) => print!("{}", render_timing(command, summary)),
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

/// The versions compared, then one line per row: its verdict, each arm's
/// median, the median round's change and the rounds new was slower in.
pub(crate) fn render_against(versions: &[&str; 2], compared: &[Compared]) -> String {
    let mut text = format!("base  {}\nnew   {}\n", versions[0], versions[1]);
    for row in compared {
        let verdict = match row.verdict {
            Verdict::Failed(arm, exit) => {
                let (status, arm) = match arm {
                    Arm::Base => ("skip", "base"),
                    Arm::New => ("FAIL", "new"),
                };
                let _ = writeln!(
                    text,
                    "{status:<6}  {arm} failed (exit {exit})  {}",
                    row.name
                );
                continue;
            }
            Verdict::Slower => "SLOWER",
            Verdict::Faster => "faster",
            Verdict::Same => "same",
        };
        let millis = |time: Duration| time.as_secs_f64() * 1_000.0;
        let _ = writeln!(
            text,
            "{verdict:<6}  base {:>8.1} ms  new {:>8.1} ms  {:>+6.1}%  {}/{} rounds slower  {}",
            millis(row.base),
            millis(row.new),
            row.delta * 100.0,
            row.slower,
            row.rounds,
            row.name,
        );
    }
    text
}

/// The runs summarised, then the median total and each phase's median.
pub(crate) fn render_timing(command: &str, summary: &Summary) -> String {
    let mut text = format!("{command}: {} runs of {}", summary.runs, summary.version);
    match summary.malformed {
        0 => {}
        1 => text.push_str("; 1 line of the log is not JSON"),
        lines => {
            let _ = write!(text, "; {lines} lines of the log are not JSON");
        }
    }
    let millis = |micros: u64| Duration::from_micros(micros).as_secs_f64() * 1_000.0;
    let _ = writeln!(text, "\n{:<10} {:>8.1} ms", "total", millis(summary.total));
    for (name, median, entered) in &summary.phases {
        let _ = writeln!(
            text,
            "{name:<10} {:>8.1} ms  {entered} runs",
            millis(*median)
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
            parts = summarise(check),
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

fn summarise(check: &Check) -> String {
    let parts = &check.parts;
    let mut shown: Vec<String> = parts
        .iter()
        .take(SHOWN_PARTS)
        .map(|part| match check.kind {
            CheckKind::PlanLinks | CheckKind::Signing => part.label.clone(),
            _ => format!("{} {}", part.label, part.amount),
        })
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
        (CheckKind::Signing, _) => "only the files signing.owners in lint.toml lists may name a \
             signing literal; use board_model's headers or the vectors, or move the code into an owner \
             (docs/architecture/writes/signing.md)"
            .to_owned(),
        (CheckKind::PlanLeaf, _) => format!(
            "split {leaf} into two milestones, or raise plan_leaves.max_words in lint.toml \
             in a change that says why",
            leaf = check.name(),
        ),
        (CheckKind::PlanLinks, _) => {
            "link each milestone leaf from the plan once, remove links to missing leaves, \
             and keep the plan directory flat"
                .to_owned()
        }
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
