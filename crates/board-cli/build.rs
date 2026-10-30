//! Sets `BOARD_BUILD`, the suffix the timing log adds to Board's version:
//! `+<commit>` with `.dirty` when the source differs from it, or nothing
//! outside a git checkout (docs/architecture/performance/timing.md).

#![expect(
    clippy::disallowed_methods,
    reason = "the build script asks git which commit it builds; no other code runs at build time"
)]

use std::process::Command;

const SOURCE: [&str; 4] = ["crates", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"];

fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    output.status.success().then(|| text.trim().to_owned())
}

fn main() {
    let commit = git(&["rev-parse", "--short=12", "HEAD"]);
    let status = [
        &["--no-optional-locks", "status", "--porcelain", "--"][..],
        &SOURCE,
    ]
    .concat();
    let dirty = git(&status).is_some_and(|changes| !changes.is_empty());
    let build = match commit {
        Some(commit) if dirty => format!("+{commit}.dirty"),
        Some(commit) => format!("+{commit}"),
        None => String::new(),
    };
    println!("cargo::rustc-env=BOARD_BUILD={build}");
    // A commit appends to HEAD's reflog; an edit changes the source.
    if let Some(log) = git(&[
        "rev-parse",
        "--path-format=absolute",
        "--git-path",
        "logs/HEAD",
    ]) {
        println!("cargo::rerun-if-changed={log}");
    }
    for path in SOURCE {
        println!("cargo::rerun-if-changed=../../{path}");
    }
}
