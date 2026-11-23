//! `board timing check`: each hot command's p90 on the personal board
//! against its budget, from the digest and the timing log, and with
//! `--record` a run of `/suites/board-speed-local`
//! (docs/architecture/performance/timing.md, The speed check).

use std::collections::BTreeMap;

use board_model::{LogicalPath, Object};
use board_ops::{Board, Expect, PutError, Timings};
use serde_json::{Value, json};

use super::budgets::{BUDGETS, Budget};
use super::digest::{self, KEPT, Run};

/// A command with fewer judged runs than this is unjudged.
pub(crate) const UNJUDGED_BELOW: u64 = 5;
/// How far back a killed run counts.
const KILLED_WITHIN_MS: u64 = 24 * 3_600_000;
/// The suite each local check records a run of.
pub(crate) const LOCAL_SUITE: &str = "/suites/board-speed-local";

/// Whether an over-budget command opens a task and, after the grace, a
/// question. The check stays silent until the owner lands the change that
/// makes it live, once the silent runs show the budgets hold
/// (docs/architecture/performance/timing.md, Silent and live).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Silent,
    Live,
}

pub(crate) const MODE: Mode = Mode::Silent;

/// One command's recent runs: the judged times in the order they ran, the
/// most the check reads, how many were judged in all, and how many runs
/// were killed in the last day.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct History {
    pub(crate) judged_us: Vec<u64>,
    pub(crate) judged: u64,
    pub(crate) killed: u64,
}

impl History {
    fn add(&mut self, run: &Run, now_ms: u64) {
        if run.judged() {
            self.judged += 1;
            self.judged_us.push(run.total_us);
            self.trim();
        }
        if run.killed && run.at_ms.saturating_add(KILLED_WITHIN_MS) >= now_ms {
            self.killed += 1;
        }
    }

    fn trim(&mut self) {
        let over = self.judged_us.len().saturating_sub(KEPT);
        self.judged_us.drain(..over);
    }
}

/// What the check read: each command's history, and the lines of the log
/// and digest it skipped because they do not parse.
#[derive(Debug, Default)]
pub(crate) struct Read {
    pub(crate) histories: BTreeMap<String, History>,
    pub(crate) skipped: usize,
}

/// The top-level runs on the board at `root` in `timings`: the digest's
/// hours from logs no longer kept, then both logs, oldest first.
pub(crate) fn read(timings: &Timings, root: &str, now_ms: u64) -> Read {
    let (previous, skipped_previous) = digest::runs(&timings.previous);
    let (current, skipped_current) = digest::runs(&timings.current);
    let (mut hours, skipped_digest) = digest::hours(&timings.digest);
    let kept_from = (previous.first().or(current.first())).map_or(u64::MAX, |run| run.at_ms);
    hours.retain(|hour| hour.from_ms < kept_from && hour.root.as_deref() == Some(root));
    hours.sort_by_key(|hour| hour.start_ms);
    let mut histories: BTreeMap<String, History> = BTreeMap::new();
    for hour in hours {
        let history = histories.entry(hour.command).or_default();
        history.judged += hour.judged;
        history.judged_us.extend(hour.judged_us);
        history.trim();
        if hour.start_ms.saturating_add(3_600_000 + KILLED_WITHIN_MS) >= now_ms {
            history.killed += hour.killed;
        }
    }
    let ours = |run: &&Run| run.depth == 0 && run.root.as_deref() == Some(root);
    for run in previous.iter().chain(&current).filter(ours) {
        histories
            .entry(run.command.clone())
            .or_default()
            .add(run, now_ms);
    }
    Read {
        histories,
        skipped: skipped_previous + skipped_current + skipped_digest,
    }
}

/// Where a command stands against its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Unjudged,
    Within,
    Over,
    /// Over three times its budget, or a session-start command killed.
    Thrice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Judged {
    pub(crate) budget: Budget,
    /// How many runs were judged in all.
    pub(crate) runs: u64,
    pub(crate) p50_us: Option<u64>,
    pub(crate) p90_us: Option<u64>,
    pub(crate) killed: u64,
    pub(crate) state: State,
}

/// The `percent`th percentile of `times` by nearest rank.
fn percentile(times: &[u64], percent: usize) -> Option<u64> {
    let mut sorted = times.to_vec();
    sorted.sort_unstable();
    let rank = (sorted.len() * percent).div_ceil(100);
    sorted.get(rank.checked_sub(1)?).copied()
}

/// `budget`'s command judged on `history`.
pub(crate) fn judge(budget: Budget, history: Option<&History>) -> Judged {
    let empty = History::default();
    let history = history.unwrap_or(&empty);
    let p90_us = percentile(&history.judged_us, 90);
    let limit = budget.ms * 1000;
    let state = match p90_us {
        _ if budget.session_start && history.killed > 0 => State::Thrice,
        _ if (history.judged_us.len() as u64) < UNJUDGED_BELOW => State::Unjudged,
        Some(p90) if p90 > 3 * limit => State::Thrice,
        Some(p90) if p90 > limit => State::Over,
        _ => State::Within,
    };
    Judged {
        budget,
        runs: history.judged,
        p50_us: percentile(&history.judged_us, 50),
        p90_us,
        killed: history.killed,
        state,
    }
}

