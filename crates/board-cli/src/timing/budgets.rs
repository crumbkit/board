//! How fast each hot command must be on the personal board
//! (docs/architecture/performance/budgets.md, Command budgets).

/// One command's budget: the p90 of its last judged runs must not pass it,
/// nor their median pass `median_ms`, when the command has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Budget {
    /// The command as the timing log names it: a capability such as
    /// `plans.open`, or a command such as `get`.
    pub(crate) command: &'static str,
    pub(crate) ms: u64,
    /// The most the median may take, for a command whose usual run must be
    /// quicker than its slow ones.
    pub(crate) median_ms: Option<u64>,
    /// Whether a session's start runs it, where a hook kills a slow run.
    pub(crate) session_start: bool,
}

const fn budget(command: &'static str, ms: u64, session_start: bool) -> Budget {
    Budget {
        command,
        ms,
        median_ms: None,
        session_start,
    }
}

const fn median(budget: Budget, ms: u64) -> Budget {
    Budget {
        median_ms: Some(ms),
        ..budget
    }
}

/// The owner set the first eleven on 23 November 2026: 100 ms for queries,
/// checks and what a session's start runs, and half the first proposal for
/// the rest. On 26 November he added the medians, `ls`, `changes.advance`
/// and `changes.brief` (/projects/board/answers/20261125T0018Z-speed-check-live).
pub(crate) const BUDGETS: [Budget; 14] = [
    budget("get", 25, false),
    budget("put", 150, false),
    budget("query", 100, false),
    budget("ls", 500, true),
    budget("plans.open", 250, false),
    budget("plans.waiting", 100, true),
    budget("orders.pinned", 100, true),
    budget("rules.check", 100, true),
    budget("rules.for-path", 500, false),
    budget("tasks.available", 1_500, false),
    budget("changes.open", 1_500, false),
    median(budget("changes.next", 10_000, false), 2_000),
    median(budget("changes.advance", 10_000, false), 2_000),
    median(budget("changes.brief", 10_000, false), 2_000),
];
