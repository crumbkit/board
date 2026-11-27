//! `cargo xtask bench --hot`: times each command in budgets.md's Command
//! budgets table in process on a generated board, as CI's timing run does
//! on each push to main, and gives a `runs/run/v1` of
//! `/suites/board-speed-ci` (docs/architecture/performance/budgets.md,
//! Command budgets). It only reports.

use std::path::Path;
use std::time::{Duration, Instant};

use board_model::{LogicalPath, PluginName};
use board_ops::{Board, Expect, Plugins};
use board_plugin::Target;
use serde_json::{Value, json};

use super::{BenchError, failed, sample};

/// The suite each run is a run of.
pub(crate) const SUITE: &str = "/suites/board-speed-ci";
/// How many timed runs each row makes, after one whose time is discarded.
const RUNS: usize = 10;
/// The open changes the generated board holds.
pub(crate) const CHANGES: usize = 100;
/// The plugins the hot rows call, which bench enables on the generated board.
const PLUGINS: [&str; 6] = ["tasks", "plans", "orders", "rules", "changes", "runs"];
/// The header of budgets.md's Command budgets table.
const HEADER: &str = "| Command | Budget (p90) | Budget (median) | Session start |";

/// One command's budgets, as budgets.md writes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Budget {
    pub(crate) command: String,
    pub(crate) p90: Duration,
    pub(crate) median: Option<Duration>,
}

/// One row: a command's budgets, and its p50 and p90, or the code Board
/// refused it with.
#[derive(Debug)]
pub(crate) struct Row {
    pub(crate) budget: Budget,
    pub(crate) timed: Result<(Duration, Duration), String>,
}

/// `duration` in milliseconds to a tenth.
fn ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 10_000.0).round() / 10.0
}

impl Row {
    /// `pass` within both budgets, `fail` over either, `error` when Board
    /// refused the command.
    pub(crate) fn outcome(&self) -> &'static str {
        match self.timed {
            Err(_) => "error",
            Ok((p50, p90)) => {
                let over_median = self.budget.median.is_some_and(|median| p50 > median);
                if p90 > self.budget.p90 || over_median {
                    "fail"
                } else {
                    "pass"
                }
            }
        }
    }

    fn result(&self) -> Value {
        let budget = &self.budget;
        let mut measures = json!({"budget_ms": ms(budget.p90)});
        if let Some(median) = budget.median {
            measures["median_budget_ms"] = json!(ms(median));
        }
        let mut result = json!({"case": budget.command, "outcome": self.outcome()});
        match &self.timed {
            Ok((p50, p90)) => {
                measures["p50_ms"] = json!(ms(*p50));
                measures["p90_ms"] = json!(ms(*p90));
                if self.outcome() == "fail" {
                    let median = budget.median.map_or_else(
                        || "no median budget".to_owned(),
                        |m| format!("a {} ms median budget", ms(m)),
                    );
                    result["note"] = json!(format!(
                        "p50 {} ms and p90 {} ms against a {} ms budget and {median}",
                        ms(*p50),
                        ms(*p90),
                        ms(budget.p90),
                    ));
                }
            }
            Err(code) => result["note"] = json!(format!("Board refused it with {code}")),
        }
        result["measures"] = measures;
        result
    }
}

/// A cell such as `10,000 ms` as a duration.
fn duration(cell: &str) -> Option<Duration> {
    let amount = cell.strip_suffix(" ms")?.replace(',', "");
    Some(Duration::from_millis(amount.parse().ok()?))
}

/// The Command budgets table in `docs`.
pub(crate) fn budgets(docs: &str) -> Result<Vec<Budget>, BenchError> {
    let mut lines = docs.lines().skip_while(|line| *line != HEADER);
    if lines.next().is_none() {
        return Err(BenchError::NoBudgets);
    }
    (lines.skip(1).take_while(|line| line.starts_with('|')))
        .map(|line| {
            let bad = || BenchError::Budget {
                line: line.to_owned(),
            };
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            let [command, p90, median, _] = cells[..] else {
                return Err(bad());
            };
            Ok(Budget {
                command: command.trim_matches('`').to_owned(),
                p90: duration(p90).ok_or_else(bad)?,
                median: match median {
                    "-" => None,
                    median => Some(duration(median).ok_or_else(bad)?),
                },
            })
        })
        .collect()
}

/// The shipped development flow, which each generated change follows.
const FLOW: &str = include_str!("../../../crates/board-plugin-changes/flows/development.json");

/// The open changes a generated board holds, in the development flow's
/// plan phase, and the flow, as files beside its records.
pub(crate) fn changes() -> Vec<(String, String)> {
    let flow = ("flows/development.json".to_owned(), FLOW.to_owned());
    (0..CHANGES)
        .map(|change| {
            let record = json!({"$type": "changes/change/v2", "title": format!("Change {change}"),
                "phase": "plan", "flow": "/flows/development"});
            (format!("changes/c{change}.json"), record.to_string())
        })
        .chain([flow])
        .collect()
}

/// The board at `root` with every built-in plugin built in.
fn opened(root: &Path) -> Result<Board, BenchError> {
    let mut board = Board::open(root, None).map_err(|error| failed(&error))?;
    board.built_by("xtask-bench");
    board.build_in(board_plugins::BUILT_IN.0);
    Ok(board)
}

