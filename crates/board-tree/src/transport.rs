//! The one module that runs the `git` executable: the fetch and push of
//! `board sync` (docs/architecture.md, Writes). git carries credentials,
//! SSH and hosts as the owner set them up; gix reads what it brings.
#![expect(
    clippy::disallowed_methods,
    reason = "board-tree's transport module owns the git child process"
)]

use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Output};
use std::time::{Duration, Instant};

use crumbkit_process::{Capture, Escalation, Exit, OverLimit, Run, RunError, Signal};

use crate::git::Repository;
use crate::write::sync::SyncError;

/// How long `board sync` may run before Board stops git. It starts before
/// the wait for the board lock and covers every fetch, push and replay.
pub const GIT_DEADLINE: Duration = Duration::from_mins(1);

/// How long git has to exit after SIGTERM, such as to remove its lock
/// files, before Board kills its group.
const GRACE: Duration = Duration::from_secs(1);
/// The most stdout Board keeps of one git run.
const STDOUT: usize = 16 * 1024 * 1024;
/// How much of the end of git's stderr Board keeps.
const STDERR: usize = 64 * 1024;

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
/// Git leads a process group of its own through crumbkit-process, which
/// stops the whole group, ssh and what it started included, when Board gets
/// SIGINT, SIGTERM or SIGHUP, and kills what is left of it when git exits.
/// When `deadline` passes with git still running, the group gets SIGTERM,
/// then SIGKILL up to a second later, and the run is
/// [`SyncError::TimedOut`], so a hung remote holds the board lock no longer
/// than the sync's deadline. A helper that left the group and holds a pipe
/// holds the run for at most a second after git exits. Neither git nor ssh
/// may ask anything: git gets `GIT_TERMINAL_PROMPT=0` and `ssh` as its
/// `GIT_SSH_COMMAND`.
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
        .env("GIT_SSH_COMMAND", &ssh.0);
    // A signal that stops Board mid-sync removes the board lock and git's
    // index lock that gix holds, so the next write is not refused.
    board_plugin::clean_up_on_signals(gix::tempfile::registry::cleanup_tempfiles)
        .map_err(|source| SyncError::NotStarted { source })?;
    let left = deadline.left();
    let settings = Run::new()
        .stdout(Capture::Whole {
            limit: STDOUT,
            over: OverLimit::Stop,
        })
        .stderr(Capture::Tail { limit: STDERR })
        .deadline(Instant::now() + left)
        .escalation(Escalation::default().step(Signal::Terminate, GRACE));
    let failure = |status: String, stderr: Vec<u8>| SyncError::Failed {
        operation,
        remote: upstream.remote.clone(),
        status,
        stderr,
    };
    let finished = match crumbkit_process::run(command, &settings) {
        Ok(finished) => finished,
        Err(RunError::Start(source)) => return Err(SyncError::NotStarted { source }),
        Err(error) => {
            let reason = match &error {
                RunError::Run(source) => source.to_string(),
                other => other.to_string(),
            };
            let status = format!("its status could not be read: {reason}");
            return Err(failure(status, Vec::new()));
        }
    };
    let exit = finished.exit();
    // git can exit before the reader sees its stdout pass the bound, so the
    // kept stdout decides, not only the exit.
    let whole = !finished.stdout().over_limit();
    let (stdout, stderr) = finished.into_output();
    let (stdout, stderr) = (stdout.into_bytes(), stderr.into_bytes());
    // The raw wait status, so the status reads as git's exit status prints.
    let status = match exit {
        Exit::Code(code) if whole => ExitStatus::from_raw(code << 8),
        Exit::Signal(signal) if whole => ExitStatus::from_raw(signal),
        Exit::TimedOut => {
            return Err(SyncError::TimedOut {
                operation,
                remote: upstream.remote.clone(),
                left,
                deadline: deadline.given,
            });
        }
        // Stopped over the stdout bound; Board gives the run no cancel.
        _ => {
            return Err(failure(
                format!("it wrote more than {STDOUT} bytes to stdout and was stopped"),
                stderr,
            ));
        }
    };
    Ok(Output {
        status,
        stdout,
        stderr,
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
