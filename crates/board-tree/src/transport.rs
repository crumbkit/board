//! The one module that runs the `git` executable: the fetch and push of
//! `board sync` (docs/architecture.md, Writes). git carries credentials,
//! SSH and hosts as the owner set them up; gix reads what it brings.
#![expect(
    clippy::disallowed_methods,
    reason = "board-tree's transport module owns the git child process"
)]

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::write::sync::SyncError;

/// How long a fetch or push may run before Board stops git.
pub const GIT_DEADLINE: Duration = Duration::from_mins(1);

/// How often a run checks whether git has finished.
const POLL: Duration = Duration::from_millis(10);
/// How long git has to exit after SIGTERM, and its pipes to close after
/// it exits.
const GRACE: Duration = Duration::from_secs(1);

/// The remote branch that the branch HEAD is on follows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upstream {
    /// The remote's name, such as `origin`.
    pub remote: String,
    /// The branch on the remote, such as `main`.
    pub branch: String,
    /// The ref here that tracks it, such as `refs/remotes/origin/main`.
    pub(crate) tracking: String,
}

/// What a push did: moved the remote branch, or was refused by the remote
/// with the reason git printed, such as `[rejected] (fetch first)`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Pushed {
    Done,
    Rejected(String),
}

/// Fetches `upstream`'s branch in `workdir` into its tracking ref, checking
/// the objects it brings.
pub(crate) fn fetch(
    workdir: &Path,
    upstream: &Upstream,
    deadline: Duration,
) -> Result<(), SyncError> {
    let refspec = format!("+refs/heads/{}:{}", upstream.branch, upstream.tracking);
    let arguments = [
        "-c",
        "fetch.fsckObjects=true",
        "fetch",
        "--quiet",
        "--",
        &upstream.remote,
        &refspec,
    ];
    let output = run(workdir, &arguments, "fetch", upstream, deadline)?;
    if output.status.success() {
        return Ok(());
    }
    Err(failed("fetch", upstream, &output))
}

/// Pushes `commit` in `workdir` to `upstream`'s branch, all or nothing.
pub(crate) fn push(
    workdir: &Path,
    upstream: &Upstream,
    commit: &str,
    deadline: Duration,
) -> Result<Pushed, SyncError> {
    let refspec = format!("{commit}:refs/heads/{}", upstream.branch);
    let arguments = [
        "push",
        "--atomic",
        "--porcelain",
        "--quiet",
        "--",
        &upstream.remote,
        &refspec,
    ];
    let output = run(workdir, &arguments, "push", upstream, deadline)?;
    if output.status.success() {
        return Ok(Pushed::Done);
    }
    match rejected(&output.stdout) {
        Some(reason) => Ok(Pushed::Rejected(reason)),
        None => Err(failed("push", upstream, &output)),
    }
}

/// The reason in the `!` line of `git push --porcelain`'s stdout, which
/// says the remote refused an update, or `None` when no line does.
pub(crate) fn rejected(stdout: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .find_map(|line| line.strip_prefix('!'))
        .map(|line| line.split('\t').nth(2).unwrap_or(line).trim().to_owned())
}

/// Runs git with `arguments` in `workdir` and returns what it printed.
/// Git still running when `deadline` passes is stopped, and the run is
/// [`SyncError::TimedOut`], so a hung remote holds the board lock no longer
/// than the deadline. A helper git started, such as ssh, that outlives git
/// holds no Board lock.
fn run(
    workdir: &Path,
    arguments: &[&str],
    operation: &'static str,
    upstream: &Upstream,
    deadline: Duration,
) -> Result<Output, SyncError> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| SyncError::NotStarted { source })?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let until = Instant::now() + deadline;
    let stopped = loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(finished(status, stdout, stderr)),
            Ok(None) if Instant::now() < until => thread::sleep(POLL),
            Ok(None) => {
                break SyncError::TimedOut {
                    operation,
                    remote: upstream.remote.clone(),
                    deadline,
                };
            }
            Err(error) => {
                break SyncError::Failed {
                    operation,
                    remote: upstream.remote.clone(),
                    status: format!("its status could not be read: {error}"),
                    stderr: Vec::new(),
                };
            }
        }
    };
    stop(&mut child);
    Err(stopped)
}

/// What git printed, once it has exited with `status`. A helper git started
/// may still hold a pipe open; after [`GRACE`] that pipe's bytes are left
/// out, so a run git finished is never reported as timed out.
fn finished(status: ExitStatus, stdout: Drained, stderr: Drained) -> Output {
    let until = Instant::now() + GRACE;
    while !(stdout.is_finished() && stderr.is_finished()) && Instant::now() < until {
        thread::sleep(POLL);
    }
    let read = |pipe: Drained| {
        if pipe.is_finished() {
            pipe.join().unwrap_or_default()
        } else {
            Vec::new()
        }
    };
    Output {
        status,
        stdout: read(stdout),
        stderr: read(stderr),
    }
}

/// Sends git SIGTERM, on which git removes the lock files it holds, gives it
/// [`GRACE`] to exit, then kills it and reaps it. Off Unix, git is killed at
/// once.
fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        use rustix::process::{Pid, Signal, kill_process};
        if kill_process(Pid::from_child(child), Signal::TERM).is_ok() {
            let until = Instant::now() + GRACE;
            while Instant::now() < until && matches!(child.try_wait(), Ok(None)) {
                thread::sleep(POLL);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// A pipe of git's, read to its end on a thread of its own.
type Drained = JoinHandle<Vec<u8>>;

/// Reads `pipe` to its end on a thread of its own.
fn drain(pipe: Option<impl Read + Send + 'static>) -> Drained {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

fn failed(operation: &'static str, upstream: &Upstream, output: &Output) -> SyncError {
    SyncError::Failed {
        operation,
        remote: upstream.remote.clone(),
        status: output.status.to_string(),
        stderr: output.stderr.clone(),
    }
}
