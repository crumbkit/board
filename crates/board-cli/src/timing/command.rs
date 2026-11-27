//! `board timing check`: prints each hot command's p50 and p90 against its
//! budget and its mode, and with `--record` imports the newest CI timing
//! run and writes a run of the speed suite to the personal board
//! (docs/architecture/performance/timing.md, The speed check).

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use board_model::{LogicalPath, Refusal};
use board_ops::{Board, Expect, MountsAt, PutError};
use serde_json::{Value, json};

use super::check::{self, Asker, Judged, LOCAL_SUITE, Mode, State};
use super::{grace, import, ready};
use crate::config::Context;
use crate::render::{self, Printer};
use crate::report::Output;

/// How long after a recorded run the next check records none.
const RECORD_EVERY: Duration = Duration::from_secs(3600);
/// The variables gh reads for its account and configuration.
const GH_VARIABLES: [&str; 4] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_CONFIG_DIR",
    "XDG_CONFIG_HOME",
];

/// A Claude Code session's `local_` id, as plans/question/v1 names it.
fn session(value: &str) -> Result<String, String> {
    let id = value.strip_prefix("local_").unwrap_or_default();
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    valid
        .then(|| value.to_owned())
        .ok_or_else(|| format!("{value:?} is not a session's local_ id, such as local_0a1b-2c3d"))
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum TimingCommand {
    /// Judge each hot command's p90 and median on the personal board against its budget.
    ///
    /// A command is judged on its last 20 top-level runs on the personal
    /// board at a load no higher than the core count, and is unjudged with
    /// fewer than 5. The runs come from the timing log and its digest. A
    /// command is silent until it has enough steady results, then live: a
    /// live command over budget opens a task, and after the grace a
    /// question for the owner.
    #[command(after_help = "\
Examples:
  board timing check
  board timing check --record
  board timing check --record --session local_0a1b-2c3d")]
    Check {
        /// Also import the newest CI timing run and write a run of /suites/board-speed-local to the personal board, at most once an hour.
        #[arg(long)]
        record: bool,
        /// The Claude Code session that asks, by its local_ id, so a question the check files names it; without it the check files none.
        #[arg(long, value_parser = session, requires = "record")]
        session: Option<String>,
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
    let TimingCommand::Check { record, session } = command;
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
    let mut warnings = Vec::new();
    let (recorded, live) = match (record, cache) {
        (false, _) => (None, shown_live(context, &personal)),
        (true, None) => (Some(Recorded::NoCache), shown_live(context, &personal)),
        (true, Some(cache)) => {
            let asker = Asker {
                at: &now.round(jiff::Unit::Second).unwrap_or(now).to_string(),
                session: session.as_deref(),
            };
            match recorded(context, &personal, cache, &judged, &asker, &mut warnings) {
                Ok(recorded) => recorded,
                Err(refusal) => return render::refused(printer, *refusal),
            }
        }
    };
    printer.print(
        render::TIMING_CHECK,
        Ok(said(
            &judged,
            &live,
            recorded.as_ref(),
            read.skipped,
            &warnings,
        )),
    )
}

/// The commands the newest local run names live, for a check that records
/// none; none when the personal board cannot be read.
fn shown_live(context: &Context, personal: &Path) -> BTreeSet<String> {
    let Ok((board, _)) = context.open(personal, MountsAt::Pins) else {
        return BTreeSet::new();
    };
    let newest = ready::runs(&board, LOCAL_SUITE, context.cache.as_deref(), 1);
    newest.last().map(ready::live_in).unwrap_or_default()
}

/// The variables `names` from the environment Board inherited, beside
/// `PATH` and `HOME`.
fn environment(context: &Context, names: &[&str]) -> Vec<(String, OsString)> {
    let inherited = &context.environment.inherited;
    (["PATH", "HOME"].iter().chain(names))
        .filter_map(|name| {
            let value = inherited.get(name)?;
            Some(((*name).to_owned(), value.clone()))
        })
        .collect()
}

/// Imports the newest CI run, decides which commands are live, and writes
/// the suite if it is absent and a run of it, unless a run was recorded
/// within the hour, then stamps the cache. Once a command is live, it writes
/// the speed rules if absent, then opens tasks and files questions. A
/// failure of gh or git is a warning: the local run is still written.
fn recorded(
    context: &Context,
    personal: &Path,
    cache: &Path,
    judged: &[Judged],
    asker: &Asker<'_>,
    warnings: &mut Vec<String>,
) -> Result<(Option<Recorded>, BTreeSet<String>), Box<Refusal>> {
    let recent = board_ops::timing_stamped(cache)
        .and_then(|stamped| stamped.elapsed().ok())
        .is_some_and(|age| age < RECORD_EVERY);
    if recent {
        return Ok((Some(Recorded::Recent), shown_live(context, personal)));
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
    // Once every command is live, the newest run alone says so.
    let mut local = ready::runs(&board, LOCAL_SUITE, Some(cache), 1);
    let before = local.last().map(ready::live_in).unwrap_or_default();
    let every = (check::budgets().iter()).all(|budget| before.contains(budget.command));
    if !every {
        local = ready::runs(&board, LOCAL_SUITE, Some(cache), ready::READ);
    }
    warnings.extend(imported(&board, context, cache, &before));
    let ci = if every {
        Vec::new()
    } else {
        ready::runs(&board, import::CI_SUITE, Some(cache), ready::READ)
    };
    let live = ready::live(&ready::Past::of(&local, &ci), judged, &ready::READY);
    let at = asker.at;
    let name = at
        .parse::<jiff::Timestamp>()
        .map(|at| at.strftime("%Y%m%dT%H%M%SZ").to_string())
        .unwrap_or_default();
    let path = logical(&format!("{LOCAL_SUITE}/runs/{name}"))?;
    let host = check::Host {
        name: nix::unistd::gethostname()
            .ok()
            .and_then(|name| name.into_string().ok()),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        cpus: super::machine().cpus.unwrap_or(1),
    };
    let run = check::run_record(judged, &live, at, super::BUILD, &host);
    board
        .put(&path, Expect::Absent, run.to_string().as_bytes())
        .map_err(refused)?;
    // Without the stamp the next check within the hour records one more
    // run, which does no harm.
    let _ = board_ops::stamp_timing(cache);
    if !live.is_empty() {
        warnings.extend(alerted(
            &board,
            context,
            personal,
            judged,
            &live,
            (&path, asker),
        )?);
    }
    Ok((Some(Recorded::Wrote(path)), live))
}

/// Imports the newest CI run through gh, or says why it could not.
fn imported(
    board: &Board,
    context: &Context,
    cache: &Path,
    live: &BTreeSet<String>,
) -> Option<String> {
    // gh downloads into a new directory, removed when the import ends. A
    // check killed during the download leaves it in the cache.
    let scratch = match board_ops::scratch_directory(&cache.join("board"), "timing-ci-") {
        Ok(scratch) => scratch,
        Err(error) => {
            return Some(format!(
                "no CI run imported: its download directory cannot be made: {error}"
            ));
        }
    };
    let gh = import::Gh {
        program: "gh".to_owned(),
        environment: environment(context, &GH_VARIABLES),
        directory: scratch.path().to_path_buf(),
    };
    let imported = import::import(board, &gh, live);
    imported
        .err()
        .map(|warning| format!("no CI run imported: {warning}"))
}

/// Writes the speed rules if absent, then opens tasks and files questions
/// for the live commands over budget, judged by the run at `run` and asked
/// as `asker`. Returns a warning when no question can be filed.
fn alerted(
    board: &Board,
    context: &Context,
    personal: &Path,
    judged: &[Judged],
    live: &BTreeSet<String>,
    (run, asker): (&LogicalPath, &Asker<'_>),
) -> Result<Option<String>, Box<Refusal>> {
    let refused = |error: PutError| Box::new(error.refusal());
    ready::rules(board).map_err(refused)?;
    let allowed = grace::allowed(board, &crate::run::plugins(context));
    let lines = grace::for_board(personal, allowed, environment(context, &[]));
    let warning = lines.is_none().then(|| {
        "no question can be filed: the personal board mounts no /repos/board, so the lines changed since a task opened are unknown".to_owned()
    });
    let grace: &dyn check::Grace = match &lines {
        Some(lines) => lines,
        None => &grace::Unmeasured,
    };
    check::alert(board, judged, live, run, asker, grace).map_err(refused)?;
    Ok(warning)
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

/// The report: a row per command, then whether a run was recorded and any
/// warnings.
fn said(
    judged: &[Judged],
    live: &BTreeSet<String>,
    recorded: Option<&Recorded>,
    skipped: usize,
    warnings: &[String],
) -> render::Answer {
    let mut text = format!(
        "{:<16} {:>9} {:>9} {:>10} {:>10} {:>5} {:>6}  {:<6}  state\n",
        "command", "budget", "median", "p50", "p90", "runs", "killed", "mode"
    );
    let shown = |us: Option<u64>| ms(us).map_or("-".to_owned(), |ms| format!("{ms} ms"));
    let mut commands = Vec::new();
    for judged in judged {
        let budget = judged.budget;
        let mode = Mode::of(budget.command, live).name();
        let median = budget
            .median_ms
            .map_or("-".to_owned(), |ms| format!("{ms} ms"));
        let _ = writeln!(
            text,
            "{:<16} {:>9} {:>9} {:>10} {:>10} {:>5} {:>6}  {:<6}  {}",
            budget.command,
            format!("{} ms", budget.ms),
            median,
            shown(judged.p50_us),
            shown(judged.p90_us),
            judged.runs,
            judged.killed,
            mode,
            state(judged.state)
        );
        commands.push(json!({
            "command": budget.command,
            "budget_ms": budget.ms,
            "median_budget_ms": budget.median_ms,
            "p50_ms": ms(judged.p50_us),
            "p90_ms": ms(judged.p90_us),
            "judged": judged.runs,
            "killed": judged.killed,
            "mode": mode,
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
    for warning in warnings {
        let _ = writeln!(text, "warning: {warning}");
    }
    render::said(
        json!({"commands": commands, "recorded": recorded_path, "skipped": skipped,
            "live": live, "warnings": warnings}),
        text,
    )
}

/// A report with every field, for the released interfaces' test.
#[cfg(test)]
pub(crate) fn sample() -> render::Answer {
    let judged = check::judge_all(&check::Read::default(), check::budgets());
    let path = "/suites/board-speed-local/runs/20261123T120000Z";
    let live = BTreeSet::from(["get".to_owned()]);
    let warnings = ["no CI run imported: `gh run list` did not run".to_owned()];
    said(
        &judged,
        &live,
        Some(&Recorded::Wrote(path.parse().unwrap())),
        1,
        &warnings,
    )
}
