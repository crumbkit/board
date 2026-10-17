//! `cargo xtask bench`: times Board's operations on a generated board, or on
//! one given with `--board`, against the budgets in
//! docs/architecture/performance/budgets.md. It reports every operation and
//! fails when one takes more than three times its budget. Build it with
//! `--release`: a debug build measures the compiler, not Board.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use board_model::Kind;
use board_ops::{Board, Plugins};
use board_plugin::{Environment, Limits, Target};
use board_tree::Status;

/// How many times each operation runs; the median is reported.
const RUNS: usize = 5;
/// How many times its budget an operation may take before the job fails.
const SLACK: u32 = 3;
/// The records a generated board holds unless `--records` says otherwise.
const RECORDS: usize = 10_000;
/// Tasks per directory in a generated board.
const PER_DIRECTORY: usize = 100;

/// The tasks plugin's tasks in a generated board: as many as `tasks.next`
/// shows at once.
const TASKS: usize = 100;
/// The schema the tasks plugin ships, which `board plugin enable` writes.
const TASK_SCHEMA: &str = include_str!("../../plugins/tasks/types/tasks/task/v1.schema.json");

/// The operation that times a call to a Python plugin.
pub(crate) const CALL: &str = "`board call tasks.next`, warm";

/// One operation's budget and what it took.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Timed {
    pub(crate) operation: String,
    pub(crate) budget: Duration,
    pub(crate) took: Took,
}

/// The median of an operation's runs, or why it did not run.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Took {
    Median(Duration),
    Skipped(String),
}

impl Timed {
    /// A skipped operation passes; its report line says why it was skipped.
    pub(crate) fn passed(&self) -> bool {
        match self.took {
            Took::Median(median) => median <= self.budget * SLACK,
            Took::Skipped(_) => true,
        }
    }
}

/// What the plugin call takes from the machine bench runs on.
#[derive(Debug)]
pub(crate) struct Host {
    /// `PATH`, which must hold `uv`: Board runs the tasks plugin through it.
    pub(crate) path: OsString,
    pub(crate) home: Option<OsString>,
    /// The directory holding the `board` that the plugin's own `board query`
    /// runs: the one beside this binary, built with its profile.
    pub(crate) board: PathBuf,
    /// `XDG_DATA_HOME`, with the tasks plugin installed in `board/plugins`.
    pub(crate) data: PathBuf,
}

/// Why bench skips the plugin call, if it does.
pub(crate) fn skip(enabled: bool, host: &Host) -> Option<String> {
    if !enabled {
        return Some("the board does not enable the tasks plugin".to_owned());
    }
    if !std::env::split_paths(&host.path).any(|directory| directory.join("uv").is_file()) {
        return Some("uv is not on PATH".to_owned());
    }
    None
}

/// The `PATH` the plugin call gets, with `host.board` first. A missing
/// `board` is an error, so the timing job cannot pass without timing the call.
pub(crate) fn plugin_path(host: &Host) -> Result<OsString, BenchError> {
    if !host.board.join("board").is_file() {
        return Err(BenchError::NoBoard {
            directory: host.board.clone(),
        });
    }
    let directories = std::iter::once(host.board.clone()).chain(std::env::split_paths(&host.path));
    std::env::join_paths(directories).map_err(|error| failed(&error))
}

/// What `bench` was asked to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) board: Option<PathBuf>,
    pub(crate) records: usize,
}

impl Options {
    /// Reads `[--board <dir>] [--records <n>]`.
    pub(crate) fn parse(mut arguments: impl Iterator<Item = String>) -> Option<Self> {
        let mut options = Self {
            board: None,
            records: RECORDS,
        };
        while let Some(flag) = arguments.next() {
            match flag.as_str() {
                "--board" => options.board = Some(PathBuf::from(arguments.next()?)),
                "--records" => options.records = arguments.next()?.parse().ok()?,
                _ => return None,
            }
        }
        Some(options)
    }
}

