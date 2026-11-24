//! How fast each hot command must be on the personal board
//! (docs/architecture/performance/budgets.md, Command budgets).

/// One command's budget: the p90 of its last judged runs must not pass it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Budget {
    /// The command as the timing log names it: a capability such as
    /// `plans.open`, or a command such as `get`.
    pub(crate) command: &'static str,
    pub(crate) ms: u64,
    /// Whether a session's start runs it, where a hook kills a slow run.
    pub(crate) session_start: bool,
}

const fn budget(command: &'static str, ms: u64, session_start: bool) -> Budget {
    Budget {
        command,
        ms,
        session_start,
    }
}

/// The owner set these on 23 November 2026: 100 ms for queries, checks and
/// what a session's start runs, and half the first proposal for the rest.
pub(crate) const BUDGETS: [Budget; 11] = [
    budget("get", 25, false),
    budget("put", 150, false),
    budget("query", 100, false),
    budget("plans.open", 250, false),
    budget("plans.waiting", 100, true),
    budget("orders.pinned", 100, true),
    budget("rules.check", 100, true),
    budget("rules.for-path", 500, false),
    budget("tasks.available", 1_500, false),
    budget("changes.open", 1_500, false),
    budget("changes.next", 10_000, false),
];
