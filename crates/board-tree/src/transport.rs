//! The one module that runs the `git` executable: the fetch and push of
//! `board sync` (docs/architecture.md, Writes). git carries credentials,
//! SSH and hosts as the owner set them up; gix reads what it brings.
#![expect(
    clippy::disallowed_methods,
    reason = "board-tree's transport module owns the git child process"
)]

use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};

use board_plugin::Group;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::git::Repository;
use crate::write::sync::SyncError;

/// How long `board sync` may run before Board stops git. It starts before
/// the wait for the board lock and covers every fetch, push and replay.
pub const GIT_DEADLINE: Duration = Duration::from_mins(1);

/// How often a run checks whether git has finished.
const POLL: Duration = Duration::from_millis(10);
/// How long git's pipes have to close after it exits.
const GRACE: Duration = Duration::from_secs(1);

/// How long one sync may run, from when it was made: every fetch and push
/// it starts shares it.
#[derive(Clone, Copy, Debug)]
pub struct Deadline {
    started: Instant,
    given: Duration,
}

impl Deadline {
    /// A deadline `given` from now.
    pub fn after(given: Duration) -> Self {
        Self {
            started: Instant::now(),
            given,
        }
    }

    /// The time left, zero once it has passed.
    pub fn left(&self) -> Duration {
        self.given.saturating_sub(self.started.elapsed())
    }
}

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

/// How git reaches a remote over ssh, as `GIT_SSH_COMMAND` gives it: the
/// owner's command with `-o BatchMode=yes` added, so ssh asks for no
/// passphrase or host key. git runs in a process group of its own, where a
/// prompt would stop it until the deadline, so a sync that needs one fails
/// at once instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ssh(pub(crate) String);

impl Repository {
    /// The ssh command git would run, as git chooses it: `GIT_SSH_COMMAND`
    /// or `core.sshCommand`, then `GIT_SSH`, then `ssh`, with `BatchMode`
    /// added unless `ssh.variant`, or the program's name, says it is not
    /// OpenSSH. gix reads the variables through git's config, as git does.
    pub(crate) fn ssh(&self) -> Ssh {
        let config = self.git().config_snapshot();
        let shell = |key: &str| config.string(key).map(|value| value.to_string());
        let command = (shell("core.sshCommand").filter(|command| !command.is_empty()))
            .or_else(|| {
                shell("gitoxide.ssh.commandWithoutShellFallback")
                    .filter(|program| !program.is_empty())
                    .map(|program| format!("'{}'", program.replace('\'', "'\\''")))
            })
            .unwrap_or_else(|| "ssh".to_owned());
        let variant = shell("ssh.variant").unwrap_or_default().to_lowercase();
        let program = command.split_whitespace().next().unwrap_or_default();
        let name = program
            .trim_matches(['\'', '"'])
            .rsplit('/')
            .next()
            .unwrap_or_default();
        let openssh = match variant.as_str() {
            "" | "auto" => !["plink", "putty", "tortoiseplink"]
                .iter()
                .any(|other| name.to_lowercase().starts_with(other)),
            other => other == "ssh",
        };
        if openssh {
            Ssh(format!("{command} -o BatchMode=yes"))
        } else {
            Ssh(command)
        }
    }
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
    ssh: &Ssh,
    deadline: Deadline,
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
    let output = run(workdir, &arguments, "fetch", upstream, ssh, deadline)?;
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
    ssh: &Ssh,
    deadline: Deadline,
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
    let output = run(workdir, &arguments, "push", upstream, ssh, deadline)?;
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
/// Git leads a process group of its own through board-plugin's process
/// module, which stops the whole group, ssh and what it started included,
/// when Board gets SIGINT, SIGTERM or SIGHUP. When `deadline` passes with
/// git still running, the group is stopped the same way and the run is
/// [`SyncError::TimedOut`], so a hung remote holds the board lock no longer
/// than the sync's deadline. A helper that outlives a git that finished
/// holds no Board lock. Neither git nor ssh may ask anything: git gets
/// `GIT_TERMINAL_PROMPT=0` and `ssh` as its `GIT_SSH_COMMAND`.
fn run(
    workdir: &Path,
    arguments: &[&str],
    operation: &'static str,
    upstream: &Upstream,
    ssh: &Ssh,
    deadline: Deadline,
) -> Result<Output, SyncError> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(workdir)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", &ssh.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A signal that stops Board mid-sync removes the board lock and git's
    // index lock that gix holds, so the next write is not refused.
    board_plugin::clean_up_on_signals(gix::tempfile::registry::cleanup_tempfiles);
    let left = deadline.left();
    let mut child =
        Group::spawn(&mut command).map_err(|source| SyncError::NotStarted { source })?;
    let stdout = drain(child.stdout());
    let stderr = drain(child.stderr());
    let stopped = loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(finished(status, stdout, stderr)),
            Ok(None) if !deadline.left().is_zero() => thread::sleep(POLL),
            Ok(None) => {
                break SyncError::TimedOut {
                    operation,
                    remote: upstream.remote.clone(),
                    left,
                    deadline: deadline.given,
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
    child.stop();
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
