//! `xtask bench` as the timing job runs it.

#![expect(
    clippy::disallowed_methods,
    reason = "the bench reads its own PATH, so the test runs it as a child process"
)]

use std::process::Command;

#[test]
fn bench_times_a_generated_board_with_the_built_in_plugin_call_and_no_path() {
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
        8,
        "{stdout}"
    );
    let call =
        (stdout.lines()).find(|line| line.ends_with("`board call tasks.available --repo /`, warm"));
    assert!(
        call.is_some_and(|line| line.starts_with("pass")),
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
    assert_eq!(output.stdout, b"");
}

/// The commands budgets.md's Command budgets table names, in its order.
fn budgeted() -> Vec<String> {
    let docs = include_str!("../../docs/architecture/performance/budgets.md");
    (docs.lines())
        .skip_while(|line| !line.starts_with("| Command | Budget (p90) |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .filter_map(|line| line.split('`').nth(1).map(str::to_owned))
        .collect()
}

#[test]
fn bench_hot_times_each_budgeted_command_on_a_board_with_open_changes_and_prints_a_ci_run() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bench", "--hot", "--records", "20", "--json"])
        .env("PATH", "")
        .env("GITHUB_SHA", "0123456789abcdef0123456789abcdef01234567")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stdout}{stderr}");
    let run: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(run["$type"], "runs/run/v1");
    assert_eq!(run["suite"], "/suites/board-speed-ci");
    assert_eq!(
        (&run["scope"], &run["dirty"]),
        (&"full".into(), &false.into())
    );
    assert_eq!(
        run["versions"]["board"],
        "0123456789abcdef0123456789abcdef01234567"
    );
    let results = run["results"].as_array().unwrap();
    let cases: Vec<&str> = results
        .iter()
        .map(|r| r["case"].as_str().unwrap())
        .collect();
    assert_eq!(cases, budgeted());
    for result in results {
        let outcome = result["outcome"].as_str().unwrap();
        assert!(matches!(outcome, "pass" | "fail"), "{result}");
        assert!(result["measures"]["p90_ms"].is_number(), "{result}");
        assert!(result["measures"]["p50_ms"].is_number(), "{result}");
    }
}

#[test]
fn bench_refuses_json_without_hot() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bench", "--json"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
