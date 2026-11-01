//! The one owner of child processes and of signal handling (STANDARDS.md 7):
//! a plugin run with a deadline and bounded output (docs/plugins.md, Calling
//! a capability).

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
#[cfg(unix)]
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use rustix::process::{Pid, Signal};

/// How long Board sleeps between checks on a child whose pipes are closed.
const POLL: Duration = Duration::from_millis(10);
/// How long a plugin has to exit after SIGTERM before Board kills it.
const GRACE: Duration = Duration::from_secs(1);

/// The process groups of the running plugins that lead their own group.
/// Board forwards SIGINT, SIGTERM and SIGHUP to each of them. The flag
/// marks a group that is swept when its leader exits; the forwarder kills
/// such a group after the signal, because a non-interactive shell's
/// background jobs ignore SIGINT.
#[cfg(unix)]
static GROUPS: Mutex<Vec<(Pid, bool)>> = Mutex::new(Vec::new());

/// Whether the thread that forwards signals started.
#[cfg(unix)]
static FORWARDER: OnceLock<Result<(), io::ErrorKind>> = OnceLock::new();

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

/// A child that exited with its pipes closed.
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

enum Event {
    Out(Vec<u8>),
    Err(Vec<u8>),
    Closed,
}

/// Runs `program` with `arguments` in `directory` with only `environment`,
/// writes `input` to its stdin, and reads its stdout and stderr until it
/// exits with both closed. Past the deadline or the stdout bound, Board stops
/// the processes `scope` names and stops waiting, even while a grandchild
/// holds a pipe. Off Unix, Board ignores `scope` and kills only the child.
/// Beside the result it returns the child it started, if it started one.
pub(crate) fn run(
    program: &Path,
    arguments: &[String],
    directory: &Path,
    environment: &[(&str, OsString)],
    input: Vec<u8>,
    limits: Limits,
    scope: Scope,
) -> (Option<Spawned>, Result<Finished, Stopped>) {
    let command = Running {
        program,
        arguments,
        directory,
        environment,
    };
    supervise(&command, input, limits, scope, false)
}

/// Runs as [`run`] does with `Scope::Group`, and when the child exits,
/// kills every process still in its group before reaping it, even while
/// one of them holds the child's pipes. A process that left the group, as
/// `setsid` does, is not killed; if it holds a pipe, Board drains until the
/// deadline and then gives the child's exit.
pub(crate) fn run_alone(
    program: &Path,
    arguments: &[String],
    directory: &Path,
    environment: &[(&str, OsString)],
    input: Vec<u8>,
    limits: Limits,
) -> (Option<Spawned>, Result<Finished, Stopped>) {
    let command = Running {
        program,
        arguments,
        directory,
        environment,
    };
    supervise(&command, input, limits, Scope::Group, true)
}

/// What to start: a program, its arguments, its directory and its only
/// variables.
struct Running<'a> {
    program: &'a Path,
    arguments: &'a [String],
    directory: &'a Path,
    environment: &'a [(&'a str, OsString)],
}

