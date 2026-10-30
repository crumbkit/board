//! `cargo xtask bench --against <board>`: runs each row's `board` command
//! with an older binary (base) and one it builds in release (new) in alternating
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
/// time. That bound holds only at 15 rounds; with fewer, no row is slower.
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
    let query: &[&str] = &["query", "SELECT count(*) FROM entries"];
    let ls = "`board ls` of a record's directory, warm";
    let table: [(&'static str, &[&str], bool); 5] = [
        ("`board get`, warm", &["get", sample], false),
        (ls, &["ls", directory], false),
        ("`board check`, warm, nothing changed", &["check"], false),
        ("simple `board query`, warm", query, false),
        ("full index, cold", &["check"], true),
    ];
    let row = |(name, words, cold): (&'static str, &[&str], bool), board: &Path| -> Row {
        let words = words.iter().map(OsString::from);
        let arguments = ["-C".into(), board.into()].into_iter().chain(words);
        (name, arguments.collect(), cold)
    };
    let mut rows: Vec<Row> = table.into_iter().map(|entry| row(entry, plain)).collect();
    rows.extend(mounted.map(|mounted| row((MOUNTED, query, false), mounted)));
    rows
}

/// Each round's arms in order, and whether it is measured: one discarded
/// round, then `runs` that alternate which arm goes first, starting with
/// base, so the measured rounds split as evenly as their count allows.
pub(crate) fn rounds(runs: usize) -> impl Iterator<Item = (bool, [Arm; 2])> {
    (0..=runs).map(|round| match round % 2 {
        1 => (true, [Arm::Base, Arm::New]),
        _ => (round > 0, [Arm::New, Arm::Base]),
    })
}

/// Judges the paired rounds of row `name`: each round's change is
/// `(new - base) / base`.
pub(crate) fn judge(name: &str, base: &[Duration], new: &[Duration]) -> Compared {
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
        name: name.to_owned(),
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

/// Builds `board` in release from the workspace at `root`, to compare as new.
pub(crate) fn build(root: &Path) -> Result<PathBuf, BenchError> {
    let mut cargo = process::cargo();
    cargo.args(["build", "--release", "--quiet", "--package", "board-cli"]);
    let status = cargo.current_dir(root).status().map_err(|e| failed(&e))?;
    if !status.success() {
        return Err(failed(&format!("building board-cli stopped with {status}")));
    }
    let directory = process::target(root).join("release");
    let board = directory.join("board");
    board
        .is_file()
        .then_some(board)
        .ok_or(BenchError::NoBoard { directory })
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
        for (measured, arms) in rounds(runs) {
            for arm in arms {
                let cold = cold.then(temporary).transpose()?;
                let cache = cold.as_ref().unwrap_or(&caches[arm as usize]).path();
                let directories = [cache, empty.path()];
                let (took, exit) = time(&binaries[arm as usize], arguments, directories, host)?;
                if exit != 0 {
                    verdict = verdict.or(Some(Verdict::Failed(arm, exit)));
                }
                if measured {
                    times[arm as usize].push(took);
                }
            }
        }
        let judged = judge(name, &times[0], &times[1]);
        compared.push(Compared {
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