/// Every budgeted command judged on what `read` found.
pub(crate) fn judge_all(read: &Read, budgets: &[Budget]) -> Vec<Judged> {
    (budgets.iter())
        .map(|budget| judge(*budget, read.histories.get(budget.command)))
        .collect()
}

/// `us` in milliseconds to a tenth, held to about 71 minutes, which a
/// float holds exactly.
pub(crate) fn ms(us: u64) -> f64 {
    let us = f64::from(u32::try_from(us).unwrap_or(u32::MAX));
    (us / 100.0).round() / 10.0
}

/// One result of the run record: the command's case and what it measured.
fn result(judged: &Judged) -> Value {
    let budget = judged.budget;
    let mut measures =
        json!({"budget_ms": budget.ms, "judged": judged.runs, "killed": judged.killed});
    for (name, us) in [("p50_ms", judged.p50_us), ("p90_ms", judged.p90_us)] {
        if let Some(us) = us {
            measures[name] = json!(ms(us));
        }
    }
    let p90 = judged.p90_us.map_or(0.0, ms);
    let (outcome, note) = match judged.state {
        State::Within => ("pass", None),
        State::Unjudged => (
            "skipped",
            Some(format!(
                "{} judged runs, fewer than {UNJUDGED_BELOW}",
                judged.runs
            )),
        ),
        State::Over => (
            "fail",
            Some(format!("p90 {p90} ms is over the {} ms budget", budget.ms)),
        ),
        State::Thrice => ("fail", Some(thrice(judged))),
    };
    let mut result = json!({"case": budget.command, "outcome": outcome, "measures": measures});
    if let Some(note) = note {
        result["note"] = json!(note);
    }
    result
}

fn thrice(judged: &Judged) -> String {
    let budget = judged.budget;
    if budget.session_start && judged.killed > 0 {
        format!("{} runs were killed at a session's start", judged.killed)
    } else {
        format!(
            "p90 {} ms is over three times the {} ms budget",
            judged.p90_us.map_or(0.0, ms),
            budget.ms
        )
    }
}

/// How many commands are over budget, and how many runs were killed.
pub(crate) fn counts(judged: &[Judged]) -> (u64, u64) {
    let over = (judged.iter())
        .filter(|judged| matches!(judged.state, State::Over | State::Thrice))
        .count() as u64;
    (over, judged.iter().map(|judged| judged.killed).sum())
}

/// The machine a run was on, as a run record names it.
pub(crate) struct Host {
    pub(crate) os: &'static str,
    pub(crate) arch: &'static str,
    pub(crate) cpus: u64,
}

/// The `runs/run/v1` record of a check judged at `at`, an RFC 3339 time, as
/// the build `build`.
pub(crate) fn run_record(judged: &[Judged], at: &str, build: &str, host: &Host) -> Value {
    let (over, killed) = counts(judged);
    json!({
        "$type": "runs/run/v1",
        "suite": LOCAL_SUITE,
        "started": at,
        "finished": at,
        "scope": "full",
        "host": {"os": host.os, "arch": host.arch, "logical_cpus": host.cpus},
        "versions": {"board": build},
        "measures": {"over_budget": over, "killed": killed},
        "results": judged.iter().map(result).collect::<Vec<_>>(),
        "summary": format!("{over} commands over budget, {killed} runs killed in the last day"),
        "written_by": "board/timing-check",
    })
}

/// The suite every local check records a run of.
pub(crate) fn local_suite() -> Value {
    json!({
        "$type": "runs/suite/v1",
        "kind": "benchmark",
        "method": "board timing check --record judges each hot command's p90 over its last 20 top-level runs on the personal board, at a load no higher than the core count, against its budget in docs/architecture/performance/budgets.md.",
        "command": "board timing check --record",
        "subjects": ["crumbkit/board"],
        "measures": [
            {"name": "over_budget", "unit": "commands", "description": "Commands whose p90 is over their budget."},
            {"name": "killed", "unit": "runs", "description": "Runs killed in the last day."}
        ]
    })
}

/// How many lines of Board changed since an over-budget command's task
/// opened, and how many may change before the owner is asked.
pub(crate) trait Grace {
    fn allowed(&self) -> u64;
    /// The lines changed since `run`, the run the task names, when known.
    fn changed_since(&self, run: &LogicalPath) -> Option<u64>;
}

/// The grace while nothing measures the lines changed: no question is
/// filed. The change that makes the check live measures them, and names the
/// session that asks.
pub(crate) struct Unmeasured;

impl Grace for Unmeasured {
    fn allowed(&self) -> u64 {
        u64::MAX
    }

    fn changed_since(&self, _: &LogicalPath) -> Option<u64> {
        None
    }
}

