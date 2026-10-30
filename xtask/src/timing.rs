//! `cargo xtask timing <command> [--last <n>]`: the median time of the last
//! runs of a command, and of each phase, from this machine's timing log
//! (docs/architecture/performance/timing.md).

use std::path::Path;

use serde_json::Value;

/// The runs `timing` summarises unless `--last` says otherwise.
const LAST: usize = 20;

/// Which runs to summarise: top-level runs of `command`, of `capability`
/// when the last word names one, and only the last `last` of them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Filter {
    pub(crate) command: String,
    pub(crate) capability: Option<String>,
    pub(crate) last: usize,
}

impl Filter {
    /// Reads `<command words> [--last <n>]`.
    pub(crate) fn parse(mut arguments: impl Iterator<Item = String>) -> Option<Self> {
        let mut words = Vec::new();
        let mut last = LAST;
        while let Some(word) = arguments.next() {
            match word.as_str() {
                "--last" => last = arguments.next()?.parse().ok().filter(|last| *last > 0)?,
                _ => words.push(word),
            }
        }
        let capability = words.pop_if(|word| word.contains('.'));
        let command = words.join(" ");
        let command = if command.is_empty() && capability.is_some() {
            "call".to_owned()
        } else {
            command
        };
        (!command.is_empty()).then_some(Self {
            command,
            capability,
            last,
        })
    }
}

/// The medians of the runs a filter chose, in microseconds.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) runs: usize,
    /// The newest version in the log, the only one summarised.
    pub(crate) version: String,
    pub(crate) malformed: usize,
    pub(crate) total: u64,
    /// Each phase in the order the runs entered it, its median and how
    /// many of the runs entered it.
    pub(crate) phases: Vec<(String, u64, usize)>,
}

/// Summarises the runs `filter` chooses from the log lines `log`, oldest
/// first.
pub(crate) fn summarize(log: &str, filter: &Filter) -> Summary {
    let mut summary = Summary::default();
    let mut chosen: Vec<Value> = Vec::new();
    for line in log.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            summary.malformed += 1;
            continue;
        };
        if record["depth"] == 0
            && record["command"] == filter.command.as_str()
            && filter
                .capability
                .as_ref()
                .is_none_or(|wanted| record["capability"] == wanted.as_str())
        {
            chosen.push(record);
        }
    }
    let newest = chosen.last().map(|record| record["version"].clone());
    chosen.retain(|record| Some(&record["version"]) == newest.as_ref());
    let chosen = &chosen[chosen.len().saturating_sub(filter.last)..];
    summary.runs = chosen.len();
    summary.version = newest
        .and_then(|version| version.as_str().map(str::to_owned))
        .unwrap_or_default();
    summary.total = middle(
        chosen
            .iter()
            .filter_map(|record| record["total_us"].as_u64())
            .collect(),
    );
    let mut phases: Vec<(String, Vec<u64>)> = Vec::new();
    for phase in chosen
        .iter()
        .filter_map(|record| record["phases"].as_array())
        .flatten()
    {
        let (Some(name), Some(us)) = (phase["name"].as_str(), phase["us"].as_u64()) else {
            continue;
        };
        match phases.iter_mut().find(|(known, _)| known == name) {
            Some((_, times)) => times.push(us),
            None => phases.push((name.to_owned(), vec![us])),
        }
    }
    summary.phases = (phases.into_iter())
        .map(|(name, times)| {
            let entered = times.len();
            (name, middle(times), entered)
        })
        .collect();
    summary
}

fn middle(mut times: Vec<u64>) -> u64 {
    times.sort_unstable();
    times.get(times.len() / 2).copied().unwrap_or_default()
}

/// The timing log under `cache`: the older file, then the current one.
pub(crate) fn read(cache: &Path) -> String {
    ["board/timings.jsonl.1", "board/timings.jsonl"]
        .map(|log| std::fs::read_to_string(cache.join(log)).unwrap_or_default())
        .concat()
}

/// The version of the last run the timing log under `cache` records.
pub(crate) fn version(cache: &Path) -> String {
    let log = read(cache);
    let last = log
        .lines()
        .last()
        .and_then(|line| serde_json::from_str::<Value>(line).ok());
    last.and_then(|record| record["version"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests;
