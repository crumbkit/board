//! A process a built-in plugin starts is stopped with everything still in
//! its group, and sees only the environment it is given.

use std::process::{Command, Stdio};
use std::time::Instant;

use rustix::process::{Pid, Signal, kill_process};

use super::*;

/// How long the processes these tests stop would otherwise sleep, in seconds.
const SLEEP: u64 = 6;

fn path() -> Vec<(String, OsString)> {
    vec![("PATH".to_owned(), OsString::from("/usr/bin:/bin"))]
}

fn shell<'a>(script: &'a [String], directory: &'a Path, seconds: f64) -> Process<'a> {
    Process {
        program: "sh",
        arguments: script,
        directory,
        input: b"",
        deadline: Duration::from_secs_f64(seconds),
    }
}

fn script(text: &str) -> Vec<String> {
    vec!["-c".to_owned(), text.to_owned()]
}

/// Whether the process `pid` stops within five seconds.
fn gone(pid: i32) -> bool {
    let pid = Pid::from_raw(pid).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while rustix::process::test_kill_process(pid).is_ok() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

fn pid(bytes: &[u8]) -> i32 {
    String::from_utf8_lossy(bytes).trim().parse().unwrap()
}

#[test]
fn a_process_past_its_deadline_is_stopped_with_what_it_started_and_one_within_it_exits() {
    let empty = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let sleeper = script(&format!("sleep {SLEEP} & echo $! >&2; wait"));
    let stopped = run(&shell(&sleeper, empty.path(), 0.5), &path()).unwrap_err();
    let ProcessError::Timeout { stderr } = stopped else {
        panic!("{stopped:?}")
    };
    assert!(started.elapsed() < Duration::from_secs(SLEEP / 2));
    assert!(gone(pid(&stderr)), "the sleep it started outlived it");
    let soon = script("echo soon");
    let exited = run(&shell(&soon, empty.path(), 5.0), &path()).unwrap();
    assert_eq!(exited.stdout, b"soon\n");
}

#[test]
fn a_process_left_behind_is_stopped_when_its_parent_exits() {
    let empty = tempfile::tempdir().unwrap();
    let leaver = script(&format!("sleep {SLEEP} >/dev/null 2>&1 & echo $!"));
    let exited = run(&shell(&leaver, empty.path(), 5.0), &path()).unwrap();
    assert!(gone(pid(&exited.stdout)));
}

#[test]
fn a_process_whose_child_holds_its_output_exits_when_it_exits_and_the_child_is_stopped() {
    let empty = tempfile::tempdir().unwrap();
    let holder = script(&format!("sleep {SLEEP} & echo $! >&2; echo done"));
    let started = Instant::now();
    let exited = run(&shell(&holder, empty.path(), 3.0), &path()).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "Board waited for the deadline"
    );
    assert_eq!((exited.code, &exited.stdout[..]), (Some(0), &b"done\n"[..]));
    assert!(
        gone(pid(&exited.stderr)),
        "the sleep holding the output outlived it"
    );
}

#[test]
fn a_process_past_the_stdout_bound_is_stopped_and_one_within_it_exits() {
    let empty = tempfile::tempdir().unwrap();
    let writer = script("head -c 4096 /dev/zero; sleep 6");
    let stopped = run_within(&shell(&writer, empty.path(), 5.0), &path(), 1024).unwrap_err();
    assert!(
        matches!(stopped, ProcessError::TooLarge { limit: 1024, .. }),
        "{stopped:?}"
    );
    let small = script("head -c 1024 /dev/zero");
    let exited = run_within(&shell(&small, empty.path(), 5.0), &path(), 1024).unwrap();
    assert_eq!(exited.stdout.len(), 1024);
}

#[test]
fn a_process_reads_its_input_in_the_directory_named() {
    let empty = tempfile::tempdir().unwrap();
    let listing = script("cat; ls -A | wc -l");
    let mut process = shell(&listing, empty.path(), 5.0);
    process.input = b"given\n";
    let words = |exited: Exited| -> Vec<String> {
        let text = String::from_utf8(exited.stdout).unwrap();
        text.split_whitespace().map(str::to_owned).collect()
    };
    assert_eq!(words(run(&process, &path()).unwrap()), ["given", "0"]);
    std::fs::File::create_new(empty.path().join("a")).unwrap();
    assert_eq!(words(run(&process, &path()).unwrap()), ["given", "1"]);
}

#[test]
fn a_process_sees_only_the_environment_it_is_given_and_its_exit_code() {
    let empty = tempfile::tempdir().unwrap();
    let echo = script("echo \"$GIVEN-$HOME\"; echo err >&2; exit 3");
    let mut environment = path();
    environment.push(("GIVEN".to_owned(), OsString::from("yes")));
    let exited = run(&shell(&echo, empty.path(), 5.0), &environment).unwrap();
    assert_eq!(
        (exited.code, &exited.stdout[..], &exited.stderr[..]),
        (Some(3), &b"yes-\n"[..], &b"err\n"[..])
    );
}

#[test]
fn a_program_that_cannot_start_is_not_started_and_one_that_can_runs() {
    let empty = tempfile::tempdir().unwrap();
    let process = Process {
        program: "/nonexistent/program",
        ..shell(&[], empty.path(), 5.0)
    };
    let refused = run(&process, &path()).unwrap_err();
    assert!(
        matches!(refused, ProcessError::NotStarted { .. }),
        "{refused:?}"
    );
    let ran = script("exit 0");
    assert_eq!(
        run(&shell(&ran, empty.path(), 5.0), &path()).unwrap().code,
        Some(0)
    );
}

/// Set in the copy of this test binary that the signal test starts, to the
/// file where the runner's sleep writes its pid. The runner runs the sleep
/// through `run` and is then stopped by the signal.
const RUNNER: &str = "BOARD_PLUGIN_CHILD_RUNNER";

#[expect(
    clippy::disallowed_methods,
    reason = "the test starts itself as a child process, the only way to show what a signal does to Board"
)]
#[test]
fn a_signal_that_stops_board_stops_every_process_in_the_group_of_one_it_runs() {
    if let Some(file) = std::env::var_os(RUNNER) {
        let empty = tempfile::tempdir().unwrap();
        let file = file.to_string_lossy();
        let sleeper = script(&format!("sleep {SLEEP} & echo $! > '{file}'; wait"));
        let _ = run(&shell(&sleeper, empty.path(), 30.0), &path());
        return;
    }
    for signal in [Signal::INT, Signal::TERM, Signal::HUP] {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("pid");
        let mut board = Command::new(std::env::current_exe().unwrap())
            .args([
                "child::tests::a_signal_that_stops_board_stops_every_process_in_the_group_of_one_it_runs",
                "--exact",
            ])
            .env(RUNNER, &file)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let sleeping = loop {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            if let Ok(pid) = text.trim().parse::<i32>() {
                break pid;
            }
            assert!(
                Instant::now() < deadline,
                "{signal:?}: the runner wrote no pid"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        kill_process(Pid::from_child(&board), signal).unwrap();
        let _ = board.wait();
        assert!(gone(sleeping), "{signal:?}: the sleep outlived Board");
    }
}