/// Enables [`PLUGINS`] on the generated board at `root`.
pub(crate) fn enable(root: &Path) -> Result<(), BenchError> {
    let board = opened(root)?;
    let cache = tempfile::tempdir().map_err(|error| failed(&error))?;
    for name in PLUGINS {
        let name: PluginName = name.parse().map_err(|error| failed(&error))?;
        board
            .plugin_enable(None, Some(cache.path()), &name, &[])
            .map_err(|error| failed(&error))?;
    }
    Ok(())
}

/// What each capability row calls with.
fn input(command: &str) -> &'static str {
    match command {
        "rules.check" => r#"{"path": "/"}"#,
        "rules.for-path" => r#"{"path": "/todo/tasks/t0.md"}"#,
        "tasks.available" => r#"{"repo": "/"}"#,
        "changes.next" | "changes.brief" | "changes.advance" => r#"{"path": "/changes/c0"}"#,
        _ => "{}",
    }
}

/// The refusal a row's run ends with once its work is done: an advance
/// whose gate is unmet has read the change and checked every gate, as
/// most advances in use do, and writes nothing, so every run is the same.
fn finished(command: &str, code: &str) -> bool {
    command == "changes.advance" && code == "changes::gate::unmet"
}

/// Times each of `budgets` on the board at `root`, with its index in a new
/// cache directory, warmed by one check.
pub(crate) fn run(root: &Path, budgets: &[Budget]) -> Result<Vec<Row>, BenchError> {
    let board = opened(root)?;
    let warm = tempfile::tempdir().map_err(|error| failed(&error))?;
    let checked = board
        .check(None, Some(warm.path()))
        .map_err(|error| failed(&error))?;
    let (sample, _) = sample(&checked)?;
    let plugins = Plugins {
        cache: Some(warm.path()),
        ..Plugins::default()
    };
    let mut puts = 0_usize;
    let mut once = |command: &str| -> Result<(), String> {
        match command {
            "get" => board
                .get(&sample)
                .map(drop)
                .map_err(|e| e.code().to_string()),
            "ls" => (board.ls(&LogicalPath::root(), Some(warm.path())))
                .map(drop)
                .map_err(|e| e.code().to_string()),
            "query" => (board.query("SELECT count(*) FROM entries", 1, None, Some(warm.path())))
                .map(drop)
                .map_err(|e| e.code().to_string()),
            "put" => {
                puts += 1;
                let path: LogicalPath = format!("/put/hot{puts}")
                    .parse()
                    .map_err(|e: board_model::PathError| e.to_string())?;
                let record = format!(
                    r#"{{"$type": "bench/task/v1", "title": "put {puts}", "project": "/projects/p0"}}"#
                );
                (board.put(&path, Expect::Absent, record.as_bytes()))
                    .map(drop)
                    .map_err(|e| e.code().to_string())
            }
            capability => {
                let target: Target = capability.parse().map_err(|e| format!("{e}"))?;
                match board.call(&target, input(capability).as_bytes(), &plugins) {
                    Ok(_) => Ok(()),
                    Err(error) => {
                        let code = error.refusal().code.as_str().to_owned();
                        if finished(capability, &code) {
                            Ok(())
                        } else {
                            Err(code)
                        }
                    }
                }
            }
        }
    };
    let mut rows = Vec::with_capacity(budgets.len());
    for budget in budgets {
        let timed = once(&budget.command).and_then(|()| {
            let mut times = Vec::with_capacity(RUNS);
            for _ in 0..RUNS {
                let started = Instant::now();
                once(&budget.command)?;
                times.push(started.elapsed());
            }
            times.sort();
            Ok((percentile(&times, 50), percentile(&times, 90)))
        });
        rows.push(Row {
            budget: budget.clone(),
            timed,
        });
    }
    Ok(rows)
}

/// The `percent`th percentile of sorted `times` by nearest rank.
fn percentile(times: &[Duration], percent: usize) -> Duration {
    let rank = (times.len() * percent).div_ceil(100).max(1);
    times.get(rank - 1).copied().unwrap_or_default()
}

/// The `runs/run/v1` of `rows`, timed from `started` to `finished` on a
/// board of `records` records, of the commit `commit`.
pub(crate) fn record(
    rows: &[Row],
    (started, finished): (&str, &str),
    records: usize,
    commit: &str,
) -> Value {
    let over = rows.iter().filter(|row| row.outcome() == "fail").count();
    let refused = rows.iter().filter(|row| row.outcome() == "error").count();
    json!({
        "$type": "runs/run/v1",
        "suite": SUITE,
        "started": started,
        "finished": finished,
        "scope": "full",
        "dirty": false,
        "host": {"os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "logical_cpus": std::thread::available_parallelism().map_or(1, usize::from)},
        "versions": {"board": commit},
        "setup": format!("cargo xtask bench --hot on a generated board of {records} records and {CHANGES} open changes"),
        "measures": {"over_budget_any": over, "refused": refused},
        "results": rows.iter().map(Row::result).collect::<Vec<_>>(),
        "summary": format!("{over} commands over budget and {refused} refused, of {}", rows.len()),
        "written_by": "xtask/bench-hot",
    })
}
