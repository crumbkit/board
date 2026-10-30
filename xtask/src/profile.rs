//! `cargo xtask profile [--save <file>] -- <board arguments>`: builds
//! `board` in the `profiling` profile, runs it once to warm its index, then
//! records twenty runs under samply, which opens the Firefox Profiler or,
//! with `--save`, writes the profile to a file
//! (docs/architecture/performance/timing.md).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::process;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Profile {
    pub(crate) save: Option<PathBuf>,
    pub(crate) arguments: Vec<OsString>,
}

impl Profile {
    /// Reads `[--save <file>] -- <board arguments>`.
    pub(crate) fn parse(mut arguments: impl Iterator<Item = String>) -> Option<Self> {
        let mut save = None;
        let mut next = arguments.next()?;
        if next == "--save" {
            save = Some(PathBuf::from(arguments.next()?));
            next = arguments.next()?;
        }
        if next != "--" {
            return None;
        }
        let arguments: Vec<OsString> = arguments.map(OsString::from).collect();
        (!arguments.is_empty()).then_some(Self { save, arguments })
    }
}

/// samply in a directory of `path`.
pub(crate) fn samply(path: &OsStr) -> Result<PathBuf, ProfileError> {
    std::env::split_paths(path)
        .map(|directory| directory.join("samply"))
        .find(|samply| samply.is_file())
        .ok_or(ProfileError::NoSamply)
}

/// Builds, warms and records `profile` from the workspace at `root`.
pub(crate) fn run(root: &Path, profile: &Profile) -> Result<(), ProfileError> {
    let samply = samply(&process::var("PATH").unwrap_or_default())?;
    let mut build = process::command("cargo");
    build.args(["build", "--profile", "profiling", "--package", "board-cli"]);
    status(build.current_dir(root))?;
    let board = root.join("target/profiling/board");
    let mut warm = process::command(&board);
    status(warm.args(&profile.arguments).stdout(Stdio::null()))?;
    let mut record = process::command(samply);
    record.args(["record", "--rate", "4000", "--iteration-count", "20"]);
    if let Some(save) = &profile.save {
        record.arg("--save-only").arg("--output").arg(save);
    }
    status(record.arg("--").arg(board).args(&profile.arguments))
}

fn status(command: &mut Command) -> Result<(), ProfileError> {
    let code = command.status().ok().map(|status| status.code());
    let program = command.get_program().to_owned();
    (code == Some(Some(0)))
        .then_some(())
        .ok_or(ProfileError::Failed {
            program,
            code: code.flatten(),
        })
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProfileError {
    #[error("samply is not on PATH; install it with `cargo install --locked samply`")]
    NoSamply,
    #[error("{} failed with exit code {code:?}", program.display())]
    Failed {
        program: OsString,
        code: Option<i32>,
    },
}

impl ProfileError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::NoSamply => "xtask::profile::no_samply",
            Self::Failed { .. } => "xtask::profile::failed",
        }
    }
}

#[cfg(test)]
mod tests;
