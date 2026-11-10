//! The one owner of child processes and of signal handling (STANDARDS.md 7):
//! a plugin run with a deadline and bounded output (docs/plugins.md, Calling
//! a capability), through crumbkit-process.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crumbkit_process::{
    Capture, Escalation, Input, Outcome, OverLimit, RunError, RunOptions, Signal, Status,
    StopReason, Zeroizing,
};

/// How long a child has to exit after SIGTERM before Board kills it.
const GRACE: Duration = Duration::from_secs(1);

/// Runs `cleanup` when Board gets SIGINT, SIGTERM or SIGHUP while a child
/// leads a group of its own, after the groups are stopped and before Board
/// stops as the signal would have. Each function is kept once. It starts
/// the thread that forwards those signals, if it has not started, and fails
/// when the thread cannot start.
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "board-plugin's process module owns signal handling"
)]
pub fn clean_up_on_signals(cleanup: fn()) -> io::Result<()> {
    crumbkit_process::forward::clean_up(cleanup);
    forward_signals()
}

/// Starts the thread that forwards SIGINT, SIGTERM and SIGHUP to every
/// child group, if it has not started.
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "board-plugin's process module owns signal handling"
)]
fn forward_signals() -> io::Result<()> {
    crumbkit_process::forward::install()
}

/// A flag that SIGINT, SIGTERM and SIGHUP set, for a long-running command
/// that stops on them and cleans up instead of dying at once. A second
/// signal, once the flag is set, exits the process at once with status 1,
/// so a command stuck before it checks the flag can still be stopped.
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "board-plugin's process module owns signal handling"
)]
pub fn stop_on_signals() -> io::Result<std::sync::Arc<std::sync::atomic::AtomicBool>> {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    for signal in [SIGINT, SIGTERM, SIGHUP] {
        // Registered first, so it runs before the flag is set and the first
        // signal never exits.
        signal_hook::flag::register_conditional_shutdown(signal, 1, std::sync::Arc::clone(&stop))?;
        signal_hook::flag::register(signal, std::sync::Arc::clone(&stop))?;
    }
    Ok(stop)
}

/// What one run of a plugin may take.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub time: Duration,
    /// The most stdout Board reads.
    pub stdout: usize,
    /// How much of the end of stderr Board keeps.
    pub stderr: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            time: Duration::from_secs(30),
            stdout: 16 * 1024 * 1024,
            stderr: 64 * 1024,
        }
    }
}

/// Which processes Board signals when a plugin passes a limit.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Scope {
    /// The plugin leads a new process group, and Board signals the group, so
    /// every process the plugin started stops with it.
    Group,
    /// The plugin stays in Board's process group, and Board signals only the
    /// plugin. A nested Board uses this, so its plugin stays in the group
    /// that the outermost Board signals.
    Process,
}

/// One plugin process Board started: when, and how long it took to write
/// the first byte of its reply, if it wrote one.
#[derive(Debug)]
pub struct Spawned {
    pub at: Instant,
    pub replied: Option<Duration>,
}

