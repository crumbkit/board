//! `cargo xtask bench --against <board>`: runs each row's `board` command
//! with an older binary (base) and the one beside xtask (new) in alternating
//! order, and calls a row slower only when a sign test over the rounds says
//! so (docs/architecture/performance/budgets.md).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use super::{BenchError, Host, MOUNTED, failed, median};
use crate::process;

/// How far the median round must move before a row is slower or faster.
const THRESHOLD: f64 = 0.05;
/// The one-sided chance a sign test may call a row slower by luck.
const SIGNIFICANCE: f64 = 0.025;

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
    let on = |board: &Path, words: &[&str]| {
        let mut arguments = vec!["-C".into(), board.into()];
        arguments.extend(words.iter().map(OsString::from));
        arguments
    };
    let query = ["query", "SELECT count(*) FROM entries"];
    let mut rows = vec![
        ("`board get`, warm", on(plain, &["get", sample]), false),
        (
            "`board ls` of a record's directory, warm",
            on(plain, &["ls", directory]),
            false,
        ),
        (
            "`board check`, warm, nothing changed",
            on(plain, &["check"]),
            false,
        ),
        ("simple `board query`, warm", on(plain, &query), false),
        ("full index, cold", on(plain, &["check"]), true),
    ];
    rows.extend(mounted.map(|mounted| (MOUNTED, on(mounted, &query), false)));
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

/// The fewest of `rounds` that must move one way for a sign test to call
/// it a change, or none when no count could.
pub(crate) fn needed(rounds: usize) -> Option<usize> {
    let rounds = i32::try_from(rounds).ok()?;
    let chance = |at_least: i32| -> f64 {
        let ways = |k: i32| {
            (0..k).fold(1.0, |ways, i| {
                ways * f64::from(rounds - i) / f64::from(i + 1)
            })
        };
        (at_least..=rounds).map(ways).sum::<f64>() / 2_f64.powi(rounds)
    };
    let needed = (0..=rounds).find(|at_least| chance(*at_least) <= SIGNIFICANCE)?;
    usize::try_from(needed).ok()
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
    let needed = needed(deltas.len()).unwrap_or(usize::MAX);
    let verdict = if delta > THRESHOLD && slower >= needed {
        Verdict::Slower
    } else if delta < -THRESHOLD && faster >= needed {
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
    if path.is_file() {
        Ok(path.to_owned())
    } else {
        Err(BenchError::NoAgainst {
            path: path.to_owned(),
        })
    }
}

/// One arm's binary and the caches and empty directories its runs get.
struct Binary {
    path: PathBuf,
    cache: TempDir,
    empty: TempDir,
}

/// Runs every row `runs` rounds on `binaries` (base, new), after one
/// discarded run of each, and returns each binary's version from its log.
pub(crate) fn compare(
    binaries: [PathBuf; 2],
    rows: &[Row],
    runs: usize,
    host: &Host,
) -> Result<([String; 2], Vec<Compared>), BenchError> {
    let temporary = || tempfile::tempdir().map_err(BenchError::Write);
    let [base, new] = binaries.map(|path| -> Result<Binary, BenchError> {
        Ok(Binary {
            path,
            cache: temporary()?,
            empty: temporary()?,
        })
    });
    let arms = [base?, new?];
    let mut compared = Vec::with_capacity(rows.len());
    for (name, arguments, cold) in rows {
        let mut times = [Vec::with_capacity(runs), Vec::with_capacity(runs)];
        let mut verdict = None;
        for round in 0..=runs {
            for arm in order(round) {
                let binary = &arms[arm as usize];
                let cold = cold.then(temporary).transpose()?;
                let cache = cold.as_ref().map_or(binary.cache.path(), TempDir::path);
                let (took, exit) = time(binary, arguments, cache, host)?;
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
    let version = |binary: &Binary| crate::timing::version(binary.cache.path());
    Ok((arms.each_ref().map(version), compared))
}

/// The wall time and exit code of `binary` with `arguments`, its cache in
/// `cache`, and nothing else from this machine but `PATH` and `HOME`.
fn time(
    binary: &Binary,
    arguments: &[OsString],
    cache: &Path,
    host: &Host,
) -> Result<(Duration, i32), BenchError> {
    let mut command = process::command(&binary.path);
    command
        .args(arguments)
        .env_clear()
        .env("PATH", &host.path)
        .env("XDG_CACHE_HOME", cache)
        .env("XDG_CONFIG_HOME", binary.empty.path())
        .env("XDG_DATA_HOME", binary.empty.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(home) = &host.home {
        command.env("HOME", home);
    }
    let started = Instant::now();
    let status = command.status().map_err(|error| failed(&error))?;
    Ok((started.elapsed(), status.code().unwrap_or(-1)))
}

#[cfg(test)]
mod tests;