/// The budgets table in `docs`: each operation's name, exactly as written,
/// and its budget.
pub(crate) fn budgets(docs: &str) -> Result<Vec<(String, Duration)>, BenchError> {
    let mut lines = docs
        .lines()
        .skip_while(|line| *line != "| Operation | Budget |");
    if lines.next().is_none() {
        return Err(BenchError::NoBudgets);
    }
    lines
        .skip(1)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            let [operation, budget] = cells[..] else {
                return Err(BenchError::Budget {
                    line: line.to_owned(),
                });
            };
            let duration = match budget.split_once(' ') {
                Some((amount, "ms")) => amount.parse().ok().map(Duration::from_millis),
                Some((amount, "s")) => amount.parse().ok().map(Duration::from_secs),
                _ => None,
            };
            duration
                .map(|duration| (operation.to_owned(), duration))
                .ok_or_else(|| BenchError::Budget {
                    line: line.to_owned(),
                })
        })
        .collect()
}

/// Times every budgeted operation on the board at `root`, a repository root.
/// `get` reads the first record a check finds, `ls` lists its directory, and
/// the plugin call runs on `host`.
pub(crate) fn run(
    root: &Path,
    budgets: &[(String, Duration)],
    host: &Host,
) -> Result<Vec<Timed>, BenchError> {
    let board = Board::open(root, None).map_err(|error| failed(&error))?;
    let warm = tempfile::tempdir().map_err(|error| failed(&error))?;
    let checked = board
        .check(None, Some(warm.path()))
        .map_err(|error| failed(&error))?;
    let sample = checked
        .entries
        .iter()
        .find(|entry| entry.kind == Kind::Record && entry.status == Status::Ok)
        .and_then(|entry| entry.logical.clone())
        .ok_or_else(|| failed(&"it holds no valid record to get"))?;
    let sample = &sample;
    let directory = sample
        .as_str()
        .rsplit_once('/')
        .and_then(|(parent, _)| parent.parse().ok())
        .unwrap_or_else(board_model::LogicalPath::root);
    budgets
        .iter()
        .map(|(operation, budget)| {
            if operation == CALL {
                let enabled = board
                    .config()
                    .plugins
                    .iter()
                    .any(|name| name.as_str() == "tasks");
                let took = match skip(enabled, host) {
                    Some(reason) => Took::Skipped(reason),
                    None => Took::Median(call(&board, warm.path(), plugin_path(host)?, host)?),
                };
                return Ok(Timed {
                    operation: operation.clone(),
                    budget: *budget,
                    took,
                });
            }
            let mut times = Vec::with_capacity(RUNS);
            for _ in 0..RUNS {
                let cold = tempfile::tempdir().map_err(|error| failed(&error))?;
                let started = Instant::now();
                match operation.as_str() {
                    "`board get`, warm" => board.get(sample).map(drop).map_err(|e| failed(&e)),
                    "`board ls` of a record's directory, warm" => board
                        .ls(&directory, Some(warm.path()))
                        .map(drop)
                        .map_err(|e| failed(&e)),
                    "`board check`, warm, nothing changed" => board
                        .check(None, Some(warm.path()))
                        .map(drop)
                        .map_err(|e| failed(&e)),
                    "simple `board query`, warm" => board
                        .query(
                            "SELECT count(*) FROM entries",
                            1,
                            None,
                            Some(warm.path()),
                            &board_ops::Plugins::default(),
                        )
                        .map(drop)
                        .map_err(|e| failed(&e)),
                    "full index, cold" => board
                        .check(None, Some(cold.path()))
                        .map(drop)
                        .map_err(|e| failed(&e)),
                    _ => Err(BenchError::Unknown {
                        operation: operation.clone(),
                    }),
                }?;
                times.push(started.elapsed());
            }
            Ok(Timed {
                operation: operation.clone(),
                budget: *budget,
                took: Took::Median(median(times)),
            })
        })
        .collect()
}

/// The median of `RUNS` calls of `tasks.next` with the index in `warm`,
/// after one call whose time is discarded: it extracts, and Python compiles
/// the plugin's bytecode.
fn call(board: &Board, warm: &Path, path: OsString, host: &Host) -> Result<Duration, BenchError> {
    let directory = host.data.join("board/plugins");
    let plugins = Plugins {
        directory: Some(&directory),
        environment: Environment {
            path: Some(path),
            home: host.home.clone(),
            cache: Some(warm.into()),
            data: Some(host.data.clone().into()),
            ..Environment::default()
        },
        limits: Limits::default(),
    };
    let target: Target = "tasks.next".parse().map_err(|error| failed(&error))?;
    let once = || {
        board
            .call(&target, b"{}", &plugins, Some(warm))
            .map(drop)
            .map_err(|error| failed(&error))
    };
    once()?;
    let mut times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let started = Instant::now();
        once()?;
        times.push(started.elapsed());
    }
    Ok(median(times))
}

