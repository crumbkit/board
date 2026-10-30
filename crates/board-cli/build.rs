//! Sets `BOARD_BUILD`, the suffix the timing log adds to Board's version:
//! `+<commit>`, with `.dirty` when the source differs from it, or nothing
//! outside a git checkout (docs/architecture/performance/timing.md).
#![expect(
    clippy::disallowed_methods,
    reason = "the build script asks git which commit it builds; no other code runs at build time"
)]

const SOURCE: [&str; 4] = ["crates", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"];

fn git(arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    let status = [
        &["--no-optional-locks", "status", "--porcelain", "--"][..],
        &SOURCE,
    ]
    .concat();
    let dirty = if git(&status).is_some_and(|changes| !changes.is_empty()) {
        ".dirty"
    } else {
        ""
    };
    let build = git(&["rev-parse", "--short=12", "HEAD"]).map(|commit| format!("+{commit}{dirty}"));
    println!("cargo::rustc-env=BOARD_BUILD={}", build.unwrap_or_default());
    // A commit appends to HEAD's reflog; an edit changes the source.
    let log = git(&[
        "rev-parse",
        "--path-format=absolute",
        "--git-path",
        "logs/HEAD",
    ]);
    for path in log
        .into_iter()
        .chain(SOURCE.map(|path| format!("../../{path}")))
    {
        println!("cargo::rerun-if-changed={path}");
    }
}
