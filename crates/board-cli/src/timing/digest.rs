//! The timing log's lines read back, and the per-hour digest that keeps the
//! runs of a log after rotation drops it
//! (docs/architecture/performance/timing.md, The digest).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The judged times a digest hour keeps for each command: as many as the
/// check reads.
pub(crate) const KEPT: usize = 20;

const HOUR_MS: u64 = 3_600_000;

/// One line of the timing log, as the check reads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
    pub(crate) at_ms: u64,
    pub(crate) root: Option<String>,
    /// The capability a call ran, or else the command.
    pub(crate) command: String,
    pub(crate) depth: u64,
    pub(crate) total_us: u64,
    pub(crate) load1: Option<f64>,
    pub(crate) cpus: Option<u64>,
    pub(crate) killed: bool,
}

impl Run {
    fn parse(line: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(line).ok()?;
        let number = |key: &str| value.get(key).and_then(Value::as_u64);
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            at_ms: number("at_ms")?,
            root: text("root"),
            command: text("capability").or_else(|| text("command"))?,
            depth: number("depth").unwrap_or_default(),
            total_us: number("total_us")?,
            load1: value.get("load1").and_then(Value::as_f64),
            cpus: number("cpus"),
            killed: value.get("killed").and_then(Value::as_bool) == Some(true),
        })
    }

    /// Whether the run counts toward its command's p90: it ended, and the
    /// machine's load was no higher than its core count.
    pub(crate) fn judged(&self) -> bool {
        let cores = |cpus: u64| f64::from(u32::try_from(cpus).unwrap_or(u32::MAX));
        let calm =
            matches!((self.load1, self.cpus), (Some(load), Some(cpus)) if load <= cores(cpus));
        !self.killed && calm
    }
}

/// The runs of `log` that parse, and how many lines did not.
pub(crate) fn runs(log: &[u8]) -> (Vec<Run>, usize) {
    let mut skipped = 0;
    let runs = (log.split(|byte| *byte == b'\n'))
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let run = Run::parse(line);
            skipped += usize::from(run.is_none());
            run
        })
        .collect();
    (runs, skipped)
}

/// One command's top-level runs on one board in one hour of one rotated log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hour {
    #[serde(rename = "$type")]
    pub(crate) kind: String,
    /// The `at_ms` of the first line of the log this hour came from, which
    /// names that log.
    pub(crate) from_ms: u64,
    pub(crate) root: Option<String>,
    pub(crate) command: String,
    pub(crate) start_ms: u64,
    pub(crate) runs: u64,
    pub(crate) killed: u64,
    /// How many runs were judged, of which `judged_us` keeps the last
    /// [`KEPT`] times in the order they ran.
    pub(crate) judged: u64,
    pub(crate) judged_us: Vec<u64>,
}

const DIGEST_TYPE: &str = "board/timing-digest/v1";

/// The hours of `digest` that parse, and how many lines did not.
pub(crate) fn hours(digest: &[u8]) -> (Vec<Hour>, usize) {
    let mut skipped = 0;
    let hours = (digest.split(|byte| *byte == b'\n'))
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let hour = serde_json::from_slice::<Hour>(line)
                .ok()
                .filter(|hour| hour.kind == DIGEST_TYPE);
            skipped += usize::from(hour.is_none());
            hour
        })
        .collect();
    (hours, skipped)
}

/// How much of the past a digest keeps, so that rotating it stays cheap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Window {
    /// For each command on each board, the newest hours whose judged times
    /// reach this many, which is all the check reads.
    pub(crate) judged: usize,
    /// Every hour this recent, before the newest hour, whose killed runs
    /// the check counts.
    pub(crate) recent_ms: u64,
    /// The most hours in all; the oldest go first past it.
    pub(crate) hours: usize,
}

/// The window the binary keeps: a day of hours, and at most 2,000 hours,
/// about 600 KB (docs/architecture/performance/timing.md, The digest).
pub(crate) const WINDOW: Window = Window {
    judged: KEPT,
    recent_ms: 24 * HOUR_MS,
    hours: 2_000,
};

/// The digest after `log` rotates, within [`WINDOW`].
pub(crate) fn summarise(log: &[u8], old: &[u8]) -> Vec<u8> {
    summarise_within(log, old, &WINDOW)
}

/// `hours` cut to `window`: for each command on each board, its newest hours
/// until their judged times reach `window.judged`, and every hour within
/// `window.recent_ms` of the newest; then at most `window.hours`, newest
/// first. Returned oldest first.
fn within(mut hours: Vec<Hour>, window: &Window) -> Vec<Hour> {
    hours.sort_by_key(|hour| std::cmp::Reverse(hour.start_ms));
    let newest = hours.first().map_or(0, |hour| hour.start_ms);
    let mut held: BTreeMap<(Option<String>, String), usize> = BTreeMap::new();
    hours.retain(|hour| {
        let held = held
            .entry((hour.root.clone(), hour.command.clone()))
            .or_default();
        let needed = *held < window.judged;
        *held += hour.judged_us.len();
        needed || hour.start_ms.saturating_add(window.recent_ms) >= newest
    });
    hours.truncate(window.hours);
    hours.reverse();
    hours
}

/// The digest after `log` rotates: the hours of `old` that parse, then the
/// hours of `log`'s top-level runs, unless `old` already holds that log,
/// cut to `window`. A digest written before a crash is so never counted
/// twice, and a line cut short is dropped.
pub(crate) fn summarise_within(log: &[u8], old: &[u8], window: &Window) -> Vec<u8> {
    let (mut kept, _) = hours(old);
    let (runs, _) = runs(log);
    if let Some(from_ms) = runs.first().map(|run| run.at_ms)
        && !kept.iter().any(|hour| hour.from_ms == from_ms)
    {
        let mut grouped: BTreeMap<(Option<String>, String, u64), Hour> = BTreeMap::new();
        for run in runs.into_iter().filter(|run| run.depth == 0) {
            let start_ms = run.at_ms - run.at_ms % HOUR_MS;
            let key = (run.root.clone(), run.command.clone(), start_ms);
            let hour = grouped.entry(key).or_insert_with(|| Hour {
                kind: DIGEST_TYPE.to_owned(),
                from_ms,
                root: run.root.clone(),
                command: run.command.clone(),
                start_ms,
                runs: 0,
                killed: 0,
                judged: 0,
                judged_us: Vec::new(),
            });
            hour.runs += 1;
            hour.killed += u64::from(run.killed);
            if run.judged() {
                hour.judged += 1;
                hour.judged_us.push(run.total_us);
                if hour.judged_us.len() > KEPT {
                    hour.judged_us.remove(0);
                }
            }
        }
        let mut grouped: Vec<Hour> = grouped.into_values().collect();
        grouped.sort_by_key(|hour| hour.start_ms);
        kept.extend(grouped);
    }
    let mut bytes = Vec::new();
    for hour in within(kept, window) {
        if let Ok(line) = serde_json::to_string(&hour) {
            bytes.extend(line.into_bytes());
            bytes.push(b'\n');
        }
    }
    bytes
}
