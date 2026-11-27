//! One timing record for each run (docs/architecture/performance/timing.md).

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, UNIX_EPOCH};

use board_ops::Trace;
use serde_json::{Value, json};

use crate::config::Context;
use crate::report::Output;

/// Board's version and the build it came from, which the timing log records
/// and `board --version` prints.
pub(crate) const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), env!("BOARD_BUILD"));

/// What `board --version` prints: [`BUILD`] and, when the build embedded the
/// commit date of its source, ` (built <UTC timestamp>)`.
pub(crate) const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    env!("BOARD_BUILD"),
    env!("BOARD_BUILD_SHOWN")
);

/// The commit date of this build's source, in seconds since the Unix epoch,
/// when the build embedded one.
pub(crate) fn built() -> Option<i64> {
    env!("BOARD_BUILD_DATE").parse().ok()
}

/// What keys the decisions an index keeps: [`BUILD`], or nothing, which
/// keeps none, for a build that does not know its commit.
pub(crate) const DECIDER: &str = if env!("BOARD_BUILD").is_empty() {
    ""
} else {
    BUILD
};

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

/// The machine a run ended on: its one-minute load and its logical cores,
/// each when known.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct Machine {
    pub(crate) load1: Option<f64>,
    pub(crate) cpus: Option<u64>,
}

/// This machine, now.
pub(crate) fn machine() -> Machine {
    Machine {
        load1: load1(),
        cpus: std::thread::available_parallelism()
            .ok()
            .map(|cpus| cpus.get() as u64),
    }
}

/// The one-minute load average, from `vm.loadavg`: three fixed-point loads
/// and their scale.
#[cfg(target_os = "macos")]
fn load1() -> Option<f64> {
    use sysctl::Sysctl as _;
    let value = sysctl::Ctl::new("vm.loadavg").ok()?.value().ok()?;
    let sysctl::CtlValue::Struct(bytes) = value else {
        return None;
    };
    let load = u32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    let scale = i64::from_ne_bytes(bytes.get(16..24)?.try_into().ok()?);
    let scale = f64::from(u32::try_from(scale).ok().filter(|scale| *scale > 0)?);
    Some(f64::from(load) / scale)
}

/// The one-minute load average, the first field of /proc/loadavg.
#[cfg(not(target_os = "macos"))]
fn load1() -> Option<f64> {
    let text = std::fs::read_to_string("/proc/loadavg").ok()?;
    text.split_whitespace().next()?.parse().ok()
}

/// The `board/timing/v1` record of a run that started at `started`, nested
/// `depth` deep, on `machine`, or none when its command line was not parsed.
/// A run a signal ended is `killed`, and its exit is null.
pub(crate) fn record(
    started: Instant,
    ran: &Ran,
    output: &Output,
    depth: u32,
    machine: Machine,
    killed: bool,
) -> Option<Value> {
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
        "version": BUILD,
        "command": command,
        "capability": ran.trace.capability,
        "depth": depth,
        "root": ran.root.as_ref().map(|root| root.display().to_string()),
        "prefix": ran.prefix,
        "exit": (!killed).then_some(output.code),
        "error": output.error,
        "total_us": micros(took),
        "phases": phases,
        "work": ran.trace.work,
        "load1": machine.load1.map(|load| (load * 100.0).round() / 100.0),
        "cpus": machine.cpus,
        "killed": killed,
    }))
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// What a signal handler needs to write the line of a run that a signal
/// ends: when it started, how deep it is, the cache its line goes to, and
/// what it runs once that is known. A run that is `ready_line` prints
/// [`READY_LINE`] once its handler is installed.
struct Pending {
    started: Instant,
    depth: u32,
    cache: PathBuf,
    command: Option<String>,
    capability: Option<String>,
    ready_line: bool,
}

/// The variable that, set to `1`, has a run print [`READY_LINE`] to stderr
/// once its signal handler is installed, so a test can signal the run
/// without guessing when it is ready.
const READY_VARIABLE: &str = "BOARD_TIMING_READY";

/// What a run prints when [`READY_VARIABLE`] is `1` and its handler is
/// installed.
const READY_LINE: &[u8] = b"board: timing handler ready\n";

/// The run in progress, which only the binary's main sets.
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

/// Whether main set [`PENDING`], so that a run whose pending line a signal
/// already took writes no second line.
static WATCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn pending() -> std::sync::MutexGuard<'static, Option<Pending>> {
    PENDING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Notes that the binary's run started at `started`, so that a signal that
/// ends it still writes its line, unless recording is off.
pub(crate) fn watch(started: Instant, context: &Context) {
    let environment = &context.environment;
    if environment.timings.as_deref() == Some("0".as_ref()) {
        return;
    }
    if let Some(cache) = &context.cache {
        WATCHED.store(true, std::sync::atomic::Ordering::SeqCst);
        *pending() = Some(Pending {
            started,
            depth: environment.depth().unwrap_or_default(),
            cache: cache.clone(),
            command: None,
            capability: None,
            ready_line: environment.inherited.get(READY_VARIABLE) == Some(&"1".into()),
        });
    }
}

/// Names the command the binary's run parsed, and the capability it calls
/// when the command line names one, and has SIGINT, SIGTERM or SIGHUP write
/// its line with `killed: true` before the run ends as the signal would
/// have, then prints [`READY_LINE`] when the run asked for it. Nothing
/// happens outside the binary's run.
pub(crate) fn naming(command: &str, capability: Option<String>) {
    let mut pending = pending();
    let Some(running) = pending.as_mut() else {
        return;
    };
    running.command = Some(command.to_owned());
    running.capability = capability;
    let ready_line = running.ready_line;
    drop(pending);
    // A run that cannot watch for signals still runs; it only loses the
    // line a signal would have written, and says nothing is ready.
    if board_plugin::clean_up_on_signals(killed).is_ok() && ready_line {
        crate::report::emit(&Output {
            stderr: READY_LINE.to_vec(),
            ..Output::default()
        });
    }
}

/// Writes the line of the run a signal is ending, once.
fn killed() {
    let Some(running) = pending().take() else {
        return;
    };
    let mut ran = Ran {
        command: running.command,
        ..Ran::default()
    };
    ran.trace.capability = running.capability;
    let output = Output::default();
    if let Some(record) = record(
        running.started,
        &ran,
        &output,
        running.depth,
        machine(),
        true,
    ) {
        let line = format!("{record}\n");
        let _ = board_ops::append_timing(&running.cache, line.as_bytes(), &digest::summarise);
    }
}

/// Appends the record of this run to the timing log under the cache
/// directory, unless `BOARD_TIMINGS` is `0`. A record that cannot be written
/// is dropped: the run it describes has already finished.
pub(crate) fn append(started: Instant, ran: &Ran, output: &Output, context: &Context) {
    // A signal from here on writes no second line, and a signal that came
    // first has written the run's one line.
    let taken = pending().take();
    if taken.is_none() && WATCHED.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let environment = &context.environment;
    let recording = environment.timings.as_deref() != Some("0".as_ref());
    let depth = environment.depth().unwrap_or_default();
    if let Some(cache) = context.cache.as_deref().filter(|_| recording)
        && let Some(record) = record(started, ran, output, depth, machine(), false)
    {
        let line = format!("{record}\n");
        let _ = board_ops::append_timing(cache, line.as_bytes(), &digest::summarise);
    }
}

mod budgets;
mod check;
mod command;
mod digest;
mod grace;
mod import;
mod ready;

#[cfg(test)]
pub(crate) use command::sample;
pub(crate) use command::{TimingCommand, timing};

#[cfg(test)]
mod tests;
