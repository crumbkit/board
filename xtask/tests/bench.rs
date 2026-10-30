//! `xtask bench` as the timing job runs it.

#![expect(
    clippy::disallowed_methods,
    reason = "the bench reads its own PATH, so the test runs it as a child process"
)]

use std::process::Command;

#[test]
fn bench_times_a_generated_board_and_skips_the_plugin_call_without_uv() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bench", "--records", "20"])
        .env("PATH", "")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{stdout}");
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.starts_with("pass"))
            .count(),
        7,
        "{stdout}"
    );
    assert!(
        stdout.contains("skip  uv is not on PATH  `board call tasks.available --repo /`, warm\n"),
        "{stdout}"
    );
}

#[test]
fn bench_against_the_board_beside_xtask_finds_every_row_the_same_in_too_few_rounds_to_decide() {
    let xtask = std::path::Path::new(env!("CARGO_BIN_EXE_xtask"));
    let board = xtask.with_file_name("board");
    let output = Command::new(xtask)
        .args(["bench", "--records", "20", "--runs", "3", "--against"])
        .arg(&board)
        .env("PATH", "")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{stdout}");
    assert_eq!(stdout.lines().count(), 6, "{stdout}");
    for row in stdout.lines() {
        assert!(row.starts_with("same  "), "{stdout}");
    }
}
