//! The one owner of child processes (STANDARDS.md 7): a plugin run with a
//! deadline and bounded output (docs/plugins.md, Calling a capability).

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// How long Board sleeps between checks on a child whose pipes are closed.
const POLL: Duration = Duration::from_millis(10);

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
/// exits with both closed. Past the deadline or the stdout bound, Board kills
/// the child and stops waiting, even while a grandchild holds a pipe.
#[expect(
    clippy::disallowed_methods,
    reason = "board-plugin owns child processes"
)]
pub(crate) fn run(
    program: &Path,
    arguments: &[String],
    directory: &Path,
    environment: &[(&str, OsString)],
    input: Vec<u8>,
    limits: Limits,
) -> Result<Finished, Stopped> {
    let deadline = Instant::now() + limits.time;
    let mut child = Command::new(program)
        .args(arguments)
        .current_dir(directory)
        .env_clear()
        .envs(environment.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(Stopped::Io)?;
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
    let mut code = None;
    loop {
        let now = Instant::now();
        if now >= deadline {
            kill(&mut child);
            return Err(Stopped::Timeout(stderr));
        }
        let wait = (deadline - now).min(POLL);
        match events.recv_timeout(wait) {
            Ok(Event::Out(bytes)) => {
                stdout.extend_from_slice(&bytes);
                if stdout.len() > limits.stdout {
                    kill(&mut child);
                    return Err(Stopped::TooLarge(stderr));
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
        if code.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => code = Some(status.code()),
                Ok(None) => {}
                Err(error) => {
                    kill(&mut child);
                    return Err(Stopped::Io(error));
                }
            }
        }
        if let (0, Some(code)) = (open, code) {
            return Ok(Finished {
                code,
                stdout,
                stderr,
            });
        }
    }
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

/// Kills the child and reaps it. A grandchild is not Board's to wait for.
fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
