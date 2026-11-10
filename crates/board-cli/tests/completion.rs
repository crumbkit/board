//! `board completion install`, `check` and `print`, run as the built binary
//! with `HOME` and the XDG directories set to a temporary directory of the
//! test's own, so that no test writes into the real home directory.
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "the tests run the built board as a child process"
)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

/// Runs `board` in `home` with a cleared environment, `home` as `HOME` and
/// the XDG directories under it, and `extra` variables added.
fn board(home: &Path, arguments: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board"));
    command
        .args(arguments)
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"));
    for (name, value) in extra {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn completion_install_writes_a_fish_registration_that_sets_board_complete() {
    let home = TempDir::new().unwrap();

    let output = board(
        home.path(),
        &["completion", "install", "--shell", "fish"],
        &[],
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let file = home.path().join("config/fish/completions/board.fish");
    let script = fs::read_to_string(file).unwrap();
    assert!(script.contains("BOARD_COMPLETE=fish"), "{script}");
}

#[test]
fn completion_print_prints_the_registration_without_writing_a_file() {
    let home = TempDir::new().unwrap();

    let output = board(
        home.path(),
        &["completion", "print", "--shell", "fish"],
        &[],
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(stdout(&output).contains("BOARD_COMPLETE=fish"));
    assert!(!home.path().join("config/fish").exists());
}

#[test]
fn completion_check_exits_with_1_for_a_missing_file_and_0_after_install() {
    let home = TempDir::new().unwrap();
    let check = ["completion", "check", "--shell", "fish"];

    let missing = board(home.path(), &check, &[]);
    board(
        home.path(),
        &["completion", "install", "--shell", "fish"],
        &[],
    );
    let current = board(home.path(), &check, &[]);

    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    assert!(stdout(&missing).contains("missing"), "{missing:?}");
    assert_eq!(current.status.code(), Some(0), "{current:?}");
}

#[test]
fn completion_install_reads_the_shell_from_the_shell_variable() {
    let home = TempDir::new().unwrap();

    let output = board(
        home.path(),
        &["completion", "install"],
        &[("SHELL", "/usr/bin/fish")],
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        home.path()
            .join("config/fish/completions/board.fish")
            .exists()
    );
}

#[test]
fn completion_install_without_a_shell_refuses_and_names_the_flag() {
    let home = TempDir::new().unwrap();

    let output = board(home.path(), &["completion", "install"], &[]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--shell"), "{stderr}");
}

#[test]
fn a_completion_request_after_the_commands_exist_still_answers_with_the_board_variable() {
    let home = TempDir::new().unwrap();

    let output = board(home.path(), &[], &[("BOARD_COMPLETE", "fish")]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(stdout(&output).contains("BOARD_COMPLETE=fish"));
}
