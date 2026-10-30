//! One timing record for each run (docs/architecture/performance/timing.md).

use std::path::PathBuf;
use std::time::{Duration, Instant, UNIX_EPOCH};

use board_ops::Trace;
use serde_json::{Value, json};

use crate::config::Context;
use crate::report::Output;

#[derive(Debug, Default)]
pub(crate) struct Ran {
    /// None when the command line was not parsed; such a run is not recorded.
    pub(crate) command: Option<String>,
    pub(crate) trace: Trace,
    pub(crate) root: Option<PathBuf>,
    /// The logical path the run started in, when a known board's mount
    /// holds that directory.
    pub(crate) prefix: Option<String>,
}

/// The `board/timing/v1` record of a run that started at `started`, nested
/// `depth` deep, or none when its command line was not parsed.
pub(crate) fn record(started: Instant, ran: &Ran, output: &Output, depth: u32) -> Option<Value> {
    let command = ran.command.as_ref()?;
    let took = started.elapsed();
    let at = UNIX_EPOCH
        .elapsed()
        .unwrap_or_default()
        .saturating_sub(took);
    let mut phases: Vec<_> = ran.trace.phases.iter().collect();
    phases.sort_by_key(|phase| (phase.started, std::cmp::Reverse(phase.took)));
    let phases: Vec<Value> = (phases.into_iter())
        .map(|phase| {
            json!({
                "name": phase.name,
                "at_us": micros(phase.started.saturating_duration_since(started)),
                "us": micros(phase.took),
                "entered": phase.entered,
            })
        })
        .collect();
    Some(json!({
        "$type": "board/timing/v1",
        "at_ms": micros(at) / 1000,
        "version": concat!(env!("CARGO_PKG_VERSION"), env!("BOARD_BUILD")),
        "command": command,
        "capability": ran.trace.capability,
        "depth": depth,
        "root": ran.root.as_ref().map(|root| root.display().to_string()),
        "prefix": ran.prefix,
        "exit": output.code,
        "error": output.error,
        "total_us": micros(took),
        "phases": phases,
        "work": ran.trace.work,
    }))
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// Appends the record of this run to the timing log under the cache
/// directory, unless `BOARD_TIMINGS` is `0`. A record that cannot be written
/// is dropped: the run it describes has already finished.
pub(crate) fn append(started: Instant, ran: &Ran, output: &Output, context: &Context) {
    let environment = &context.environment;
    let recording = environment.timings.as_deref() != Some("0".as_ref());
    let depth = environment.depth().unwrap_or_default();
    if let Some(cache) = context.cache.as_deref().filter(|_| recording)
        && let Some(record) = record(started, ran, output, depth)
    {
        let _ = board_ops::append_timing(cache, format!("{record}\n").as_bytes());
    }
}

#[cfg(test)]
mod tests;
