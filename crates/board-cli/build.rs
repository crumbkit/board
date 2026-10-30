//! Sets `BOARD_BUILD`, the suffix the timing log adds to Board's version:
//! `+<commit>`, with `.dirty.<hash>` when the source differs from it, or
//! nothing outside a git checkout, so that a build there keeps no decisions
//! (docs/architecture/performance/timing.md).
#![expect(
    clippy::disallowed_methods,
    reason = "the build script asks git which commit it builds and how the source differs from it; no other code runs at build time"
)]

use std::ffi::OsStr;
use std::hash::{DefaultHasher, Hasher};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const SOURCE: [&str; 4] = ["crates", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"];
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn git(arguments: &[&str]) -> Option<Vec<u8>> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .current_dir(ROOT)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(output.stdout)
}

fn text(arguments: &[&str]) -> Option<String> {
    git(arguments).map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
}

/// A hash of how the source differs from `HEAD`: the diff of tracked files,
/// which no diff driver or text conversion changes, and the name and bytes
/// of each untracked file, so two edits never share a build.
fn changes() -> String {
    let mut hasher = DefaultHasher::new();
    let diff = [
        "diff-index",
        "-p",
        "--binary",
        "--no-ext-diff",
        "--no-textconv",
        "HEAD",
        "--",
    ];
    hasher.write(&git(&[&diff[..], &SOURCE[..]].concat()).unwrap_or_default());
    let untracked = ["ls-files", "-z", "--others", "--exclude-standard", "--"];
    let names = git(&[&untracked[..], &SOURCE[..]].concat()).unwrap_or_default();
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        hasher.write(name);
        hasher.write(
            &std::fs::read(Path::new(ROOT).join(OsStr::from_bytes(name))).unwrap_or_default(),
        );
    }
    format!("{:016x}", hasher.finish())
}

fn main() {
    let status = [
        &["--no-optional-locks", "status", "--porcelain", "--"],
        &SOURCE[..],
    ]
    .concat();
    let dirty = text(&status).is_some_and(|changes| !changes.is_empty());
    let build = match text(&["rev-parse", "--short=12", "HEAD"]) {
        Some(commit) if dirty => format!("+{commit}.dirty.{}", changes()),
        Some(commit) => format!("+{commit}"),
        None => String::new(),
    };
    println!("cargo::rustc-env=BOARD_BUILD={build}");
    // A commit appends to HEAD's reflog, or moves HEAD where git keeps no
    // reflog; an edit changes the source.
    let path = |name| text(&["rev-parse", "--path-format=absolute", "--git-path", name]);
    let head = (path("logs/HEAD").filter(|log| Path::new(log).exists())).or_else(|| path("HEAD"));
    for watched in head
        .into_iter()
        .chain(SOURCE.map(|path| format!("../../{path}")))
    {
        println!("cargo::rerun-if-changed={watched}");
    }
}
