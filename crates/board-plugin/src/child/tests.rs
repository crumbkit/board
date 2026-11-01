//! A process a built-in plugin starts is stopped with everything it
//! started, and sees only the environment it is given.

use std::time::Instant;

use super::*;

/// How long the processes these tests stop would otherwise sleep, in seconds.
const SLEEP: u64 = 6;

fn path() -> Vec<(String, OsString)> {
    vec![("PATH".to_owned(), OsString::from("/usr/bin:/bin"))]
}

fn shell(script: &[String], seconds: f64) -> Process<'_> {
    Process {
        program: "sh",
        arguments: script,
        directory: None,
        input: b"",
        deadline: Duration::from_secs_f64(seconds),
    }
}

fn script(text: &str) -> Vec<String> {
    vec!["-c".to_owned(), text.to_owned()]
}

/// Whether the process `pid` stops within five seconds.
fn gone(pid: i32) -> bool {
    let pid = rustix::process::Pid::from_raw(pid).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while rustix::process::test_kill_process(pid).is_ok() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

#[test]
fn a_process_past_its_deadline_is_stopped_and_one_within_it_exits() {
    let started = Instant::now();
    let sleeper = script(&format!("sleep {SLEEP}; echo late"));
    let stopped = run(&shell(&sleeper, 0.2), &path()).unwrap_err();
    assert!(
        matches!(stopped, ProcessError::Timeout { .. }),
        "{stopped:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(SLEEP / 2));
    let soon = script("echo soon");
    assert_eq!(run(&shell(&soon, 5.0), &path()).unwrap().stdout, b"soon\n");
}

#[test]
fn a_process_left_behind_is_stopped_when_its_parent_exits() {
    let leaver = script(&format!("sleep {SLEEP} >/dev/null 2>&1 & echo $!"));
    let exited = run(&shell(&leaver, 5.0), &path()).unwrap();
    let pid = String::from_utf8(exited.stdout).unwrap();
    assert!(gone(pid.trim().parse().unwrap()));
}

#[test]
fn a_process_reads_its_input_in_an_empty_directory_or_the_one_named() {
    let listing = script("cat; ls -A | wc -l");
    let mut process = shell(&listing, 5.0);
    process.input = b"given\n";
    let exited = run(&process, &path()).unwrap();
    assert_eq!(
        String::from_utf8(exited.stdout)
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["given", "0"]
    );
    let full = tempfile::tempdir().unwrap();
    std::fs::File::create_new(full.path().join("a")).unwrap();
    process.directory = Some(full.path());
    let exited = run(&process, &path()).unwrap();
    assert_eq!(
        String::from_utf8(exited.stdout)
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["given", "1"]
    );
}

#[test]
fn a_process_sees_only_the_environment_it_is_given_and_its_exit_code() {
    let echo = script("echo \"$GIVEN-$HOME\"; echo err >&2; exit 3");
    let mut environment = path();
    environment.push(("GIVEN".to_owned(), OsString::from("yes")));
    let exited = run(&shell(&echo, 5.0), &environment).unwrap();
    assert_eq!(
        (exited.code, &exited.stdout[..], &exited.stderr[..]),
        (Some(3), &b"yes-\n"[..], &b"err\n"[..])
    );
}

#[test]
fn a_program_that_cannot_start_is_not_started_and_one_that_can_runs() {
    let process = Process {
        program: "/nonexistent/program",
        ..shell(&[], 5.0)
    };
    let refused = run(&process, &path()).unwrap_err();
    assert!(
        matches!(refused, ProcessError::NotStarted { .. }),
        "{refused:?}"
    );
    let ran = script("exit 0");
    assert_eq!(run(&shell(&ran, 5.0), &path()).unwrap().code, Some(0));
}
