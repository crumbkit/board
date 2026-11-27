//! When a command goes live: once it has enough judged results, locally
//! and on CI, and its over-budget results rarely clear at the next one. A
//! live command stays live, and the first one writes the speed rules
//! (docs/architecture/performance/timing.md, Silent and live).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use board_model::{Kind, LogicalPath};
use board_ops::{Board, Expect, Listed, PutError};
use serde_json::{Value, json};

use super::check::{Judged, LOCAL_SUITE, Mode, State};
use super::import::CI_SUITE;

/// How many judged results a command needs before it goes live: `local`
/// from local checks, and `ci` from CI runs once CI has timed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Thresholds {
    pub(crate) local: usize,
    pub(crate) ci: usize,
}

/// The owner's thresholds (/projects/board/answers/20261125T0018Z-speed-check-live).
pub(crate) const READY: Thresholds = Thresholds { local: 100, ci: 14 };

/// The most runs of each suite a check reads, newest first: enough for
/// [`READY`] and the flapping it measures. This is a soft limit: a command
/// that flaps only in older runs is not seen; a check that needs more
/// history reads a run trend instead.
pub(crate) const READ: usize = 120;

/// One result of a command in one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Within,
    Over,
    Unjudged,
    /// Board refused the command in a CI run.
    Refused,
}

/// What earlier runs say: the commands the newest local run names live,
/// and each command's results in the local and CI runs, oldest first.
#[derive(Debug, Default)]
pub(crate) struct Past {
    pub(crate) live: BTreeSet<String>,
    pub(crate) local: BTreeMap<String, Vec<Outcome>>,
    pub(crate) ci: BTreeMap<String, Vec<Outcome>>,
}

impl Past {
    /// The past of `local` and `ci` runs, each oldest first. The live
    /// commands are those of the newest local run that names modes, so a
    /// run from a build that names none leaves them live.
    pub(crate) fn of(local: &[Value], ci: &[Value]) -> Self {
        Self {
            live: local.iter().rev().find_map(live_in).unwrap_or_default(),
            local: outcomes(local),
            ci: outcomes(ci),
        }
    }
}

/// The commands `run` names live, or none when it names no command's mode.
pub(crate) fn live_in(run: &Value) -> Option<BTreeSet<String>> {
    let mut modes = results(run).filter(|result| result["measures"]["mode"].is_string());
    modes.next()?;
    Some(
        (results(run))
            .filter(|result| result["measures"]["mode"] == Mode::Live.name())
            .filter_map(|result| result["case"].as_str().map(str::to_owned))
            .collect(),
    )
}

fn results(run: &Value) -> impl Iterator<Item = &Value> {
    run["results"].as_array().into_iter().flatten()
}

/// Each command's outcomes in `runs`, in their order.
fn outcomes(runs: &[Value]) -> BTreeMap<String, Vec<Outcome>> {
    let mut outcomes: BTreeMap<String, Vec<Outcome>> = BTreeMap::new();
    for result in runs.iter().flat_map(results) {
        let Some(case) = result["case"].as_str() else {
            continue;
        };
        let outcome = match result["outcome"].as_str() {
            Some("pass") => Outcome::Within,
            Some("fail") => Outcome::Over,
            Some("error") => Outcome::Refused,
            _ => Outcome::Unjudged,
        };
        outcomes.entry(case.to_owned()).or_default().push(outcome);
    }
    outcomes
}

fn outcome(judged: &Judged) -> Outcome {
    match judged.state {
        State::Unjudged => Outcome::Unjudged,
        State::Within => Outcome::Within,
        State::Over | State::Thrice => Outcome::Over,
    }
}

/// How many of `outcomes` were judged, how many were over, and how many of
/// those the next judged result cleared.
fn tally(outcomes: &[Outcome]) -> (usize, usize, usize) {
    let judged: Vec<Outcome> = (outcomes.iter().copied())
        .filter(|outcome| matches!(outcome, Outcome::Within | Outcome::Over))
        .collect();
    let over = judged.iter().filter(|o| **o == Outcome::Over).count();
    let cleared = (judged.windows(2))
        .filter(|pair| pair == &[Outcome::Over, Outcome::Within])
        .count();
    (judged.len(), over, cleared)
}

