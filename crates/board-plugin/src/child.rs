//! A process a built-in plugin starts, such as git, gh or an agent, through
//! the process module, which stays the one owner of child processes and
//! signals (STANDARDS.md 7).

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::time::Duration;

use crate::process::{self, Finished, Limits, Stopped};

/// A process to start: its program, found on the environment's `PATH`, its
/// arguments, the directory it runs in, what it reads on stdin, and how
/// long it may run.
#[derive(Clone, Copy, Debug)]
pub struct Process<'a> {
    pub program: &'a str,
    pub arguments: &'a [String],
    pub directory: &'a Path,
    pub input: &'a [u8],
    pub deadline: Duration,
}

/// A process that exited: its exit code, `None` after a signal, and what it
/// wrote. Stdout is whole; stderr keeps its last 64 KiB.
#[derive(Debug)]
pub struct Exited {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// A process that could not start, or was stopped.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("{source}")]
    NotStarted { source: io::Error },
    /// It ran past its deadline, and its group was stopped.
    #[error("it ran past its deadline and was stopped")]
    Timeout { stderr: Vec<u8> },
    /// It wrote more than 16 MiB to stdout, and its group was stopped.
    #[error("it wrote more than {limit} bytes to stdout and was stopped")]
    TooLarge { limit: usize, stderr: Vec<u8> },
}

/// Runs `process` with only `environment`, as the leader of its own process
/// group. Every process still in the group is killed when the process
/// exits, even while one of them holds its output open, past its deadline,
/// past 16 MiB of stdout, or when Board gets SIGINT, SIGTERM or SIGHUP. A
/// process that left the group, as `setsid` does, is not killed.
pub fn run(
    process: &Process<'_>,
    environment: &[(String, OsString)],
) -> Result<Exited, ProcessError> {
    run_within(process, environment, Limits::default().stdout)
}

/// [`run`], reading at most `stdout` bytes of stdout.
fn run_within(
    process: &Process<'_>,
    environment: &[(String, OsString)],
    stdout: usize,
) -> Result<Exited, ProcessError> {
    let environment: Vec<(&str, OsString)> = (environment.iter())
        .map(|(name, value)| (name.as_str(), value.clone()))
        .collect();
    let limits = Limits {
        time: process.deadline,
        stdout,
        ..Limits::default()
    };
    let (_, finished) = process::run_alone(
        Path::new(process.program),
        process.arguments,
        process.directory,
        &environment,
        process.input.to_vec(),
        limits,
    );
    match finished {
        Ok(Finished {
            code,
            stdout,
            stderr,
        }) => Ok(Exited {
            code,
            stdout,
            stderr,
        }),
        Err(Stopped::Io(source)) => Err(ProcessError::NotStarted { source }),
        Err(Stopped::Timeout(stderr)) => Err(ProcessError::Timeout { stderr }),
        Err(Stopped::TooLarge(stderr)) => Err(ProcessError::TooLarge {
            limit: limits.stdout,
            stderr,
        }),
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests;
