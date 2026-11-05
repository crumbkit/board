//! `cargo xtask migrate-shapes`: rewrites a board's records to the shared
//! actor, digest, subject and reference shapes of `board_plugin::shape`.
//! Without `--write` it only counts what it would change.

mod ordered;
mod rules;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use self::rules::{Context, Outcome};

#[derive(Debug, Default)]
pub(crate) struct Options {
    pub(crate) write: bool,
    pub(crate) map: Option<PathBuf>,
}

impl Options {
    /// `<board> [--write] [--map <file>]`: the board and the options.
    pub(crate) fn parse(mut arguments: impl Iterator<Item = String>) -> Option<(PathBuf, Self)> {
        let board = arguments.next()?.into();
        let mut options = Self::default();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--write" => options.write = true,
                "--map" => options.map = Some(arguments.next()?.into()),
                _ => return None,
            }
        }
        Some((board, options))
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MigrateError {
    #[error("could not read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the map {path} is not a JSON object of old strings to new values")]
    Map {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("the map {path} maps {key:?} to a value that is neither an Actor nor a Subject")]
    MapEntry { path: PathBuf, key: String },
}

impl MigrateError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Read { .. } => "xtask::migrate::unreadable",
            Self::Write { .. } => "xtask::migrate::unwritable",
            Self::Map { .. } => "xtask::migrate::map_invalid",
            Self::MapEntry { .. } => "xtask::migrate::map_entry_invalid",
        }
    }
}

/// Migrates every JSON record and Markdown task in the board at `board`,
/// outside hidden directories, and reports the counts: files read, changed
/// and written, each field changed, and each value dropped or left unmapped.
pub(crate) fn run(board: &Path, options: &Options) -> Result<Value, MigrateError> {
    let context = match &options.map {
        Some(path) => {
            let text = read(path)?;
            let map = serde_json::from_str(&text).map_err(|source| MigrateError::Map {
                path: path.clone(),
                source,
            })?;
            Context::new(map).map_err(|key| MigrateError::MapEntry {
                path: path.clone(),
                key,
            })?
        }
        None => Context::default(),
    };
    let resolved = RefCell::new(HashMap::new());
    let mount = |name: &str| {
        resolved
            .borrow_mut()
            .entry(name.to_owned())
            .or_insert_with(|| repository(board, name, 0))
            .clone()
    };
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut dropped = Vec::new();
    let mut unmapped = Vec::new();
    let [mut seen, mut changed, mut written] = [0; 3];
    for path in files(board)? {
        let text = read(&path)?;
        let migrated = if path.extension().is_some_and(|extension| extension == "md") {
            seen += 1;
            rules::markdown(&text, &context)
        } else {
            let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            seen += 1;
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            let outcomes = rules::record(&mut value, &stem, &context, &mount);
            Some((ordered::text(&value, &text), outcomes))
        };
        let Some((text, outcomes)) = migrated else {
            continue;
        };
        let name = format!("/{}", path.strip_prefix(board).unwrap_or(&path).display());
        let mut edited = false;
        for outcome in outcomes {
            match outcome {
                Outcome::Changed(field) => {
                    edited = true;
                    *counts.entry(field).or_default() += 1;
                }
                Outcome::Dropped(field, value) => {
                    edited = true;
                    dropped.push(json!({"path": name, "field": field, "value": value}));
                }
                Outcome::Unmapped(field, value) => {
                    unmapped.push(json!({"path": name, "field": field, "value": value}));
                }
            }
        }
        if !edited {
            continue;
        }
        changed += 1;
        if options.write {
            write(&path, &text)?;
            written += 1;
        }
    }
    Ok(json!({
        "files": {"read": seen, "changed": changed, "written": written},
        "changed": counts,
        "dropped": dropped,
        "unmapped": unmapped,
    }))
}

/// The GitHub `owner/name` of the repository the board at `board` mounts at
/// `/repos/<name>`, following a mount of a path inside another mount.
fn repository(board: &Path, name: &str, depth: usize) -> Option<String> {
    if depth > 8 || name.contains('/') || name.is_empty() {
        return None;
    }
    let text = std::fs::read_to_string(board.join("repos").join(format!("{name}.json"))).ok()?;
    let mount: Value = serde_json::from_str(&text).ok()?;
    if mount.get("$type").and_then(Value::as_str) != Some("board/mount/v1") {
        return None;
    }
    if let Some(repo) = mount.get("repo").and_then(Value::as_str) {
        let output = crate::process::command("git")
            .arg("-C")
            .arg(home(repo))
            .args(["remote", "get-url", "origin"])
            .output()
            .ok()?;
        return rules::github(String::from_utf8_lossy(&output.stdout).trim());
    }
    let path = mount.get("path").and_then(Value::as_str)?;
    let rest = path.strip_prefix("/repos/")?;
    let (outer, inner) = rest.split_once("/repos/")?;
    let outer: Value = serde_json::from_str(
        &std::fs::read_to_string(board.join("repos").join(format!("{outer}.json"))).ok()?,
    )
    .ok()?;
    let directory = outer.get("repo").and_then(Value::as_str)?;
    repository(&home(directory), inner, depth + 1)
}

/// The mount's `repo` path, with a leading `~/` read as the home directory,
/// as Board reads it.
fn home(repo: &str) -> PathBuf {
    match (repo.strip_prefix("~/"), crate::process::var("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(repo),
    }
}

/// The `.json` and `.md` files below `board`, outside hidden directories,
/// in sorted order.
fn files(board: &Path) -> Result<Vec<PathBuf>, MigrateError> {
    let mut pending = vec![board.to_path_buf()];
    let mut found = Vec::new();
    while let Some(directory) = pending.pop() {
        let unreadable = |source| MigrateError::Read {
            path: directory.clone(),
            source,
        };
        for entry in std::fs::read_dir(&directory).map_err(unreadable)? {
            let entry = entry.map_err(unreadable)?;
            let kind = entry.file_type().map_err(unreadable)?;
            let path = entry.path();
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if kind.is_dir() && !hidden {
                pending.push(path);
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json" || extension == "md")
            {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

fn read(path: &Path) -> Result<String, MigrateError> {
    std::fs::read_to_string(path).map_err(|source| MigrateError::Read {
        path: path.to_path_buf(),
        source,
    })
}

#[expect(
    clippy::disallowed_methods,
    reason = "the migration rewrites the board it was pointed at, and nothing else"
)]
fn write(path: &Path, text: &str) -> Result<(), MigrateError> {
    std::fs::write(path, text).map_err(|source| MigrateError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests;