#[expect(
    clippy::disallowed_methods,
    reason = "board-plugin owns child processes"
)]
fn supervise(
    &Running {
        program,
        arguments,
        directory,
        environment,
    }: &Running<'_>,
    input: Vec<u8>,
    limits: Limits,
    scope: Scope,
    sweep: bool,
) -> (Option<Spawned>, Result<Finished, Stopped>) {
    // Off Unix there are no groups to sweep, and the child is reaped as
    // `run` reaps it.
    let sweep = sweep && cfg!(unix);
    let deadline = Instant::now() + limits.time;
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(directory)
        .env_clear()
        .envs(environment.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let spawned = Instant::now();
    let mut child = match spawn(&mut command, scope, sweep) {
        Ok(child) => child,
        Err(error) => return (None, Err(Stopped::Io(error))),
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A child that exits without reading closes the pipe; that is its
        // answer, not Board's failure.
        thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let (sender, events) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        read(stdout, sender.clone(), Event::Out);
    }
    if let Some(stderr) = child.stderr.take() {
        read(stderr, sender, Event::Err);
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut open = 2;
    let mut replied = None;
    // In sweep mode, whether the child was seen exited and its group killed.
    let mut swept = false;
    let finished = loop {
        let now = Instant::now();
        if now >= deadline {
            if swept {
                // The child exited, but a process that left its group still
                // holds a pipe: Board stops draining and gives the exit.
                break finish(&mut child, scope, stdout, stderr);
            }
            kill(&mut child, scope);
            break Err(Stopped::Timeout(stderr));
        }
        let wait = (deadline - now).min(POLL);
        match events.recv_timeout(wait) {
            Ok(Event::Out(bytes)) => {
                replied.get_or_insert_with(|| spawned.elapsed());
                stdout.extend_from_slice(&bytes);
                if stdout.len() > limits.stdout {
                    kill(&mut child, scope);
                    break Err(Stopped::TooLarge(stderr));
                }
            }
            Ok(Event::Err(bytes)) => {
                stderr.extend_from_slice(&bytes);
                let excess = stderr.len().saturating_sub(limits.stderr);
                stderr.drain(..excess);
            }
            Ok(Event::Closed) => open -= 1,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => thread::sleep(wait),
        }
        // In sweep mode Board looks for the child's exit on every poll,
        // since a process it started may hold its pipes open after it
        // exits. It kills the group while the child is unreaped, so the
        // child's pid still names the group, and reaps it only after both
        // pipes close. Killing the group closes every pipe its members
        // held. Soft limit: what is left gets SIGKILL with no SIGTERM
        // first, since waiting a grace period would delay every run.
        #[cfg(unix)]
        if sweep && !swept && exited(&child) {
            let _ = rustix::process::kill_process_group(Pid::from_child(&child), Signal::KILL);
            swept = true;
        }
        #[cfg(not(unix))]
        let _ = sweep;
        // Board reaps the child only after both pipes close, so until then
        // its pid, and the group it leads, name no other process.
        if open == 0 && (swept || !sweep) {
            match reap(&mut child) {
                Ok(Some(status)) => {
                    break Ok(Finished {
                        code: status.code(),
                        stdout,
                        stderr,
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    kill(&mut child, scope);
                    break Err(Stopped::Io(error));
                }
            }
        }
    };
    let spawned = Spawned {
        at: spawned,
        replied,
    };
    (Some(spawned), finished)
}

/// Reaps a child already seen exited and gives its exit with what was read.
fn finish(
    child: &mut Child,
    scope: Scope,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
) -> Result<Finished, Stopped> {
    match reap(child) {
        Ok(Some(status)) => Ok(Finished {
            code: status.code(),
            stdout,
            stderr,
        }),
        Ok(None) => {
            kill(child, scope);
            Err(Stopped::Timeout(stderr))
        }
        Err(error) => {
            kill(child, scope);
            Err(Stopped::Io(error))
        }
    }
}

/// Starts `command`. With `Scope::Group`, the child leads a new process
/// group, and Board forwards the signals that stop it to that group.
#[cfg(unix)]
fn spawn(command: &mut Command, scope: Scope, sweep: bool) -> io::Result<Child> {
    if let Scope::Process = scope {
        return command.spawn();
    }
    std::os::unix::process::CommandExt::process_group(command, 0);
    (*FORWARDER.get_or_init(forward)).map_err(io::Error::from)?;
    // Board holds the lock from the spawn until the group is listed, so a
    // signal that arrives in between still reaches the group.
    let mut groups = groups();
    let child = command.spawn()?;
    groups.push((Pid::from_child(&child), sweep));
    Ok(child)
}

#[cfg(not(unix))]
fn spawn(command: &mut Command, _: Scope, _: bool) -> io::Result<Child> {
    command.spawn()
}

/// Starts the thread that forwards SIGINT, SIGTERM and SIGHUP to every
/// listed group and then stops Board as the signal would have. The thread
/// stays for the life of the process, because signal-hook does not restore
/// the default action when a handler is removed: SIGINT would be ignored.
/// The handler replaces an inherited ignored SIGINT or SIGHUP, as under
/// `nohup`; no safe API reads the inherited disposition to respect it.
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "board-plugin's process module owns signal handling"
)]
fn forward() -> Result<(), io::ErrorKind> {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let mut signals = signal_hook::iterator::Signals::new([SIGINT, SIGTERM, SIGHUP])
        .map_err(|error| error.kind())?;
    thread::spawn(move || {
        for raw in signals.forever() {
            // The lock stays held until Board stops, so no plugin starts
            // after the signal.
            let groups = groups();
            if let Some(signal) = Signal::from_named_raw(raw) {
                for (group, _) in groups.iter() {
                    let _ = rustix::process::kill_process_group(*group, signal);
                }
                // A swept group gets SIGKILL too, so nothing it started
                // outlives Board, whatever the signal's disposition in it.
                for (group, _) in groups.iter().filter(|(_, sweep)| *sweep) {
                    let _ = rustix::process::kill_process_group(*group, Signal::KILL);
                }
            }
            // For SIGINT, SIGTERM and SIGHUP this call does not return: it
            // restores the default action and raises the signal, and falls
            // back to abort(). So the lock held here is never released to a
            // plugin that starts later.
            let _ = signal_hook::low_level::emulate_default_handler(raw);
        }
    });
    Ok(())
}

#[cfg(unix)]
fn groups() -> MutexGuard<'static, Vec<(Pid, bool)>> {
    // Each change to the list is a single push or retain, so a panic while
    // the lock was held cannot leave it half changed.
    GROUPS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Stops forwarding signals to the group `child` leads. The caller has not
/// reaped `child` yet, so the forwarder never signals a reused id.
#[cfg(unix)]
fn unlist(groups: &mut Vec<(Pid, bool)>, child: &Child) {
    let pid = Pid::from_child(child);
    groups.retain(|(group, _)| *group != pid);
}

/// Whether `child` has exited, leaving it unreaped.
#[cfg(unix)]
fn exited(child: &Child) -> bool {
    use rustix::process::{WaitId, WaitIdOptions, waitid};
    let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
    matches!(
        waitid(WaitId::Pid(Pid::from_child(child)), options),
        Ok(Some(_))
    )
}

/// Reaps `child` if it has exited. On Unix, Board unlists its group under
/// the same lock, so the forwarder never sees a reaped child listed.
fn reap(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    #[cfg(unix)]
    let mut groups = groups();
    let status = child.try_wait()?;
    #[cfg(unix)]
    if status.is_some() {
        unlist(&mut groups, child);
    }
    Ok(status)
}

/// Sends what `pipe` yields, wrapped by `event`, until it closes.
fn read(mut pipe: impl Read + Send + 'static, sender: Sender<Event>, event: fn(Vec<u8>) -> Event) {
    thread::spawn(move || {
        let mut buffer = vec![0; 64 * 1024];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if sender.send(event(buffer[..read].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = sender.send(Event::Closed);
    });
}

/// Sends SIGTERM to the processes `scope` names, gives the child `GRACE` to
/// exit, then sends them SIGKILL, stops forwarding signals to the group, and
/// reaps the child. The caller has not reaped the child, so its pid still
/// names it and its group. Off Unix, the child is killed at once.
fn kill(child: &mut Child, scope: Scope) {
    #[cfg(unix)]
    {
        use rustix::process::{WaitId, WaitIdOptions, waitid};
        let pid = Pid::from_child(child);
        let signal = |signal| match scope {
            Scope::Group => rustix::process::kill_process_group(pid, signal),
            Scope::Process => rustix::process::kill_process(pid, signal),
        };
        // NOWAIT leaves an exited child unreaped, so its pid still names its
        // own group when SIGKILL is sent.
        let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        let running = || matches!(waitid(WaitId::Pid(pid), options), Ok(None));
        if signal(Signal::TERM).is_ok() {
            let deadline = Instant::now() + GRACE;
            while Instant::now() < deadline && running() {
                thread::sleep(POLL);
            }
        }
        let _ = signal(Signal::KILL);
        unlist(&mut groups(), child);
    }
    #[cfg(not(unix))]
    let _ = scope;
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts itself as a child process that writes to stdout, the only way to show what a signal does to a process"
)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use rustix::process::{Pid, Signal, kill_process};

    /// Set in the child this test starts, which stops on signals and then
    /// never stops, as a command stuck before it checks the flag would.
    const CHILD: &str = "BOARD_PLUGIN_STUCK_CHILD";

    fn stuck_child() {
        let stop = super::stop_on_signals().unwrap();
        let mut out = std::io::stdout().lock();
        writeln!(out, "ready").unwrap();
        out.flush().unwrap();
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(10));
        }
        writeln!(out, "stopping").unwrap();
        out.flush().unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    #[test]
    fn a_first_signal_sets_the_flag_and_a_second_exits_a_process_stuck_before_it_checks_it() {
        if std::env::var_os(CHILD).is_some() {
            stuck_child();
        }
        for signal in [Signal::INT, Signal::TERM, Signal::HUP] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "process::tests::a_first_signal_sets_the_flag_and_a_second_exits_a_process_stuck_before_it_checks_it",
                    "--exact",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let pid = Pid::from_child(&child);
            let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
            let mut wait_for = |wanted: &str| {
                let found = lines
                    .by_ref()
                    .map_while(Result::ok)
                    .any(|line| line == wanted);
                assert!(found, "{signal:?}: the child never printed {wanted}");
            };
            wait_for("ready");
            kill_process(pid, signal).unwrap();
            wait_for("stopping");
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                child.try_wait().unwrap().is_none(),
                "{signal:?}: the first signal exited the process"
            );
            kill_process(pid, signal).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    panic!("{signal:?}: the second signal did not exit the process");
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(status.code(), Some(1), "{signal:?}: {status}");
        }
    }
}
