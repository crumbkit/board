//! `board timing check`: each hot command's p90 on the personal board
//! against its budget, from the digest and the timing log, and with
//! `--record` a run of `/suites/board-speed-local`
//! (docs/architecture/performance/timing.md, The speed check).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

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
/// question. A command is silent until it is ready, then live for good
/// (docs/architecture/performance/timing.md, Silent and live).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Silent,
    Live,
}

impl Mode {
    pub(crate) fn of(command: &str, live: &BTreeSet<String>) -> Self {
        if live.contains(command) {
            Self::Live
        } else {
            Self::Silent
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Live => "live",
        }
    }
}

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
    let p50_us = percentile(&history.judged_us, 50);
    // Whether the p90, or the median where it has a budget, is over
    // `times` its budget.
    let past = |times: u64| {
        let over = |us: Option<u64>, ms: Option<u64>| {
            us.zip(ms).is_some_and(|(us, ms)| us > times * ms * 1000)
        };
        over(p90_us, Some(budget.ms)) || over(p50_us, budget.median_ms)
    };
    let state = if budget.session_start && history.killed > 0 {
        State::Thrice
    } else if (history.judged_us.len() as u64) < UNJUDGED_BELOW {
        State::Unjudged
    } else if past(3) {
        State::Thrice
    } else if past(1) {
        State::Over
    } else {
        State::Within
    };
    Judged {
        budget,
        runs: history.judged,
        p50_us,
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

/// One result of the run record: the command's case, its mode and what it
/// measured.
fn result(judged: &Judged, mode: Mode) -> Value {
    let budget = judged.budget;
    let mut measures = json!({"budget_ms": budget.ms, "judged": judged.runs,
        "killed": judged.killed, "mode": mode.name()});
    if let Some(median) = budget.median_ms {
        measures["median_budget_ms"] = json!(median);
    }
    for (name, us) in [("p50_ms", judged.p50_us), ("p90_ms", judged.p90_us)] {
        if let Some(us) = us {
            measures[name] = json!(ms(us));
        }
    }
    let (outcome, note) = match judged.state {
        State::Within => ("pass", None),
        State::Unjudged => (
            "skipped",
            Some(format!(
                "{} judged runs, fewer than {UNJUDGED_BELOW}",
                judged.runs
            )),
        ),
        State::Over => ("fail", Some(over(judged))),
        State::Thrice => ("fail", Some(thrice(judged))),
    };
    let mut result = json!({"case": budget.command, "outcome": outcome, "measures": measures});
    if let Some(note) = note {
        result["note"] = json!(note);
    }
    result
}

/// The measure over its budget, `times` over: its name, its value in ms
/// and the budget's own name. The p90 when it is over, else the median.
fn measure_over(judged: &Judged, times: u64) -> (&'static str, f64, String) {
    let budget = judged.budget;
    let p90_over = judged
        .p90_us
        .is_some_and(|us| us > times * budget.ms * 1000);
    match budget.median_ms {
        Some(median) if !p90_over => (
            "p50",
            judged.p50_us.map_or(0.0, ms),
            format!("{median} ms median budget"),
        ),
        _ => (
            "p90",
            judged.p90_us.map_or(0.0, ms),
            format!("{} ms budget", budget.ms),
        ),
    }
}

fn over(judged: &Judged) -> String {
    let (name, value, budget) = measure_over(judged, 1);
    format!("{name} {value} ms is over the {budget}")
}

fn thrice(judged: &Judged) -> String {
    let budget = judged.budget;
    if budget.session_start && judged.killed > 0 {
        format!("{} runs were killed at a session's start", judged.killed)
    } else {
        let (name, value, budget) = measure_over(judged, 3);
        format!("{name} {value} ms is over three times the {budget}")
    }
}

/// How many live commands are over budget, and how many of their runs were
/// killed. A silent command counts in neither, so a zero rule on them
/// fails only for a live command.
pub(crate) fn counts(judged: &[Judged], live: &BTreeSet<String>) -> (u64, u64) {
    let live: Vec<&Judged> = (judged.iter())
        .filter(|judged| live.contains(judged.budget.command))
        .collect();
    let over = (live.iter())
        .filter(|judged| matches!(judged.state, State::Over | State::Thrice))
        .count() as u64;
    (over, live.iter().map(|judged| judged.killed).sum())
}

/// The machine a run was on, as a run record names it.
pub(crate) struct Host {
    /// The machine's host name, when it has one, so a reader with several
    /// Macs can tell their runs apart.
    pub(crate) name: Option<String>,
    pub(crate) os: &'static str,
    pub(crate) arch: &'static str,
    pub(crate) cpus: u64,
}

/// The `runs/run/v1` record of a check judged at `at`, an RFC 3339 time, as
/// the build `build`.
pub(crate) fn run_record(
    judged: &[Judged],
    live: &BTreeSet<String>,
    at: &str,
    build: &str,
    host: &Host,
) -> Value {
    let (over, killed) = counts(judged, live);
    let (all_over, all_killed) = counts(judged, &all(judged));
    let (silent_over, silent_killed) = (all_over - over, all_killed - killed);
    let on = host.name.as_ref().map_or_else(
        || "an unnamed machine".to_owned(),
        |name| format!("the machine {name}"),
    );
    json!({
        "setup": format!("board timing check --record on {on}"),
        "$type": "runs/run/v1",
        "suite": LOCAL_SUITE,
        "started": at,
        "finished": at,
        "scope": "full",
        "host": {"os": host.os, "arch": host.arch, "logical_cpus": host.cpus},
        "versions": {"board": build},
        "dirty": build.contains(".dirty."),
        "measures": {"over_budget": over, "killed": killed,
            "silent_over_budget": silent_over, "silent_killed": silent_killed},
        "results": (judged.iter())
            .map(|judged| result(judged, Mode::of(judged.budget.command, live)))
            .collect::<Vec<_>>(),
        "summary": format!("{over} live commands over budget and {killed} of their runs killed in the last day; {silent_over} silent commands over budget"),
        "written_by": "board/timing-check",
    })
}

/// Every command `judged` names.
fn all(judged: &[Judged]) -> BTreeSet<String> {
    (judged.iter())
        .map(|judged| judged.budget.command.to_owned())
        .collect()
}

/// The suite every local check records a run of.
pub(crate) fn local_suite() -> Value {
    json!({
        "$type": "runs/suite/v1",
        "kind": "benchmark",
        "method": "board timing check --record judges each hot command's p90, and its median where it has a median budget, over its last 20 top-level runs on the personal board, at a load no higher than the core count, against its budget in docs/architecture/performance/budgets.md. Each result names the command's mode; the run's measures count only live commands, and the silent_ measures the rest.",
        "command": "board timing check --record",
        "subjects": ["crumbkit/board"],
        "measures": [
            {"name": "over_budget", "unit": "commands", "description": "Live commands over their budget."},
            {"name": "killed", "unit": "runs", "description": "Runs of live commands killed in the last day."},
            {"name": "silent_over_budget", "unit": "commands", "description": "Silent commands over their budget."},
            {"name": "silent_killed", "unit": "runs", "description": "Runs of silent commands killed in the last day."}
        ]
    })
}

/// How many lines of Board changed since an over-budget command's task
/// opened, and how many may change before the owner is asked.
pub(crate) trait Grace {
    fn allowed(&self) -> u64;
    /// The lines changed since `opened`, when the run the task names
    /// started, when known.
    fn changed_since(&self, opened: jiff::Timestamp) -> Option<u64>;
}

/// When a check asks, the session it asks as, when it has one, and the
/// cache its reads use.
pub(crate) struct Asker<'a> {
    pub(crate) at: &'a str,
    pub(crate) session: Option<&'a str>,
    pub(crate) cache: Option<&'a Path>,
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

/// The question of one overflow: the command's, named by the run that
/// opened or reopened its task, so each overflow can ask once.
fn question_path(command: &str, opened: &LogicalPath) -> String {
    let run = opened.as_str().rsplit('/').next().unwrap_or_default();
    format!("{QUESTIONS}/{PREFIX}{}-{run}", slug(command))
}

/// Where speed questions are filed, and the start of each one's name.
const QUESTIONS: &str = "/projects/board/questions";
const PREFIX: &str = "board-speed-";

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

fn question(judged: &Judged, run: &LogicalPath, asked: &str, by: &Value, task: &str) -> Value {
    let budget = judged.budget;
    json!({
        "blocks": task,
        "$type": "plans/question/v1",
        "question": format!("Board's {} is still over its {} ms budget after the grace. Should its budget change, or should it be retired?", budget.command, budget.ms),
        "why": format!("{run} found it over budget, and the lines changed in /repos/board since its task opened are past overflow-agent-grace."),
        "by": by,
        "asked": asked,
        "status": "open",
        "mount": "/repos/board",
    })
}

/// Whether any speed question is open: its status is open and no answer,
/// which plans keeps at the question's id under answers/, names it.
fn asking(board: &Board, cache: Option<&Path>) -> bool {
    let held = |path: &str| (logical(path).ok()).and_then(|path| board.get(&path).ok());
    let Ok(listed) = (logical(QUESTIONS)).map(|questions| board.ls(&questions, cache)) else {
        return false;
    };
    let Ok(listed) = listed else {
        return false;
    };
    (listed.listed.iter()).any(|listed| {
        let board_ops::Listed::Entry {
            logical: Some(logical),
            ..
        } = listed
        else {
            return false;
        };
        let name = logical.as_str().rsplit('/').next().unwrap_or_default();
        if !name.starts_with(PREFIX) {
            return false;
        }
        let open = held(&format!("{QUESTIONS}/{name}"))
            .and_then(|got| serde_json::from_slice::<Value>(&got.bytes).ok())
            .is_some_and(|question| question["status"] == "open");
        open && held(&format!("/projects/board/answers/{name}")).is_none()
    })
}

/// When the run at `run` started, when it reads.
fn started(board: &Board, run: &LogicalPath) -> Option<jiff::Timestamp> {
    let got = board.get(run).ok()?;
    let record: Value = serde_json::from_slice(&got.bytes).ok()?;
    record["started"].as_str()?.parse().ok()
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

/// Whether the task text `held` is open, as its front matter says.
fn is_open(held: &str) -> bool {
    (held.lines())
        .find_map(|line| line.strip_prefix("status: "))
        .is_some_and(|status| status.trim().trim_matches('"') == "open")
}

/// For each command in `live` over budget: opens a task, where none exists
/// or reopens one that is closed, naming this run; gives an open task the
/// title of a command now far too slow; and once `grace` has passed since
/// the run that opened or reopened the task, files that overflow's one
/// question, asked as `asker`'s session, and only while no other speed
/// question is open. A check with no session files none. `run` is the run
/// that judged them.
pub(crate) fn alert(
    board: &Board,
    judged: &[Judged],
    live: &BTreeSet<String>,
    run: &LogicalPath,
    asker: &Asker<'_>,
    grace: &dyn Grace,
) -> Result<Alerted, PutError> {
    let mut alerted = Alerted::default();
    for judged in judged {
        let over = matches!(judged.state, State::Over | State::Thrice);
        if !over || !live.contains(judged.budget.command) {
            continue;
        }
        let command = judged.budget.command;
        let path = logical(&task_path(command))?;
        let text = task(judged, run);
        let held = existing(board, &path);
        let reopen = match &held {
            None => Some(Expect::Absent),
            Some((held, Some(blob))) if !is_open(&String::from_utf8_lossy(held)) => {
                Some(Expect::Blob(*blob))
            }
            Some(_) => None,
        };
        if let Some(expect) = reopen {
            match board.put_file(&path, expect, text.as_bytes()) {
                Ok(_) => alerted.tasks.push(path.to_string()),
                // Another check opened or reopened it first.
                Err(PutError::Stale { .. }) => {}
                Err(error) => return Err(error),
            }
            continue;
        }
        let Some((held, blob)) = held else {
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
        let Some(session) = asker.session else {
            continue;
        };
        let Some(opened) = (held.lines())
            .find_map(|line| line.strip_prefix("refs: "))
            .and_then(|refs| serde_json::from_str::<Vec<String>>(refs).ok())
            .and_then(|refs| refs.into_iter().next())
            .and_then(|run| run.parse::<LogicalPath>().ok())
        else {
            continue;
        };
        let past = started(board, &opened)
            .and_then(|at| grace.changed_since(at))
            .is_some_and(|changed| changed > grace.allowed());
        let path = logical(&question_path(command, &opened))?;
        if past && board.get(&path).is_err() && !asking(board, asker.cache) {
            let by = json!({"program": "board", "session": session});
            let task = task_path(command);
            let record = question(judged, run, asker.at, &by, &task).to_string();
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
