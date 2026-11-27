//! The newest CI timing run, imported into `/suites/board-speed-ci` through
//! gh (docs/architecture/performance/timing.md, The CI timing run).

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use board_model::LogicalPath;
use board_ops::{Board, Expect, PutError};
use board_plugin::child::{self, Process};
use serde_json::{Value, json};

/// The suite each CI timing run is a run of.
pub(crate) const CI_SUITE: &str = "/suites/board-speed-ci";
/// Board's own repository, whose timing workflow runs on each push to main.
const REPOSITORY: &str = "crumbkit/board";
/// The workflow and the artifact it uploads, which holds `run.json`.
const WORKFLOW: &str = "timing.yml";
const ARTIFACT: &str = "board-speed-run";
/// How long each gh command may run.
const DEADLINE: Duration = Duration::from_secs(30);

/// The gh that reaches GitHub: its program, the environment it runs with,
/// and an empty directory it downloads into.
#[derive(Debug, Clone)]
pub(crate) struct Gh {
    pub(crate) program: String,
    pub(crate) environment: Vec<(String, OsString)>,
    pub(crate) directory: PathBuf,
}

impl Gh {
    fn run(&self, arguments: &[&str]) -> Result<Vec<u8>, String> {
        let arguments: Vec<String> = arguments.iter().map(|a| (*a).to_owned()).collect();
        let process = Process {
            program: &self.program,
            arguments: &arguments,
            directory: &self.directory,
            input: b"",
            deadline: DEADLINE,
        };
        let shown = format!("gh {}", arguments.join(" "));
        let exited = child::run(&process, &self.environment)
            .map_err(|error| format!("`{shown}` did not run: {error}"))?;
        if exited.code == Some(0) {
            return Ok(exited.stdout);
        }
        let stderr = String::from_utf8_lossy(&exited.stderr);
        let tail: String = (stderr.trim().chars().rev().take(200).collect::<Vec<_>>())
            .into_iter()
            .rev()
            .collect();
        Err(format!("`{shown}` failed: {tail}"))
    }

    /// The id of the newest successful timing run on a push to main, if
    /// there is one.
    fn newest(&self) -> Result<Option<u64>, String> {
        let listed = self.run(&[
            "run",
            "list",
            "--repo",
            REPOSITORY,
            "--workflow",
            WORKFLOW,
            "--branch",
            "main",
            "--event",
            "push",
            "--status",
            "success",
            "--limit",
            "1",
            "--json",
            "databaseId",
        ])?;
        let listed: Value = serde_json::from_slice(&listed)
            .map_err(|error| format!("gh listed runs that are not JSON: {error}"))?;
        Ok(listed[0]["databaseId"].as_u64())
    }

    /// The run record that run `id` uploaded.
    fn download(&self, id: u64) -> Result<Vec<u8>, String> {
        let id = id.to_string();
        let directory = self.directory.display().to_string();
        self.run(&[
            "run", "download", &id, "--repo", REPOSITORY, "--name", ARTIFACT, "--dir", &directory,
        ])?;
        std::fs::read(self.directory.join("run.json"))
            .map_err(|error| format!("the artifact of run {id} holds no run.json: {error}"))
    }
}

/// What an import did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Imported {
    /// It wrote the newest run here.
    Wrote(LogicalPath),
    /// The board already holds the newest run, or CI has none.
    Held,
}

/// Imports the newest CI timing run into `board` unless it holds it,
/// counting as over budget only the commands in `live`. Any failure of gh
/// or of the artifact is a warning, and writes nothing.
pub(crate) fn import(board: &Board, gh: &Gh, live: &BTreeSet<String>) -> Result<Imported, String> {
    let Some(id) = gh.newest()? else {
        return Ok(Imported::Held);
    };
    let path: LogicalPath = format!("{CI_SUITE}/runs/{id}")
        .parse()
        .map_err(|error| format!("{error}"))?;
    if board.get(&path).is_ok() {
        return Ok(Imported::Held);
    }
    let bytes = gh.download(id)?;
    let run = parsed(&bytes, live)
        .ok_or_else(|| format!("the artifact of run {id} is not a runs/run/v1 of {CI_SUITE}"))?;
    let refused = |error: PutError| error.refusal().message;
    let suite: LogicalPath = CI_SUITE.parse().map_err(|error| format!("{error}"))?;
    match board.put(&suite, Expect::Absent, ci_suite().to_string().as_bytes()) {
        Ok(_) | Err(PutError::Stale { .. }) => {}
        Err(error) => return Err(refused(error)),
    }
    match board.put(&path, Expect::Absent, run.to_string().as_bytes()) {
        Ok(_) => Ok(Imported::Wrote(path)),
        // Another check imported it first.
        Err(PutError::Stale { .. }) => Ok(Imported::Held),
        Err(error) => Err(refused(error)),
    }
}

/// The run `bytes` holds, with `over_budget` counting the commands in
/// `live` that failed and `silent_over_budget` the rest, or none when it
/// is not a run of the CI suite.
fn parsed(bytes: &[u8], live: &BTreeSet<String>) -> Option<Value> {
    let mut run: Value = serde_json::from_slice(bytes).ok()?;
    let ours = run["$type"] == "runs/run/v1" && run["suite"] == CI_SUITE;
    let results = run["results"].as_array().filter(|_| ours)?;
    let failed = |live_wanted: bool| {
        (results.iter())
            .filter(|result| result["outcome"] == "fail")
            .filter(|result| {
                result["case"]
                    .as_str()
                    .is_some_and(|case| live.contains(case) == live_wanted)
            })
            .count()
    };
    let (over, silent) = (failed(true), failed(false));
    let measures = run["measures"].as_object_mut()?;
    measures.insert("over_budget".to_owned(), json!(over));
    measures.insert("silent_over_budget".to_owned(), json!(silent));
    measures.insert("killed".to_owned(), json!(0));
    Some(run)
}

/// The suite every CI timing run is a run of.
pub(crate) fn ci_suite() -> Value {
    json!({
        "$type": "runs/suite/v1",
        "kind": "benchmark",
        "method": "On each push to main, timing.yml runs `cargo xtask bench --hot` on Linux, which times each budgeted command in process on a generated board of 10,000 records and 100 open changes, and judges its p90 and median against the budgets in docs/architecture/performance/budgets.md. `board timing check --record` imports the newest run through gh; on import, over_budget counts only the commands live then.",
        "command": "cargo xtask bench --hot --json",
        "subjects": ["crumbkit/board"],
        "measures": [
            {"name": "over_budget", "unit": "commands", "description": "Live commands over their budget, counted when the run is imported."},
            {"name": "silent_over_budget", "unit": "commands", "description": "Silent commands over their budget."},
            {"name": "killed", "unit": "runs", "description": "Always 0: a CI run kills no command."}
        ]
    })
}
