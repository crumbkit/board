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

/// `xtask bench --against` a `board` that is `true`, with a `cargo` that
/// builds nothing and a target directory holding `release/board`, also
/// `true`, when `built`. Testing xtask alone, as the slow lane's mutants
/// jobs do, builds no real `board`.
fn against(built: bool) -> std::io::Result<std::process::Output> {
    let board = std::path::Path::new("/usr/bin/true");
    let target = tempfile::tempdir()?;
    if built {
        std::fs::create_dir(target.path().join("release"))?;
        std::os::unix::fs::symlink(board, target.path().join("release/board"))?;
    }
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bench", "--records", "20", "--runs", "3", "--against"])
        .arg(board)
        .env("PATH", "")
        .env("CARGO", "/usr/bin/true")
        .env("CARGO_TARGET_DIR", target.path())
        .output()
}

#[test]
fn bench_against_a_release_build_finds_every_row_the_same_in_too_few_rounds_to_decide() {
    let output = against(true).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{stdout}");
    assert_eq!(stdout.lines().count(), 6, "{stdout}");
    for row in stdout.lines() {
        assert!(row.starts_with("same  "), "{stdout}");
    }
}

#[test]
fn bench_against_is_refused_when_the_release_build_left_no_board() {
    let output = against(false).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!output.status.success());
    assert!(stderr.contains("xtask::bench::no_board"), "{stderr}");
    assert!(output.stdout.is_empty());
}
