//! The grace before a live command's task becomes a question for the
//! owner, measured in lines changed in Board's own repository
//! (docs/architecture/performance/timing.md, The grace).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use board_ops::Board;
use board_plugin::child::{self, Process};
use serde_json::Value;

use super::check::Grace;

/// The mount of Board's own repository on the personal board.
const MOUNT: &str = "repos/board.json";
/// The param that holds the grace, and its heuristic's default, which
/// applies when the board has no value for it.
const PARAM: &str = "overflow-agent-grace";
pub(crate) const DEFAULT_ALLOWED: u64 = 200;
/// How long git may count.
const DEADLINE: Duration = Duration::from_secs(20);

/// Lines added plus removed on `follow` in `repository` since a time, and
/// how many may change before the owner is asked.
#[derive(Debug, Clone)]
pub(crate) struct ChangedLines {
    pub(crate) repository: PathBuf,
    /// The branch the mount follows, such as `origin/main`.
    pub(crate) follow: String,
    pub(crate) allowed: u64,
    pub(crate) environment: Vec<(String, OsString)>,
}

impl Grace for ChangedLines {
    fn allowed(&self) -> u64 {
        self.allowed
    }

    fn changed_since(&self, opened: jiff::Timestamp) -> Option<u64> {
        let arguments: Vec<String> = [
            "log",
            "--numstat",
            "--format=",
            "--no-renames",
            &format!("--since={}", opened.strftime("%Y-%m-%d %H:%M:%S +0000")),
            &self.follow,
            "--",
        ]
        .map(str::to_owned)
        .to_vec();
        let process = Process {
            program: "git",
            arguments: &arguments,
            directory: &self.repository,
            input: b"",
            deadline: DEADLINE,
        };
        let exited = child::run(&process, &self.environment).ok()?;
        (exited.code == Some(0)).then(|| counted(&exited.stdout))
    }
}

/// The lines added plus removed that `git log --numstat` printed. A binary
/// file's `-` counts nothing.
fn counted(numstat: &[u8]) -> u64 {
    (String::from_utf8_lossy(numstat).lines())
        .flat_map(|line| line.split('\t').take(2))
        .filter_map(|count| count.parse::<u64>().ok())
        .sum()
}

/// The grace for Board's repository as the personal board at `personal`
/// mounts it, allowing `allowed` lines, or none when the board does not
/// mount it at /repos/board.
pub(crate) fn for_board(
    personal: &Path,
    allowed: u64,
    environment: Vec<(String, OsString)>,
) -> Option<ChangedLines> {
    let pointer: Value = serde_json::from_slice(&std::fs::read(personal.join(MOUNT)).ok()?).ok()?;
    Some(ChangedLines {
        repository: PathBuf::from(pointer["repo"].as_str()?),
        follow: pointer["follow"].as_str().unwrap_or("HEAD").to_owned(),
        allowed,
        environment,
    })
}

/// The `overflow-agent-grace` param for /repos/board, through the params
/// plugin, or its heuristic's default when the board gives none.
pub(crate) fn allowed(board: &Board, plugins: &board_ops::Plugins<'_>) -> u64 {
    let Ok(target) = "params.get".parse() else {
        return DEFAULT_ALLOWED;
    };
    let input = serde_json::json!({"name": PARAM, "scope": "/repos/board"}).to_string();
    let Ok(report) = board.call(&target, input.as_bytes(), plugins) else {
        return DEFAULT_ALLOWED;
    };
    report.output["value"]
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .and_then(|value| format!("{value:.0}").parse().ok())
        .unwrap_or(DEFAULT_ALLOWED)
}

/// The grace when Board's repository is not mounted: no lines are known,
/// so no question is filed.
pub(crate) struct Unmeasured;

impl Grace for Unmeasured {
    fn allowed(&self) -> u64 {
        u64::MAX
    }

    fn changed_since(&self, _: jiff::Timestamp) -> Option<u64> {
        None
    }
}
