//! `cargo xtask bench`: times Board's operations on a generated board, or on
//! one given with `--board`, against the budgets in
//! docs/architecture/performance/budgets.md. It reports every operation and
//! fails when one takes more than three times its budget. Build it with
//! `--release`: a debug build measures the compiler, not Board.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use board_model::{Kind, LogicalPath, ObjectId};
use board_ops::{Board, CheckReport, Expect, Plugins};
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
/// The rounds `--against` runs unless `--runs` says otherwise.
const AGAINST_RUNS: usize = 15;
/// The most repositories a generated mounted board mounts.
const MOUNTS: usize = 20;

/// The tasks plugin's tasks in a generated board: as many as one section of
/// `tasks.available` shows.
const TASKS: usize = 100;
/// The schema the tasks plugin ships, which `board plugin enable` writes.
const TASK_SCHEMA: &str = include_str!("../../plugins/tasks/types/tasks/task/v2.schema.json");

/// The operation that times a call to a Python plugin.
pub(crate) const CALL: &str = "`board call tasks.available --repo /`, warm";
/// The operation that times a put, which writes a record into the board.
pub(crate) const PUT: &str = "`board put` of a small record, warm";
/// The operation that times a query of a board that mounts others.
pub(crate) const MOUNTED: &str = "simple `board query`, warm, 20 mounts";

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
    /// The base `board` binary `--against` compares this one with.
    pub(crate) against: Option<PathBuf>,
    /// The rounds `--against` runs, if `--runs` sets them.
    pub(crate) runs: Option<usize>,
}

impl Options {
    /// Reads `[--board <dir>] [--records <n>] [--against <board> [--runs <n>]]`.
    pub(crate) fn parse(mut arguments: impl Iterator<Item = String>) -> Option<Self> {
        let mut options = Self {
            board: None,
            records: RECORDS,
            against: None,
            runs: None,
        };
        while let Some(flag) = arguments.next() {
            match flag.as_str() {
                "--board" => options.board = Some(PathBuf::from(arguments.next()?)),
                "--records" => options.records = arguments.next()?.parse().ok()?,
                "--against" => options.against = Some(PathBuf::from(arguments.next()?)),
                "--runs" => {
                    options.runs = Some(arguments.next()?.parse().ok().filter(|runs| *runs > 0)?);
                }
                _ => return None,
            }
        }
        Some(options)
    }