/// What a live check wrote: tasks opened or retitled, and questions filed.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Alerted {
    pub(crate) tasks: Vec<String>,
    pub(crate) questions: Vec<String>,
}

fn slug(command: &str) -> String {
    command.replace('.', "-")
}

fn task_path(command: &str) -> String {
    format!("/tasks/board-speed-{}.md", slug(command))
}

fn question_path(command: &str) -> String {
    format!("/projects/board/questions/board-speed-{}", slug(command))
}

fn title(judged: &Judged) -> String {
    let budget = judged.budget;
    match judged.state {
        State::Thrice => format!(
            "Board's {} is far too slow, so the lead names it to the owner: {}",
            budget.command,
            thrice(judged)
        ),
        _ => format!(
            "Board's {} is over its {} ms budget",
            budget.command, budget.ms
        ),
    }
}

fn task(judged: &Judged, run: &LogicalPath) -> String {
    let title = serde_json::to_string(&title(judged)).unwrap_or_default();
    let refs = serde_json::to_string(&[run.as_str()]).unwrap_or_default();
    format!(
        "---\n$type: \"tasks/task/v3\"\ntitle: {title}\nstatus: \"open\"\nrefs: {refs}\n---\n\n\
         `board timing check` found {} over its budget in {run}. The open change that makes this command faster fixes it; close this task with tasks.done once the command is within its budget.\n",
        judged.budget.command
    )
}

fn question(judged: &Judged, run: &LogicalPath, asked: &str, by: &Value) -> Value {
    let budget = judged.budget;
    json!({
        "$type": "plans/question/v1",
        "question": format!("Board's {} is still over its {} ms budget after the grace. Should its budget change, or should it be retired?", budget.command, budget.ms),
        "why": format!("{run} found it over budget, and the lines changed in /repos/board since its task opened are past overflow-agent-grace."),
        "by": by,
        "asked": asked,
        "status": "open",
        "mount": "/repos/board",
    })
}

/// The task's bytes and blob, when it exists.
fn existing(board: &Board, path: &LogicalPath) -> Option<(Vec<u8>, Option<board_model::ObjectId>)> {
    let got = board.get(path).ok()?;
    let blob = match got.location.object {
        Object::Blob(blob) => Some(blob),
        Object::Directory => None,
    };
    Some((got.bytes, blob))
}

fn logical(path: &str) -> Result<LogicalPath, PutError> {
    Ok(path.parse()?)
}

/// Opens a task for each command over budget, only where none exists; gives
/// an open task the title of a command now far too slow; and files one
/// question for each command still over once `grace` has passed since its
/// task opened, only where none exists, asked by `by` at `asked`. `run` is
/// the run that judged them.
pub(crate) fn alert(
    board: &Board,
    judged: &[Judged],
    run: &LogicalPath,
    (asked, by): (&str, &Value),
    grace: &dyn Grace,
) -> Result<Alerted, PutError> {
    let mut alerted = Alerted::default();
    for judged in judged {
        if !matches!(judged.state, State::Over | State::Thrice) {
            continue;
        }
        let command = judged.budget.command;
        let path = logical(&task_path(command))?;
        let text = task(judged, run);
        let Some((held, blob)) = existing(board, &path) else {
            match board.put_file(&path, Expect::Absent, text.as_bytes()) {
                Ok(_) => alerted.tasks.push(path.to_string()),
                // Another check opened it first.
                Err(PutError::Stale { .. }) => {}
                Err(error) => return Err(error),
            }
            continue;
        };
        let titled = format!(
            "title: {}\n",
            serde_json::to_string(&title(judged)).unwrap_or_default()
        );
        let held = String::from_utf8_lossy(&held);
        if judged.state == State::Thrice
            && !held.contains(&titled)
            && let Some(blob) = blob
        {
            let retitled: String = (held.lines())
                .map(|line| {
                    if line.starts_with("title: ") {
                        titled.clone()
                    } else {
                        format!("{line}\n")
                    }
                })
                .collect();
            if board
                .put_file(&path, Expect::Blob(blob), retitled.as_bytes())
                .is_ok()
            {
                alerted.tasks.push(path.to_string());
            }
        }
        let opened_by = (held.lines())
            .find_map(|line| line.strip_prefix("refs: "))
            .and_then(|refs| serde_json::from_str::<Vec<String>>(refs).ok())
            .and_then(|refs| refs.into_iter().next())
            .and_then(|run| run.parse::<LogicalPath>().ok());
        let past = opened_by
            .and_then(|opened| grace.changed_since(&opened))
            .is_some_and(|changed| changed > grace.allowed());
        if past {
            let path = logical(&question_path(command))?;
            let record = question(judged, run, asked, by).to_string();
            match board.put(&path, Expect::Absent, record.as_bytes()) {
                Ok(_) => alerted.questions.push(path.to_string()),
                Err(PutError::Stale { .. }) => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(alerted)
}

/// The budgets every check judges.
pub(crate) fn budgets() -> &'static [Budget] {
    &BUDGETS
}
