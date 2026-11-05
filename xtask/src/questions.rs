//! `cargo xtask migrate-questions <board> [--dry-run]`: the one-off migration
//! that drops `answered` from `plans/question/v1.status`. A question is
//! answered when a `plans/answer/v1` names it, so each question stored as
//! `answered` becomes `open` and its answer keeps the fact. It reports every
//! question whose stored status disagreed with its answers.
//!
//! It reads the board's own working tree, every `.json` file outside a
//! directory whose name starts with a dot, and never a mount. A record's
//! logical path is its path from the board's root without `.json`, which is
//! how an answer's `question` names it. Only the status's own bytes change,
//! so the rest of each file is left as it was.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

const QUESTION: &str = "plans/question/v1";
const ANSWER: &str = "plans/answer/v1";

/// What the migration found and did.
#[derive(Debug, Default, PartialEq, Serialize)]
pub(crate) struct Migrated {
    /// The questions read, by their stored status.
    pub(crate) questions: BTreeMap<String, usize>,
    /// The answers read.
    pub(crate) answers: usize,
    /// The questions rewritten from `answered` to `open`, or that would be
    /// with `--dry-run`.
    pub(crate) rewritten: Vec<String>,
    /// Each question whose stored status disagreed with its answers, and
    /// each whose status is none of `open`, `answered` and `withdrawn`.
    pub(crate) disagreed: Vec<Disagreement>,
    /// Answers whose `question` names no question in the board.
    pub(crate) dangling: Vec<String>,
    /// Questions stored as `answered` whose status could not be rewritten in
    /// place, which a person fixes by hand.
    pub(crate) by_hand: Vec<String>,
}

/// A question whose stored status said one thing and its answers another.
#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct Disagreement {
    pub(crate) question: String,
    pub(crate) status: String,
    /// The paths of the answers that name it; none when it said `answered`.
    pub(crate) answers: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MigrateError {
    #[error("could not read or write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl MigrateError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "xtask::migrate::io",
        }
    }
}

struct Question {
    file: PathBuf,
    text: String,
    status: String,
}

/// Migrates the board at `root`, writing nothing when `dry_run`.
pub(crate) fn migrate(root: &Path, dry_run: bool) -> Result<Migrated, MigrateError> {
    let mut questions: BTreeMap<String, Question> = BTreeMap::new();
    let mut answers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut migrated = Migrated::default();
    for file in records(root)? {
        let text = fs::read_to_string(&file).map_err(|source| io_error(&file, source))?;
        let Ok(record) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let logical = logical(root, &file);
        match record.get("$type").and_then(Value::as_str) {
            Some(QUESTION) => {
                let status = match record.get("status") {
                    Some(Value::String(status)) => status.clone(),
                    other => other.map_or_else(|| "none".to_owned(), Value::to_string),
                };
                *migrated.questions.entry(status.clone()).or_default() += 1;
                questions.insert(logical, Question { file, text, status });
            }
            Some(ANSWER) => {
                migrated.answers += 1;
                let named = record.get("question").and_then(Value::as_str);
                answers
                    .entry(named.unwrap_or_default().to_owned())
                    .or_default()
                    .push(logical);
            }
            _ => {}
        }
    }
    for (named, paths) in &answers {
        if !questions.contains_key(named) {
            migrated.dangling.extend(paths.iter().cloned());
        }
    }
    for (path, question) in &questions {
        let named = answers.get(path).cloned().unwrap_or_default();
        let answered = !named.is_empty();
        let agrees = match question.status.as_str() {
            "answered" => answered,
            "open" | "withdrawn" => !answered,
            _ => false,
        };
        if !agrees {
            migrated.disagreed.push(Disagreement {
                question: path.clone(),
                status: question.status.clone(),
                answers: named,
            });
        }
        if question.status != "answered" {
            continue;
        }
        let Some(text) = reopened(&question.text) else {
            migrated.by_hand.push(path.clone());
            continue;
        };
        if !dry_run {
            write(&question.file, &text)?;
        }
        migrated.rewritten.push(path.clone());
    }
    migrated.dangling.sort();
    Ok(migrated)
}

/// `text` with its one `"status": "answered"` spelled `"status": "open"`,
/// when the result reads as the same record with only the status changed.
fn reopened(text: &str) -> Option<String> {
    let mut expected: Value = serde_json::from_str(text).ok()?;
    expected["status"] = Value::from("open");
    let mut found = None;
    for (at, _) in text.match_indices("\"status\"") {
        let rest = text[at + "\"status\"".len()..].trim_start();
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let value = rest.trim_start();
        if value.starts_with("\"answered\"") {
            let start = text.len() - value.len();
            if found.replace(start).is_some() {
                return None;
            }
        }
    }
    let start = found?;
    let text = format!(
        "{}\"open\"{}",
        &text[..start],
        &text[start + "\"answered\"".len()..]
    );
    (serde_json::from_str::<Value>(&text).ok()? == expected).then_some(text)
}

/// Every `.json` file under `root`, outside directories whose name starts
/// with a dot, in path order.
fn records(root: &Path) -> Result<Vec<PathBuf>, MigrateError> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|source| io_error(&directory, source))?;
        for entry in entries {
            let entry = entry.map_err(|source| io_error(&directory, source))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|source| io_error(&path, source))?;
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if kind.is_dir() && !hidden {
                pending.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// `/<path from root>` without `.json`, with `/` between segments.
fn logical(root: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(root).unwrap_or(file).with_extension("");
    let segments: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    format!("/{}", segments.join("/"))
}

#[expect(
    clippy::disallowed_methods,
    reason = "the migration rewrites the question records of the board it is given, and nothing else"
)]
fn write(file: &Path, text: &str) -> Result<(), MigrateError> {
    fs::write(file, text).map_err(|source| io_error(file, source))
}

fn io_error(path: &Path, source: io::Error) -> MigrateError {
    MigrateError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests;