    /// The base binary `--against` names and the rounds to run, if it names
    /// one: an existing file, and `--runs` only with it.
    pub(crate) fn against(&self) -> Result<Option<(&Path, usize)>, BenchError> {
        match (&self.against, self.runs) {
            (None, None) => Ok(None),
            (None, Some(_)) => Err(BenchError::RunsWithoutAgainst),
            (Some(base), _) if !base.is_file() => Err(BenchError::NoAgainst { path: base.clone() }),
            (Some(base), runs) => Ok(Some((base, runs.unwrap_or(AGAINST_RUNS)))),
        }
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
/// `get` reads the first record a check finds, `ls` lists its directory, the
/// plugin call runs on `host`, and `put` writes a record for each run into a
/// board bench generated. `mounted` is the mounted board generated beside
/// it, which the mounted query reads; a board given with `--board` has none,
/// and bench only reads it.
pub(crate) fn run(
    root: &Path,
    budgets: &[(String, Duration)],
    host: &Host,
    mounted: Option<&Path>,
) -> Result<Vec<Timed>, BenchError> {
    let (board, warm, checked) = warmed(root)?;
    let given = mounted.is_none();
    let mounted = mounted.map(warmed).transpose()?;
    let (sample, directory) = sample(&checked)?;
    let sample = &sample;
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
            let skipped = match (operation.as_str(), given) {
                (PUT, true) => "writes only a board it generated",
                (MOUNTED, true) => "mounts repositories only into a board it generated",
                _ => "",
            };
            if !skipped.is_empty() {
                return Ok(Timed {
                    operation: operation.clone(),
                    budget: *budget,
                    took: Took::Skipped(format!(
                        "the board was given with --board, and bench {skipped}"
                    )),
                });
            }
            let mut times = Vec::with_capacity(RUNS);
            for run in 0..RUNS {
                let cold = tempfile::tempdir().map_err(|error| failed(&error))?;
                let path: LogicalPath = format!("/put/r{run}")
                    .parse()
                    .map_err(|error| failed(&error))?;
                let record = format!(
                    r#"{{"$type": "bench/task/v1", "title": "put {run}", "project": "/projects/p0"}}"#
                );
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
                    "simple `board query`, warm" => count(&board, warm.path()),
                    MOUNTED => (mounted.as_ref()).map_or(Ok(()), |(b, w, _)| count(b, w.path())),
                    "full index, cold" => board
                        .check(None, Some(cold.path()))
                        .map(drop)
                        .map_err(|e| failed(&e)),
                    PUT => board
                        .put(&path, Expect::Absent, record.as_bytes())
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

/// The board at `root`, checked once into a new cache directory, and what
/// the check found.
pub(crate) fn warmed(root: &Path) -> Result<(Board, tempfile::TempDir, CheckReport), BenchError> {
    let mut board = Board::open(root, None).map_err(|error| failed(&error))?;
    // One name serves every build, as each bench's cache is a new temporary
    // directory.
    board.built_by("xtask-bench");
    let warm = tempfile::tempdir().map_err(|error| failed(&error))?;
    let checked = board
        .check(None, Some(warm.path()))
        .map_err(|error| failed(&error))?;
    Ok((board, warm, checked))
}

/// The first valid record `checked` found, which `get` reads, and its
/// directory, which `ls` lists.
pub(crate) fn sample(checked: &CheckReport) -> Result<(LogicalPath, LogicalPath), BenchError> {
    let sample = (checked.entries.iter())
        .find(|entry| entry.kind == Kind::Record && entry.status == Status::Ok)
        .and_then(|entry| entry.logical.clone())
        .ok_or_else(|| failed(&"it holds no valid record to get"))?;
    let directory = (sample.as_str().rsplit_once('/'))
        .and_then(|(parent, _)| parent.parse().ok())
        .unwrap_or_else(LogicalPath::root);
    Ok((sample, directory))
}

/// The simple query the query rows time.
fn count(board: &Board, warm: &Path) -> Result<(), BenchError> {
    board
        .query("SELECT count(*) FROM entries", 1, None, Some(warm))
        .map(drop)
        .map_err(|error| failed(&error))
}

/// The median of `RUNS` calls of `tasks.available` on the board's own tree,
/// which syncs no mount and reads no git remote, with the index in `warm`,
/// after one call whose time is discarded: Python compiles
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
        cache: Some(warm),
    };
    let target: Target = "tasks.available".parse().map_err(|error| failed(&error))?;
    let once = || {
        board
            .call(&target, br#"{"repo": "/"}"#, &plugins)
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
/// directory so that `root` is a repository's working tree, and an identity
/// so that a put can commit, and commits it with `extra`. The board
/// enables the tasks plugin and holds `TASKS` of its open tasks; one in five
/// waits on the first, so `tasks.available` asks Board what blocks it.
/// With `mounts`, each task refers to a project of the board mounted at
/// `/repos/r<n>` instead.
#[expect(
    clippy::disallowed_methods,
    reason = "bench writes the board it measures, and nothing else"
)]
pub(crate) fn generate(
    root: &Path,
    records: usize,
    extra: &[(String, String)],
    mounts: usize,
) -> std::io::Result<ObjectId> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut write = |path: &str, text: &str| -> std::io::Result<()> {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !path.starts_with(".git/") {
            files.push((path.to_owned(), text.to_owned()));
        }
        std::fs::write(full, text)
    };
    for directory in [".git/objects", ".git/refs/heads"] {
        std::fs::create_dir_all(root.join(directory))?;
    }
    write(".git/HEAD", "ref: refs/heads/main\n")?;
    write(
        ".git/config",
        "[core]\n\trepositoryformatversion = 0\n\tbare = false\n\
         [user]\n\tname = bench\n\temail = bench@example.com\n",
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
    write(".board/types/tasks/task/v2.schema.json", TASK_SCHEMA)?;
    for task in 0..TASKS {
        let after = if task % 5 == 4 {
            "after: [\"/todo/tasks/t0.md\"]\n"
        } else {
            ""
        };
        write(
            &format!("todo/tasks/t{task}.md"),
            &format!("---\n$type: tasks/task/v2\ntitle: task {task}\nstatus: open\n{after}---\n"),
        )?;
    }
    for project in 0..100 {
        write(
            &format!("projects/p{project}.json"),
            r#"{"$type": "bench/project/v1"}"#,
        )?;
    }
    for task in 0..records {
        let mount = match mounts {
            0 => String::new(),
            mounts => format!("/repos/r{:02}", task % mounts),
        };
        write(
            &format!("tasks/d{}/t{task}.json", task / PER_DIRECTORY),
            &format!(
                r#"{{"$type": "bench/task/v1", "title": "task {task}", "project": "{mount}/projects/p{}"}}"#,
                task % 100
            ),
        )?;
    }
    for (path, text) in extra {
        write(path, text)?;
    }
    commit(root, &files)
}

/// Writes repositories under `root/repos`, one for each 500 `records` up to
/// `MOUNTS`, each a generated board of `records / 40` tasks with a hundred
/// directories of source files, and a generated board at `root/board` of
/// `records / 5` tasks that mounts each at `/repos/r<n>`. Returns the
/// board's root. The source files name their repository, so no two
/// repositories share a commit, which Board would index once.
pub(crate) fn generate_mounted(root: &Path, records: usize) -> std::io::Result<PathBuf> {
    let mounts = (records / 500).clamp(1, MOUNTS);
    let mut pointers = Vec::with_capacity(mounts);
    for mount in 0..mounts {
        let source: Vec<_> = (0..100)
            .map(|directory| (format!("src/d{directory}/lib.rs"), format!("// r{mount}\n")))
            .collect();
        let repository = root.join(format!("repos/r{mount:02}"));
        let commit = generate(&repository, records / 40, &source, 0)?;
        let pointer = serde_json::json!({"$type": "board/mount/v1",
            "repo": repository, "follow": "main", "commit": commit.to_string()});
        pointers.push((format!("repos/r{mount:02}.json"), pointer.to_string()));
    }
    let board = root.join("board");
    generate(&board, records / 5, &pointers, mounts)?;
    Ok(board)
}

/// Waits until the generated boards' files are a second old: a file
/// modified in the second a refresh begins is hashed again, as git does.
pub(crate) fn settle() {
    std::thread::sleep(Duration::from_millis(1_100));
}

/// Commits `files`, already in the working tree at `root`, in one commit,
/// so that git's index holds every record as a real board's does.
fn commit(root: &Path, files: &[(String, String)]) -> std::io::Result<ObjectId> {
    let board = Board::open(root, None).map_err(std::io::Error::other)?;
    let repository = board.repository();
    let lock = repository
        .lock(board_tree::LOCK_WAIT)
        .map_err(std::io::Error::other)?;
    let writes: Vec<(&str, Option<&[u8]>)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), Some(text.as_bytes())))
        .collect();
    let staged = repository
        .stage_commit(&lock, &writes, "generated")
        .map_err(std::io::Error::other)?
        .ok_or_else(|| std::io::Error::other("a generated board changes its repository"))?;
    repository
        .finish_commit(&lock, staged)
        .map_err(std::io::Error::other)
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
    #[error(
        "{} is not a file; build a base board from main as docs/architecture/performance/budgets.md says",
        path.display()
    )]
    NoAgainst { path: PathBuf },
    #[error("--runs sets the rounds of --against; add --against <board> or drop --runs")]
    RunsWithoutAgainst,
}

impl BenchError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::NoBudgets | Self::Budget { .. } => "xtask::bench::bad_budget",
            Self::Unknown { .. } => "xtask::bench::unknown_operation",
            Self::Board { .. } | Self::Write(_) => "xtask::bench::failed",
            Self::NoBoard { .. } => "xtask::bench::no_board",
            Self::NoAgainst { .. } => "xtask::bench::no_against",
            Self::RunsWithoutAgainst => "xtask::bench::runs_without_against",
        }
    }
}

pub(crate) mod against;

#[cfg(test)]
mod tests;
