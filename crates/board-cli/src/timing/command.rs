//! `board timing check`: prints each hot command's p90 against its budget,
//! and with `--record` writes a run of the speed suite to the personal board
//! (docs/architecture/performance/timing.md, The speed check).

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use board_model::{LogicalPath, Refusal};
use board_ops::{Expect, MountsAt, PutError};
use serde_json::{Value, json};

use super::check::{self, Judged, LOCAL_SUITE, MODE, Mode, State};
use crate::config::Context;
use crate::render::{self, Printer};
use crate::report::Output;

/// How long after a recorded run the next check records none.
const RECORD_EVERY: Duration = Duration::from_secs(3600);

#[derive(Debug, clap::Subcommand)]
pub(crate) enum TimingCommand {
    /// Judge each hot command's p90 on the personal board against its budget.
    ///
    /// A command is judged on its last 20 top-level runs on the personal
    /// board at a load no higher than the core count, and is unjudged with
    /// fewer than 5. The runs come from the timing log and its digest.
    #[command(after_help = "\
Examples:
  board timing check
  board timing check --record")]
    Check {
        /// Also write a run of /suites/board-speed-local to the personal board, at most once an hour.
        #[arg(long)]
        record: bool,
    },
}

/// Why a check recorded no run.
enum Recorded {
    Wrote(LogicalPath),
    Recent,
    NoCache,
}

/// `board timing`.
pub(crate) fn timing(command: &TimingCommand, context: &Context, printer: Printer) -> Output {
    let TimingCommand::Check { record } = command;
    let personal = match context.personal() {
        Ok(personal) => personal,
        Err(refusal) => return render::refused(printer, *refusal),
    };
    let root = std::fs::canonicalize(&personal).unwrap_or_else(|_| personal.clone());
    let cache = context.cache.as_deref();
    let timings = cache.map_or_default(board_ops::read_timings);
    let now = jiff::Timestamp::now();
    let now_ms = u64::try_from(now.as_millisecond()).unwrap_or_default();
    let read = check::read(&timings, &root.display().to_string(), now_ms);
    let judged = check::judge_all(&read, check::budgets());
    let recorded = match (record, cache) {
        (false, _) => None,
        (true, None) => Some(Recorded::NoCache),
        (true, Some(cache)) => match recorded(context, &personal, cache, &judged, now) {
            Ok(recorded) => Some(recorded),
            Err(refusal) => return render::refused(printer, *refusal),
        },
    };
    printer.print(
        render::TIMING_CHECK,
        Ok(said(&judged, recorded.as_ref(), read.skipped)),
    )
}

/// Writes the suite if it is absent and a run of it, unless a run was
/// recorded within the hour, then stamps the cache; a live check then opens
/// tasks and files questions.
fn recorded(
    context: &Context,
    personal: &Path,
    cache: &Path,
    judged: &[Judged],
    now: jiff::Timestamp,
) -> Result<Recorded, Box<Refusal>> {
    let recent = board_ops::timing_stamped(cache)
        .and_then(|stamped| stamped.elapsed().ok())
        .is_some_and(|age| age < RECORD_EVERY);
    if recent {
        return Ok(Recorded::Recent);
    }
    let (mut board, _) = context.open(personal, MountsAt::Pins)?;
    board.ask_as(crate::effects::asker(context, false)?);
    let refused = |error: PutError| Box::new(error.refusal());
    let suite = logical(LOCAL_SUITE)?;
    match board.put(
        &suite,
        Expect::Absent,
        check::local_suite().to_string().as_bytes(),
    ) {
        Ok(_) | Err(PutError::Stale { .. }) => {}
        Err(error) => return Err(refused(error)),
    }
    let at = now.round(jiff::Unit::Second).unwrap_or(now);
    let name = at.strftime("%Y%m%dT%H%M%SZ").to_string();
    let path = logical(&format!("{LOCAL_SUITE}/runs/{name}"))?;
    let host = check::Host {
        name: nix::unistd::gethostname()
            .ok()
            .and_then(|name| name.into_string().ok()),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        cpus: super::machine().cpus.unwrap_or(1),
    };
    let run = check::run_record(judged, &at.to_string(), super::BUILD, &host);
    board
        .put(&path, Expect::Absent, run.to_string().as_bytes())
        .map_err(refused)?;
    // Without the stamp the next check within the hour records one more
    // run, which does no harm.
    let _ = board_ops::stamp_timing(cache);
    if MODE == Mode::Live {
        let asked = (&*at.to_string(), &json!({"program": "board"}));
        check::alert(&board, judged, &path, asked, &check::Unmeasured).map_err(refused)?;
    }
    Ok(Recorded::Wrote(path))
}

fn logical(path: &str) -> Result<LogicalPath, Box<Refusal>> {
    path.parse()
        .map_err(|error| Box::new(PutError::Path(error).refusal()))
}

fn state(state: State) -> &'static str {
    match state {
        State::Unjudged => "unjudged",
        State::Within => "within",
        State::Over => "over",
        State::Thrice => "over three times",
    }
}

fn ms(us: Option<u64>) -> Option<f64> {
    us.map(check::ms)
}

/// The report: a row per command, then whether a run was recorded.
fn said(judged: &[Judged], recorded: Option<&Recorded>, skipped: usize) -> render::Answer {
    let mut text = format!(
        "{:<18} {:>9} {:>11} {:>11} {:>6} {:>6}  state\n",
        "command", "budget", "p50", "p90", "runs", "killed"
    );
    let shown = |us: Option<u64>| ms(us).map_or("-".to_owned(), |ms| format!("{ms} ms"));
    let mut commands = Vec::new();
    for judged in judged {
        let budget = judged.budget;
        let _ = writeln!(
            text,
            "{:<18} {:>9} {:>11} {:>11} {:>6} {:>6}  {}",
            budget.command,
            format!("{} ms", budget.ms),
            shown(judged.p50_us),
            shown(judged.p90_us),
            judged.runs,
            judged.killed,
            state(judged.state)
        );
        commands.push(json!({
            "command": budget.command,
            "budget_ms": budget.ms,
            "p50_ms": ms(judged.p50_us),
            "p90_ms": ms(judged.p90_us),
            "judged": judged.runs,
            "killed": judged.killed,
            "state": state(judged.state),
        }));
    }
    if skipped > 0 {
        let _ = writeln!(
            text,
            "{skipped} lines of the log or digest do not parse and were left out"
        );
    }
    let recorded_path = match recorded {
        Some(Recorded::Wrote(path)) => {
            let _ = writeln!(text, "recorded {path}");
            Value::from(path.to_string())
        }
        Some(Recorded::Recent) => {
            let _ = writeln!(
                text,
                "not recorded: a run was recorded less than an hour ago"
            );
            Value::Null
        }
        Some(Recorded::NoCache) => {
            let _ = writeln!(
                text,
                "not recorded: Board has no cache directory, so no timing log"
            );
            Value::Null
        }
        None => Value::Null,
    };
    let mode = match MODE {
        Mode::Silent => "silent",
        Mode::Live => "live",
    };
    render::said(
        json!({"commands": commands, "recorded": recorded_path, "skipped": skipped, "mode": mode}),
        text,
    )
}

/// A report with every field, for the released interfaces' test.
#[cfg(test)]
pub(crate) fn sample() -> render::Answer {
    let judged = check::judge_all(&check::Read::default(), check::budgets());
    let path = "/suites/board-speed-local/runs/20261123T120000Z";
    said(&judged, Some(&Recorded::Wrote(path.parse().unwrap())), 1)
}
