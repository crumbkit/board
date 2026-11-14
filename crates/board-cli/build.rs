//! Sets `BOARD_BUILD_DATE`, the commit date of the source in seconds (empty
//! when neither git nor `SOURCE_DATE_EPOCH` gives one, git's commit date first), and `BOARD_BUILD`, the suffix the timing log adds to Board's version:
//! `+<commit>`, with `.dirty.<hash>` when the source differs from it.
//! `BOARD_BUILD_COMMIT` names the commit in place of git's, and asserts that
//! the source is that commit: a copy whose own repository committed the
//! snapshot, or a build with no local git to check for edits, reads as clean
//! and shares decisions with every build that names the same commit. While
//! the variable is unset or empty, a build outside a git checkout has no
//! suffix, so that it keeps no decisions
//! (docs/architecture/performance/timing.md). It also writes `skills.rs`
//! in `OUT_DIR`, which embeds the skills of the plugins this build enables
//! (docs/plugins.md, Skills).
#![expect(
    clippy::disallowed_methods,
    reason = "the build script asks git which commit it builds and how the source differs from it, reads the cargo features it builds with, and writes the skills it embeds into OUT_DIR; no other code runs at build time"
)]

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hasher};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[path = "build/built.rs"]
mod built;
#[path = "build/commit.rs"]
mod commit;

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
    let given = std::env::var_os("BOARD_BUILD_COMMIT");
    let head = || text(&["rev-parse", "--short=12", "HEAD"]);
    let differs = || {
        let dirty = text(&status).is_some_and(|changes| !changes.is_empty());
        dirty.then(changes)
    };
    match commit::suffix(given.as_deref(), head, differs) {
        Ok(build) => println!("cargo::rustc-env=BOARD_BUILD={build}"),
        Err(message) => fail(&message),
    }
    println!("cargo::rerun-if-env-changed=BOARD_BUILD_COMMIT");
    let epoch = std::env::var_os("SOURCE_DATE_EPOCH");
    let log = |commit: Option<&str>| {
        let mut arguments = vec!["log", "-1", "--format=%ct"];
        arguments.extend(commit);
        arguments.push("--");
        text(&arguments)
    };
    match built::seconds(epoch.as_deref(), given.as_deref(), log) {
        Ok(date) => {
            println!(
                "cargo::rustc-env=BOARD_BUILD_DATE={}",
                date.map_or_else(String::new, |date| date.to_string())
            );
            println!(
                "cargo::rustc-env=BOARD_BUILD_SHOWN={}",
                date.map_or_else(String::new, |date| format!(" (built {})", utc(date)))
            );
        }
        Err(message) => fail(&message),
    }
    println!("cargo::rerun-if-env-changed=SOURCE_DATE_EPOCH");
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
    skills();
}

/// Each skill the plugins this build enables ship, as its name and its
/// directory. A plugin's own skill, `plugins/<plugin>/SKILL.md`, is named
/// after the plugin and ships its `SKILL.md` and `README.md`; a skill in
/// `plugins/<plugin>/skills/<skill>/` ships every file in its directory.
fn found(plugins: &Path) -> Vec<(String, PathBuf, Vec<PathBuf>)> {
    let mut found = Vec::new();
    for plugin in sorted(plugins) {
        let Some(name) = plugin.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let feature = format!("CARGO_FEATURE_{}", name.to_uppercase().replace('-', "_"));
        if std::env::var_os(feature).is_none() {
            continue;
        }
        if plugin.join("SKILL.md").is_file() {
            let files = ["SKILL.md", "README.md"].map(|file| plugin.join(file));
            let files = files.into_iter().filter(|file| file.is_file()).collect();
            found.push((name.to_owned(), plugin.clone(), files));
        }
        for skill in sorted(&plugin.join("skills")) {
            if !skill.join("SKILL.md").is_file() {
                continue;
            }
            let name = skill.file_name().unwrap_or_default().to_string_lossy();
            let mut files = Vec::new();
            every_file(&skill, &mut files);
            found.push((name.into_owned(), skill.clone(), files));
        }
    }
    found
}

/// The entries of `directory`, sorted, or none when it cannot be read.
fn sorted(directory: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(directory).into_iter().flatten();
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    paths
}

/// Every file beneath `directory`, in sorted order.
fn every_file(directory: &Path, files: &mut Vec<PathBuf>) {
    for path in sorted(directory) {
        if path.is_dir() {
            every_file(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

/// Writes `skills.rs`: `SKILLS`, each skill's name and each of its files'
/// path within the skill and bytes, in name order.
fn skills() {
    let plugins = Path::new(ROOT).join("plugins");
    println!("cargo::rerun-if-changed={}", plugins.display());
    let mut found = found(&plugins);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    let mut source = String::from("&[\n");
    for (name, directory, files) in found {
        let mut characters = name.chars();
        let lower = |character: char| character.is_ascii_lowercase() || character.is_ascii_digit();
        let named = characters.next().is_some_and(lower)
            && characters.all(|character| lower(character) || character == '-');
        if !named {
            return fail(&format!(
                "the skill in {} is named {name:?}; a skill's folder is named with [a-z0-9][a-z0-9-]*",
                directory.display()
            ));
        }
        let _ = writeln!(source, "    Skill {{ name: {name:?}, files: &[");
        for file in files {
            println!("cargo::rerun-if-changed={}", file.display());
            let relative = file.strip_prefix(&directory).unwrap_or(&file);
            let (Some(relative), Some(absolute)) = (relative.to_str(), file.to_str()) else {
                return fail(&format!(
                    "the skill file {} has a path that is not UTF-8, which the binary cannot embed",
                    file.display()
                ));
            };
            let _ = writeln!(
                source,
                "        ({relative:?}, include_bytes!({absolute:?})),"
            );
        }
        source.push_str("    ] },\n");
    }
    source.push_str("]\n");
    let out = std::env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_default();
    let written = out.join("skills.rs");
    if let Err(error) = std::fs::write(&written, source) {
        fail(&format!("cannot write {}: {error}", written.display()));
    }
}

/// Fails the build with `message`, which cargo prints as an error.
fn fail(message: &str) {
    println!("cargo::error={message}");
}

/// `seconds` since the Unix epoch as a UTC timestamp, such as
/// `2026-11-13T10:00:00Z`.
fn utc(seconds: i64) -> String {
    jiff::Timestamp::from_second(seconds).map_or_else(|_| seconds.to_string(), |at| at.to_string())
}
