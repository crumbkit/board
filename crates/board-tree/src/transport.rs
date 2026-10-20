//! The one module that runs the `git` executable: the fetch and push of
//! `board sync` (docs/architecture.md, Writes). git carries credentials,
//! SSH and hosts as the owner set them up; gix reads what it brings.
#![expect(
    clippy::disallowed_methods,
    reason = "board-tree's transport module owns the git child process"
)]

use std::path::Path;
use std::process::{Command, Stdio};

use crate::write::sync::SyncError;

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
pub(crate) fn fetch(workdir: &Path, upstream: &Upstream) -> Result<(), SyncError> {
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
    let output = run(workdir, &arguments)?;
    if output.status.success() {
        return Ok(());
    }
    Err(failed("fetch", upstream, &output))
}

/// Pushes `commit` in `workdir` to `upstream`'s branch, all or nothing.
pub(crate) fn push(workdir: &Path, upstream: &Upstream, commit: &str) -> Result<Pushed, SyncError> {
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
    let output = run(workdir, &arguments)?;
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

fn run(workdir: &Path, arguments: &[&str]) -> Result<std::process::Output, SyncError> {
    Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| SyncError::NotStarted { source })
}

fn failed(
    operation: &'static str,
    upstream: &Upstream,
    output: &std::process::Output,
) -> SyncError {
    SyncError::Failed {
        operation,
        remote: upstream.remote.clone(),
        status: output.status.to_string(),
        stderr: output.stderr.clone(),
    }
}
