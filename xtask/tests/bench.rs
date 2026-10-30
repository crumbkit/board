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
fn bench_against_the_board_beside_xtask_gives_every_row_a_verdict() {
    let xtask = std::path::Path::new(env!("CARGO_BIN_EXE_xtask"));
    let board = xtask.with_file_name("board");
    let output = Command::new(xtask)
        .args(["bench", "--records", "20", "--runs", "3", "--against"])
        .arg(&board)
        .env("PATH", "")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(matches!(output.status.code(), Some(0 | 1)), "{stdout}");
    let rows: Vec<&str> = stdout.lines().skip(2).collect();
    assert_eq!(rows.len(), 6, "{stdout}");
    for row in rows {
        let verdict = row.split_whitespace().next().unwrap();
        assert!(["same", "SLOWER", "faster"].contains(&verdict), "{stdout}");
    }
    assert!(stdout.starts_with("base  0.0.0"), "{stdout}");
}

#[test]
fn timing_prints_the_median_phases_of_the_last_runs_of_a_command() {
    let cache = tempfile::tempdir().unwrap();
    std::fs::create_dir(cache.path().join("board")).unwrap();
    let line = r#"{"command": "get", "depth": 0, "version": "1", "total_us": 2000, "phases": [{"name": "open", "us": 1000}]}"#;
    std::fs::write(
        cache.path().join("board/timings.jsonl"),
        format!("{line}\n"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["timing", "get"])
        .env("XDG_CACHE_HOME", cache.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "get: 1 runs of 1\ntotal           2.0 ms\nopen            1.0 ms  1 runs\n"
    );
}