/// The middle of `times`.
fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times.get(times.len() / 2).copied().unwrap_or_default()
}

fn failed(error: &dyn std::fmt::Display) -> BenchError {
    BenchError::Board {
        message: error.to_string(),
    }
}

/// Writes a board of `records` tasks, a hundred to a directory, each
/// referring to one of a hundred projects, into `root`, with a minimal git
/// directory so that `root` is a repository's working tree. The board
/// enables the tasks plugin and holds `TASKS` of its open tasks; one in five
/// waits on the first, so `tasks.next` asks Board what blocks it.
#[expect(
    clippy::disallowed_methods,
    reason = "bench writes the board it measures, and nothing else"
)]
pub(crate) fn generate(root: &Path, records: usize) -> std::io::Result<()> {
    let write = |path: &str, text: &str| -> std::io::Result<()> {
        let path = root.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)
    };
    for directory in [".git/objects", ".git/refs/heads"] {
        std::fs::create_dir_all(root.join(directory))?;
    }
    write(".git/HEAD", "ref: refs/heads/main\n")?;
    write(
        ".git/config",
        "[core]\n\trepositoryformatversion = 0\n\tbare = false\n",
    )?;
    write(
        ".board/board.json",
        r#"{"$type": "board/config/v1", "plugins": ["tasks"]}"#,
    )?;
    write(
        ".board/types/bench/task/v1.schema.json",
        r#"{"type": "object", "required": ["title"], "properties": {"title": {"type": "string"}, "project": {"type": "string", "format": "board-ref", "x-board-type": "bench/project/v1"}}}"#,
    )?;
    write(
        ".board/types/bench/project/v1.schema.json",
        r#"{"type": "object"}"#,
    )?;
    write(".board/types/tasks/task/v1.schema.json", TASK_SCHEMA)?;
    for task in 0..TASKS {
        let after = if task % 5 == 4 {
            r#", "after": ["/todo/t0"]"#
        } else {
            ""
        };
        write(
            &format!("todo/t{task}.json"),
            &format!(
                r#"{{"$type": "tasks/task/v1", "title": "task {task}", "status": "open"{after}}}"#
            ),
        )?;
    }
    for project in 0..100 {
        write(
            &format!("projects/p{project}.json"),
            r#"{"$type": "bench/project/v1"}"#,
        )?;
    }
    for task in 0..records {
        write(
            &format!("tasks/d{}/t{task}.json", task / PER_DIRECTORY),
            &format!(
                r#"{{"$type": "bench/task/v1", "title": "task {task}", "project": "/projects/p{}"}}"#,
                task % 100
            ),
        )?;
    }
    // A file modified in the second a refresh begins is hashed again, as
    // git does, so the warm runs start in a later second.
    std::thread::sleep(Duration::from_millis(1_100));
    Ok(())
}

/// Why `bench` could not measure.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BenchError {
    #[error("docs/architecture/performance/budgets.md has no budgets table")]
    NoBudgets,
    #[error("budget row {line:?} is not `| operation | <n> ms |` or `<n> s`")]
    Budget { line: String },
    #[error("budget {operation:?} names no operation bench can time")]
    Unknown { operation: String },
    #[error("the board cannot be measured: {message}")]
    Board { message: String },
    #[error("the board cannot be written")]
    Write(#[source] std::io::Error),
    #[error(
        "{} holds no `board`; build it with xtask's profile first, \
         as in `cargo build --release --package board-cli`",
        directory.display()
    )]
    NoBoard { directory: PathBuf },
}

impl BenchError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::NoBudgets | Self::Budget { .. } => "xtask::bench::bad_budget",
            Self::Unknown { .. } => "xtask::bench::unknown_operation",
            Self::Board { .. } | Self::Write(_) => "xtask::bench::failed",
            Self::NoBoard { .. } => "xtask::bench::no_board",
        }
    }
}

#[cfg(test)]
mod tests;