/// The commands live after this check judged `judged`: every command
/// `past` names live, and each that is ready now.
pub(crate) fn live(past: &Past, judged: &[Judged], thresholds: &Thresholds) -> BTreeSet<String> {
    let mut live = past.live.clone();
    for judged in judged {
        let command = judged.budget.command;
        let mut local = past.local.get(command).cloned().unwrap_or_default();
        local.push(outcome(judged));
        let ci = past.ci.get(command).map_or(&[][..], Vec::as_slice);
        let (local_judged, local_over, local_cleared) = tally(&local);
        let (ci_judged, ci_over, ci_cleared) = tally(ci);
        let enough_ci = ci_judged == 0 || ci_judged >= thresholds.ci;
        // A command CI's newest run refused waits until CI times it again.
        let refused = ci.last() == Some(&Outcome::Refused);
        let over = local_over + ci_over;
        // Under 1 in 10 of the over-budget results cleared at the next one.
        let steady = (local_cleared + ci_cleared) * 10 < over || over == 0;
        if local_judged >= thresholds.local && enough_ci && steady && !refused {
            live.insert(command.to_owned());
        }
    }
    live
}

/// The newest `count` runs of `suite`, oldest first, read through `cache`.
/// A suite with no runs, or a run that does not read as JSON, gives
/// nothing for it.
pub(crate) fn runs(board: &Board, suite: &str, cache: Option<&Path>, count: usize) -> Vec<Value> {
    let Ok(directory) = format!("{suite}/runs").parse::<LogicalPath>() else {
        return Vec::new();
    };
    let Ok(listed) = board.ls(&directory, cache) else {
        return Vec::new();
    };
    let mut names: Vec<(u64, String, LogicalPath)> = (listed.listed.into_iter())
        .filter_map(|listed| match listed {
            Listed::Entry {
                name,
                logical: Some(logical),
                kind: Kind::Record,
                ..
            } => Some((name.parse().unwrap_or(0), name, logical)),
            _ => None,
        })
        .collect();
    // A CI run is named by its run id, so a number sorts it; a local run
    // is named by its time, so its name does.
    names.sort();
    let skip = names.len().saturating_sub(count);
    (names.into_iter().skip(skip))
        .filter_map(|(_, _, path)| board.get(&path).ok())
        .filter_map(|got| serde_json::from_slice(&got.bytes).ok())
        .collect()
}

/// The speed rules, one per suite, since a zero rule reads one suite.
const RULES: [(&str, &str); 2] = [
    ("/rules/board/speed-budgets", LOCAL_SUITE),
    ("/rules/board/speed-budgets-ci", CI_SUITE),
];

fn rule(suite: &str) -> Value {
    let summary = if suite == LOCAL_SUITE {
        "Every live Board command meets its speed budget on the personal board."
    } else {
        "Every live Board command meets its speed budget in CI's timing run."
    };
    json!({
        "$type": "rules/rule/v1",
        "kind": "standing",
        "paths": ["*"],
        "requirement": format!("The latest run of {suite} reports no live command over its budget and no live command's run killed. A command over its budget has a task at /tasks/board-speed-<command>.md; land the change it names."),
        "purpose": "Board measures its own command times so that a slowdown never grows unseen (/projects/board/decisions/20261123T1055Z-board-reliably-quick).",
        "summary": summary,
        "checker": {"check": "zero", "suite": suite, "measures": ["over_budget", "killed"]},
    })
}

/// Writes each speed rule `board` lacks, and returns the paths written.
pub(crate) fn rules(board: &Board) -> Result<Vec<String>, PutError> {
    let mut written = Vec::new();
    for (path, suite) in RULES {
        let path: LogicalPath = path.parse()?;
        if board.get(&path).is_ok() {
            continue;
        }
        match board.put(&path, Expect::Absent, rule(suite).to_string().as_bytes()) {
            Ok(_) => written.push(path.to_string()),
            // Another check wrote it first.
            Err(PutError::Stale { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(written)
}