/// A child that exited, with what Board kept of its output.
#[derive(Debug)]
pub(crate) struct Finished {
    /// The exit code; `None` after a signal.
    pub(crate) code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

/// A child Board could not start, or stopped. Each keeps the stderr read so
/// far.
#[derive(Debug)]
pub(crate) enum Stopped {
    Io(io::Error),
    Timeout(Vec<u8>),
    TooLarge(Vec<u8>),
}

/// Runs `program` with `arguments` in `directory` with only `environment`,
/// writes `input` to its stdin, and reads its stdout and stderr until it
/// exits. Past the deadline or the stdout bound, Board sends the processes
/// `scope` names SIGTERM, then SIGKILL up to [`GRACE`] later. With
/// `Scope::Group`, every process still in the child's group is killed when
/// the child exits, and Board forwards SIGINT, SIGTERM and SIGHUP to the
/// group: each group gets the signal and SIGCONT, then SIGKILL once every
/// leader has exited or a second later. A process that left the group, as
/// `setsid` does, is not killed, and a pipe it holds keeps the run for at
/// most a second after the child exits, which can end the run up to a
/// second past its deadline. Every `Scope::Group` caller gets this, the
/// signer and verifier that the sign module runs included. Beside the
/// result it returns the child it started, if it started one.
pub(crate) fn run(
    program: &Path,
    arguments: &[String],
    directory: &Path,
    environment: &[(&str, OsString)],
    input: Vec<u8>,
    limits: Limits,
    scope: Scope,
) -> (Option<Spawned>, Result<Finished, Stopped>) {
    #[cfg(unix)]
    if let Scope::Group = scope
        && let Err(error) = forward_signals()
    {
        return (None, Err(Stopped::Io(error)));
    }
    let at = Instant::now();
    let settings = RunOptions::new()
        .stdin(Input::Bytes(Zeroizing::new(input)))
        .stdout(Capture::Whole {
            limit: limits.stdout,
            over: OverLimit::Stop,
        })
        .stderr(Capture::Tail {
            limit: limits.stderr,
        })
        .deadline(at + limits.time)
        .escalation(Escalation::default().step(Signal::Terminate, GRACE))
        .scope(match scope {
            Scope::Group => crumbkit_process::Scope::Group,
            Scope::Process => crumbkit_process::Scope::Child,
        });
    let command = command(program, arguments, directory, environment);
    let finished = match crumbkit_process::run(command, &settings) {
        Ok(finished) => finished,
        Err(RunError::Start(error)) => return (None, Err(Stopped::Io(error))),
        Err(error) => {
            let error = match error {
                RunError::Run(error) => error,
                other => io::Error::other(other),
            };
            let spawned = Spawned { at, replied: None };
            return (Some(spawned), Err(Stopped::Io(error)));
        }
    };
    let spawned = Spawned {
        at,
        replied: finished.first_stdout(),
    };
    let outcome = finished.outcome();
    // A child can exit before the reader sees its stdout pass the bound, so
    // the kept stdout decides, not only the exit.
    let too_large = finished.stdout().over_limit();
    let (stdout, stderr) = finished.into_output();
    let (stdout, stderr) = (stdout.into_bytes(), stderr.into_bytes());
    let result = match outcome {
        Outcome::Stopped(StopReason::OverLimit) => Err(Stopped::TooLarge(stderr)),
        _ if too_large => Err(Stopped::TooLarge(stderr)),
        Outcome::Ended(Status::Code(code)) => Ok(Finished {
            code: Some(code),
            stdout,
            stderr,
        }),
        Outcome::Ended(Status::Signal(_)) => Ok(Finished {
            code: None,
            stdout,
            stderr,
        }),
        Outcome::TimedOut => Err(Stopped::Timeout(stderr)),
        // Board gives the run no cancel, so only a later crumbkit-process
        // can end it another way.
        _ => Err(Stopped::Io(io::Error::other(
            "the child ended in a way board does not know",
        ))),
    };
    (Some(spawned), result)
}

/// Runs as [`run`] does with `Scope::Group`, so a process left in the group
/// or a pipe held after the child exits can keep it up to two seconds past
/// the child's end: one second while the group empties and one second while
/// the pipes drain.
pub(crate) fn run_alone(
    program: &Path,
    arguments: &[String],
    directory: &Path,
    environment: &[(&str, OsString)],
    input: Vec<u8>,
    limits: Limits,
) -> (Option<Spawned>, Result<Finished, Stopped>) {
    run(
        program,
        arguments,
        directory,
        environment,
        input,
        limits,
        Scope::Group,
    )
}

/// The command that starts `program` with `arguments` in `directory` with
/// only `environment`.
#[expect(
    clippy::disallowed_methods,
    reason = "board-plugin owns child processes"
)]
fn command(
    program: &Path,
    arguments: &[String],
    directory: &Path,
    environment: &[(&str, OsString)],
) -> Command {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(directory)
        .env_clear()
        .envs(environment.iter().map(|(key, value)| (key, value)));
    command
}

#[cfg(test)]
#[cfg(unix)]
mod tests;
