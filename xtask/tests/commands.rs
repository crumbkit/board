//! Which command `xtask` runs for its arguments.

#![expect(
    clippy::disallowed_methods,
    reason = "the tests run xtask as a child process"
)]

use std::process::{Command, Output};

/// `xtask` with `arguments` and an empty `PATH`.
fn xtask(arguments: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(arguments)
        .env("PATH", "")
        .output()
}

#[test]
fn lint_with_an_extra_argument_prints_the_usage_and_lint_alone_lints() {
    let extra = xtask(&["lint", "extra"]).unwrap();
    let stderr = String::from_utf8(extra.stderr).unwrap();
    assert_eq!(extra.status.code(), Some(2), "{stderr}");
    assert!(stderr.starts_with("usage: cargo xtask lint\n"), "{stderr}");
    let lint = xtask(&["lint"]).unwrap();
    let stdout = String::from_utf8(lint.stdout).unwrap();
    assert_eq!(lint.status.code(), Some(0), "{stdout}");
    assert!(!stdout.is_empty());
}

#[test]
fn profile_runs_and_is_refused_without_samply_on_path() {
    let output = xtask(&["profile", "--", "check"]).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.starts_with("error: xtask::profile::no_samply: "),
        "{stderr}"
    );
}
