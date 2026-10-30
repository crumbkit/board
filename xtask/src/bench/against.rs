//! `cargo xtask bench --against <board>`: runs each row's `board` command
//! with an older binary (base) and the one beside xtask (new) in alternating
//! order, and calls a row slower only when a sign test over the rounds says
//! so (docs/architecture/performance/budgets.md).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::{BenchError, Host, MOUNTED, failed, median};
use crate::process;

/// How far the median round must move before a row is slower or faster.
const THRESHOLD: f64 = 0.05;
/// The rounds that must move one way for a row to be slower or faster: a
/// sign test over 15 rounds that calls a row slower by luck under 2% of the
/// time. With fewer rounds, no row is.
const AGREE: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arm {
    Base,
    New,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Verdict {
    Slower,
    Faster,
    Same,
    /// The arm whose command exited with this code.
    Failed(Arm, i32),
}

/// One row: the median of each arm, the median round's change, and how
/// many rounds new was slower in.
#[derive(Debug)]
pub(crate) struct Compared {
    pub(crate) name: String,
    pub(crate) base: Duration,
    pub(crate) new: Duration,
    pub(crate) delta: f64,
    pub(crate) slower: usize,
    pub(crate) rounds: usize,
    pub(crate) verdict: Verdict,
}

/// A row: its name, its arguments to `board`, and whether each run starts
/// from an empty cache.
pub(crate) type Row = (&'static str, Vec<OsString>, bool);

/// The rows on `plain`, reading `sample` and listing `directory`, and the
/// mounted query on `mounted`.
pub(crate) fn rows(
    plain: &Path,
    sample: &str,
    directory: &str,
    mounted: Option<&Path>,
) -> Vec<Row> {
    let row = |name, board: &Path, words: &[&str], cold| -> Row {
        let words = words.iter().map(OsString::from);
        (
            name,
            ["-C".into(), board.into()]
                .into_iter()
                .chain(words)
                .collect(),
            cold,
        )
    };
    let query = ["query", "SELECT count(*) FROM entries"];
    let mut rows = vec![
        row("`board get`, warm", plain, &["get", sample], false),
        row(
            "`board ls` of a record's directory, warm",
            plain,
            &["ls", directory],
            false,
        ),
        row(
            "`board check`, warm, nothing changed",
            plain,
            &["check"],
            false,
        ),
        row("simple `board query`, warm", plain, &query, false),
        row("full index, cold", plain, &["check"], true),
    ];
    rows.extend(mounted.map(|mounted| row(MOUNTED, mounted, &query, false)));
    rows
}

/// The arms in round `round`: base goes first in even rounds.
pub(crate) fn order(round: usize) -> [Arm; 2] {
    if round.is_multiple_of(2) {
        [Arm::Base, Arm::New]
    } else {
        [Arm::New, Arm::Base]
    }
}

/// Judges paired rounds: each round's change is `(new - base) / base`.
pub(crate) fn judge(base: &[Duration], new: &[Duration]) -> Compared {
    let deltas: Vec<f64> = base
        .iter()
        .zip(new)
        .map(|(base, new)| (new.as_secs_f64() - base.as_secs_f64()) / base.as_secs_f64())
        .collect();
    let mut sorted = deltas.clone();
    sorted.sort_by(f64::total_cmp);
    let delta = sorted.get(sorted.len() / 2).copied().unwrap_or_default();
    let slower = deltas.iter().filter(|delta| **delta > 0.0).count();
    let faster = deltas.iter().filter(|delta| **delta < 0.0).count();
    let verdict = if delta > THRESHOLD && slower >= AGREE {
        Verdict::Slower
    } else if delta < -THRESHOLD && faster >= AGREE {
        Verdict::Faster
    } else {
        Verdict::Same
    };
    Compared {
        name: String::new(),
        base: median(base.to_vec()),
        new: median(new.to_vec()),
        delta,
        slower,
        rounds: deltas.len(),
        verdict,
    }
}

/// Whether no row is slower and new failed nowhere.
pub(crate) fn passed(compared: &[Compared]) -> bool {
    compared
        .iter()
        .all(|row| !matches!(row.verdict, Verdict::Slower | Verdict::Failed(Arm::New, _)))
}

/// `path`, when it names a file.
pub(crate) fn base(path: &Path) -> Result<PathBuf, BenchError> {
    let path = path.to_owned();
    path.is_file()
        .then(|| path.clone())
        .ok_or(BenchError::NoAgainst { path })
}

/// Runs every row `runs` rounds on `binaries` (base, new), each with its own
/// cache, after one discarded run of each.
pub(crate) fn compare(
    binaries: &[PathBuf; 2],
    rows: &[Row],
    runs: usize,
    host: &Host,
) -> Result<Vec<Compared>, BenchError> {
    let temporary = || tempfile::tempdir().map_err(BenchError::Write);
    let (empty, caches) = (temporary()?, [temporary()?, temporary()?]);
    let mut compared = Vec::with_capacity(rows.len());
    for (name, arguments, cold) in rows {
        let mut times = [Vec::with_capacity(runs), Vec::with_capacity(runs)];
        let mut verdict = None;
        for round in 0..=runs {
            for arm in order(round) {
                let cold = cold.then(temporary).transpose()?;
                let cache = cold.as_ref().unwrap_or(&caches[arm as usize]).path();
                let directories = [cache, empty.path()];
                let (took, exit) = time(&binaries[arm as usize], arguments, directories, host)?;
                if exit != 0 {
                    verdict = verdict.or(Some(Verdict::Failed(arm, exit)));
                }
                if round > 0 {
                    times[arm as usize].push(took);
                }
            }
        }
        let judged = judge(&times[0], &times[1]);
        compared.push(Compared {
            name: (*name).to_owned(),
            verdict: verdict.unwrap_or(judged.verdict),
            ..judged
        });
    }
    Ok(compared)
}

/// The wall time and exit code of `binary` with `arguments`, a cache and an
/// empty config and data directory, and nothing else from this machine but
/// `PATH` and `HOME`.
fn time(
    binary: &Path,
    arguments: &[OsString],
    [cache, empty]: [&Path; 2],
    host: &Host,
) -> Result<(Duration, i32), BenchError> {
    let mut command = process::command(binary);
    command.args(arguments).env_clear().env("PATH", &host.path);
    command.envs(host.home.iter().map(|home| ("HOME", home)));
    command.env("XDG_CACHE_HOME", cache);
    command.envs(["XDG_CONFIG_HOME", "XDG_DATA_HOME"].map(|name| (name, empty)));
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let started = Instant::now();
    let status = command.status().map_err(|error| failed(&error))?;
    Ok((started.elapsed(), status.code().unwrap_or(-1)))
}

#[cfg(test)]
mod tests;
