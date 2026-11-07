//! A plugin run gives its child's exit within its limits, stops the
//! processes its scope names, and stops on signals when asked.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};

use super::*;

/// How long the processes these tests stop would otherwise sleep, in seconds.
const SLEEP: u64 = 6;

/// Runs `script` under `sh` with `limits` in `scope`, in a new empty
/// directory, with only `PATH`.
fn shell(
    script: &str,
    limits: Limits,
    scope: Scope,
) -> (Option<Spawned>, Result<Finished, Stopped>) {
    let empty = tempfile::tempdir().unwrap();
    run(
        Path::new("sh"),
        &["-c".to_owned(), script.to_owned()],
        empty.path(),
        &[("PATH", OsString::from("/usr/bin:/bin"))],
        Vec::new(),
        limits,
        scope,
    )
}

fn within(seconds: f64) -> Limits {
    Limits {
        time: Duration::from_secs_f64(seconds),
        ..Limits::default()
    }
}

fn finished(script: &str, limits: Limits) -> Finished {
    match shell(script, limits, Scope::Group).1 {
        Ok(finished) => finished,
        Err(stopped) => panic!("{stopped:?}"),
    }
}

/// Whether the process `pid` stops within five seconds. A zombie counts as
/// stopped: it holds nothing, and only waits for its parent to reap it.
fn gone(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while running(pid) {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

/// Whether the process `pid` runs, as `ps` reports its state.
#[expect(
    clippy::disallowed_methods,
    reason = "the test asks ps for a process's state, which no safe call gives"
)]
fn running(pid: i32) -> bool {
    let state = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&state.stdout);
    !(state.trim().is_empty() || state.trim_start().starts_with('Z'))
}

/// The numbers `bytes` holds, one for each whitespace-separated word.
fn numbers(bytes: &[u8]) -> Vec<i32> {
    (String::from_utf8_lossy(bytes).split_whitespace())
        .map(|word| word.parse().unwrap())
        .collect()
}

#[test]
fn a_plugin_that_exits_with_a_code_gives_it_and_one_a_signal_ends_gives_none() {
    assert_eq!(finished("exit 3", within(5.0)).code, Some(3));
    assert_eq!(finished("kill -KILL $$", within(5.0)).code, None);
}

#[test]
fn stdout_of_exactly_the_limit_is_admitted_and_one_byte_more_is_too_large() {
    let limits = Limits {
        stdout: 1024,
        ..within(5.0)
    };
    let admitted = finished("head -c 1024 /dev/zero", limits);
    assert_eq!(admitted.stdout.len(), 1024);
    let refused = shell("head -c 1025 /dev/zero", limits, Scope::Group).1;
    assert!(
        matches!(&refused, Err(Stopped::TooLarge(stderr)) if stderr.is_empty()),
        "{refused:?}"
    );
}

#[test]
fn a_late_child_times_out_with_its_stderr_tail_and_one_that_exits_in_time_is_admitted() {
    let script = "printf abcdefgh >&2; sleep 1";
    let short = Limits {
        stderr: 4,
        ..within(0.2)
    };
    let refused = shell(script, short, Scope::Group).1;
    assert!(
        matches!(&refused, Err(Stopped::Timeout(stderr)) if stderr == b"efgh"),
        "{refused:?}"
    );
    let admitted = finished(
        script,
        Limits {
            stderr: 4,
            ..within(5.0)
        },
    );
    assert_eq!(
        (admitted.code, &admitted.stderr[..]),
        (Some(0), &b"efgh"[..])
    );
}

#[test]
fn a_missing_program_is_not_started_and_gives_no_spawn_and_a_present_one_does() {
    let empty = tempfile::tempdir().unwrap();
    let start = |program: &str| {
        run(
            Path::new(program),
            &["-c".to_owned(), "exit 0".to_owned()],
            empty.path(),
            &[],
            Vec::new(),
            within(5.0),
            Scope::Group,
        )
    };
    let (spawned, refused) = start("/nonexistent/program");
    assert!(spawned.is_none(), "{spawned:?}");
    assert!(
        matches!(&refused, Err(Stopped::Io(error)) if error.kind() == io::ErrorKind::NotFound),
        "{refused:?}"
    );
    let (spawned, admitted) = start("/bin/sh");
    assert!(spawned.is_some());
    assert_eq!(admitted.unwrap().code, Some(0));
}

#[test]
fn a_spawn_records_when_the_child_first_wrote_stdout_and_none_when_it_wrote_nothing() {
    let (spawned, _) = shell("echo reply", within(5.0), Scope::Group);
    assert!(spawned.unwrap().replied.is_some());
    let (spawned, _) = shell("echo nothing >&2", within(5.0), Scope::Group);
    assert_eq!(spawned.unwrap().replied, None);
}

#[test]
fn a_process_left_in_a_plugins_group_is_stopped_when_the_plugin_exits() {
    let left = finished(
        &format!("sleep {SLEEP} >/dev/null 2>&1 & echo $!"),
        within(5.0),
    );
    assert_eq!(left.code, Some(0));
    assert!(
        gone(numbers(&left.stdout)[0]),
        "the sleep outlived the plugin"
    );
}

#[test]
fn a_process_that_left_the_group_is_not_stopped_and_its_pipe_holds_the_run_about_a_second() {
    // perl leaves the group with setsid, keeps the inherited stdout open,
    // and makes a file once it has left, which the plugin waits for.
    let script = format!(
        "perl -MPOSIX -e '$| = 1; POSIX::setsid(); print qq($$\\n); open(my $f, q(>left)); sleep {SLEEP}' & \
         until [ -e left ]; do sleep 0.05; done"
    );
    let started = Instant::now();
    let left = finished(&script, within(6.0));
    let took = started.elapsed();
    let pid = numbers(&left.stdout)[0];
    let alive = rustix::process::test_kill_process(Pid::from_raw(pid).unwrap()).is_ok();
    // The process that left the group is not Board's to stop; the test stops it.
    let _ = kill_process(Pid::from_raw(pid).unwrap(), Signal::KILL);
    assert!(alive, "Board stopped a process that left the group");
    assert!(took < Duration::from_secs(3), "Board drained for {took:?}");
    assert_eq!(left.code, Some(0));
}

#[test]
fn a_plugin_run_in_a_group_leads_its_own_and_one_run_as_a_process_stays_in_boards() {
    let script = "echo $$; perl -e 'print getpgrp()'";
    let grouped = finished(script, within(5.0));
    let [shell_pid, group] = numbers(&grouped.stdout)[..] else {
        panic!("{grouped:?}")
    };
    assert_eq!(group, shell_pid);
    let (_, alone) = shell(script, within(5.0), Scope::Process);
    let alone = alone.unwrap();
    let [shell_pid, group] = numbers(&alone.stdout)[..] else {
        panic!("{alone:?}")
    };
    assert_ne!(group, shell_pid);
    assert_eq!(group, rustix::process::getpgrp().as_raw_nonzero().get());
}

/// Set in the child this test starts, which stops on signals and then
/// never stops, as a command stuck before it checks the flag would.
const CHILD: &str = "BOARD_PLUGIN_STUCK_CHILD";

#[expect(
    clippy::disallowed_methods,
    reason = "the child writes to stdout, the only way to show the test what a signal did"
)]
fn stuck_child() {
    let stop = stop_on_signals().unwrap();
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
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts itself as a child process that writes to stdout, the only way to show what a signal does to a process"
)]
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
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("{signal:?}: the second signal did not exit the process");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(1), "{signal:?}: {status}");
    }
}
