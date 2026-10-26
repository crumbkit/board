//! A unified diff, as `git diff` or `diff -u` prints it, read into files,
//! hunks and numbered lines for the `diff` kind. Like `git apply`, it skips
//! lines outside a file diff, such as a commit message. Paths, hunk headers
//! and line text are read with hidden characters escaped (see [`escaped`]),
//! so a patch cannot hide code from the owner who reviews it.

use serde::Serialize;

use crate::path::{escaped, is_hidden};

/// The most lines a patch may hold. Each line becomes a JSON object on
/// every put and every page request, so a 10 MB patch once took 1.77 GB.
pub(crate) const MAX_LINES: usize = 20_000;

/// A patch: each file it changes, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Patch {
    pub(crate) files: Vec<FileDiff>,
}

/// One file's change. `old` is absent for a new file and `new` for a
/// deleted one. `copied` marks a copy, and `old_mode` and `new_mode` a
/// changed mode.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct FileDiff {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) new: Option<String>,
    pub(crate) added: usize,
    pub(crate) removed: usize,
    pub(crate) binary: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) copied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) old_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) new_mode: Option<String>,
    pub(crate) hunks: Vec<Hunk>,
}

/// A hunk: its `@@` header line and its lines.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Hunk {
    pub(crate) header: String,
    pub(crate) lines: Vec<Line>,
}

/// A line of a hunk with its numbers in the old and new file. `hidden`
/// says that its text held hidden characters, now escaped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Line {
    pub(crate) change: Change,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) old: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) new: Option<usize>,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Change {
    Context,
    Added,
    Removed,
}

/// Why a text does not read as a unified diff. Lines count from 1.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PatchError {
    #[error("it holds no file diff, which starts with `diff --git` or `---` and `+++` lines")]
    Empty,
    #[error("line {line} starts a hunk before any file diff names its file")]
    Fragment { line: usize },
    #[error("line {line} is not a hunk header of the form `@@ -1,3 +1,4 @@`")]
    Header { line: usize },
    #[error("the hunk at line {line} ends before it holds the lines its header counts")]
    Short { line: usize },
    #[error("line {line}, in the hunk at line {hunk}, starts with none of ` `, `+`, `-` or `\\`")]
    Stray { line: usize, hunk: usize },
    #[error("line {line} goes past the old or new line count of the hunk header at line {hunk}")]
    Over { line: usize, hunk: usize },
    #[error("it holds more than {max} lines; split the diff across slots or bindings")]
    TooLong { max: usize },
}

impl Patch {
    pub(crate) fn parse(text: &str) -> Result<Self, PatchError> {
        Self::parse_within(text, MAX_LINES)
    }

    /// Reads `text`, refusing it when it holds more than `max` lines. Each
    /// line keeps a `\r` before its `\n`, so a change to line endings shows.
    pub(crate) fn parse_within(text: &str, max: usize) -> Result<Self, PatchError> {
        let mut lines: Vec<&str> = text.split('\n').collect();
        if text.ends_with('\n') {
            lines.pop();
        }
        if lines.len() > max {
            return Err(PatchError::TooLong { max });
        }
        let mut files: Vec<FileDiff> = Vec::new();
        let mut at = 0;
        while let Some(line) = lines.get(at).map(|line| line.trim_end_matches('\r')) {
            if let Some(rest) = line.strip_prefix("diff --git ") {
                let name = same_halves(rest);
                files.push(FileDiff {
                    old: name.clone(),
                    new: name,
                    ..FileDiff::default()
                });
            } else if let (Some(old), Some(new)) = (
                line.strip_prefix("--- "),
                lines
                    .get(at + 1)
                    .and_then(|next| next.trim_end_matches('\r').strip_prefix("+++ ")),
            ) {
                let (old, new) = names(old, new);
                match files.last_mut() {
                    Some(file) if file.hunks.is_empty() && !file.binary => {
                        (file.old, file.new) = (old, new);
                    }
                    _ => files.push(FileDiff {
                        old,
                        new,
                        ..FileDiff::default()
                    }),
                }
                at += 1;
            } else if line.starts_with("@@") {
                let file = files
                    .last_mut()
                    .ok_or(PatchError::Fragment { line: at + 1 })?;
                at = hunk(&lines, at, file)?;
                continue;
            } else if let Some(file) = files.last_mut() {
                extended(line, file);
            }
            at += 1;
        }
        if files.is_empty() {
            return Err(PatchError::Empty);
        }
        Ok(Self { files })
    }
}

/// Reads the hunk whose header is at `start` into `file`, and returns the
/// index of the line after it.
fn hunk(lines: &[&str], start: usize, file: &mut FileDiff) -> Result<usize, PatchError> {
    let header = lines[start].trim_end_matches('\r');
    let (mut old, mut old_left, mut new, mut new_left) =
        counts(header).ok_or(PatchError::Header { line: start + 1 })?;
    let mut read = Vec::new();
    let mut at = start + 1;
    while old_left > 0 || new_left > 0 {
        let line = lines.get(at).ok_or(PatchError::Short { line: start + 1 })?;
        at += 1;
        let (line_number, hunk_number) = (at, start + 1);
        let (change, text) = match line.chars().next() {
            Some('\\') => continue,
            Some(' ') => (Change::Context, &line[1..]),
            None => (Change::Context, ""),
            // A blank context line of a CRLF patch whose space was stripped.
            Some('\r') if *line == "\r" => (Change::Context, *line),
            Some('-') => (Change::Removed, &line[1..]),
            Some('+') => (Change::Added, &line[1..]),
            Some(_) => {
                return Err(PatchError::Stray {
                    line: line_number,
                    hunk: hunk_number,
                });
            }
        };
        let (old_number, new_number) = match change {
            Change::Context if old_left > 0 && new_left > 0 => {
                (old_left, new_left) = (old_left - 1, new_left - 1);
                (old, new) = (old + 1, new + 1);
                (Some(old - 1), Some(new - 1))
            }
            Change::Removed if old_left > 0 => {
                (old_left, old, file.removed) = (old_left - 1, old + 1, file.removed + 1);
                (Some(old - 1), None)
            }
            Change::Added if new_left > 0 => {
                (new_left, new, file.added) = (new_left - 1, new + 1, file.added + 1);
                (None, Some(new - 1))
            }
            _ => {
                return Err(PatchError::Over {
                    line: line_number,
                    hunk: hunk_number,
                });
            }
        };
        read.push(Line {
            change,
            old: old_number,
            new: new_number,
            hidden: text.chars().any(|char| char != '\t' && is_hidden(char)),
            text: code(text),
        });
    }
    if lines.get(at).is_some_and(|line| line.starts_with('\\')) {
        at += 1;
    }
    file.hunks.push(Hunk {
        header: escaped(header),
        lines: read,
    });
    Ok(at)
}

/// The old start, old count, new start and new count of a header such as
/// `@@ -1,3 +1,4 @@ fn main`. A count left out is 1. A range whose end
/// overflows is no header, so numbering its lines cannot overflow.
fn counts(header: &str) -> Option<(usize, usize, usize, usize)> {
    let rest = header.strip_prefix("@@ -")?;
    let (ranges, _) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |text: &str| -> Option<(usize, usize)> {
        let (start, count): (usize, usize) = match text.split_once(',') {
            Some((start, count)) => (start.parse().ok()?, count.parse().ok()?),
            None => (text.parse().ok()?, 1),
        };
        start.checked_add(count).map(|_| (start, count))
    };
    let (old_start, old_count) = range(old)?;
    let (new_start, new_count) = range(new)?;
    Some((old_start, old_count, new_start, new_count))
}

/// Applies a git extended header line, which names a rename, a copy, a
/// changed mode, a new or deleted file, or a binary change.
fn extended(line: &str, file: &mut FileDiff) {
    if !file.hunks.is_empty() {
        return;
    }
    let from = line
        .strip_prefix("rename from ")
        .or_else(|| line.strip_prefix("copy from "));
    let to = line
        .strip_prefix("rename to ")
        .or_else(|| line.strip_prefix("copy to "));
    if let Some(name) = from {
        file.old = Some(escaped(&unquoted(name)));
    } else if let Some(name) = to {
        file.new = Some(escaped(&unquoted(name)));
        file.copied = line.starts_with("copy ");
    } else if let Some(mode) = line.strip_prefix("old mode ") {
        file.old_mode = Some(escaped(mode));
    } else if let Some(mode) = line.strip_prefix("new mode ") {
        file.new_mode = Some(escaped(mode));
    } else if line.starts_with("new file mode ") {
        file.old = None;
    } else if line.starts_with("deleted file mode ") {
        file.new = None;
    } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
        file.binary = true;
    }
}

/// The path of a `diff --git a/<path> b/<path>` header whose two halves
/// name the same path. Git quotes both halves when the path holds unusual
/// bytes. A header that renames is read from its `rename from` and
/// `rename to` lines instead.
fn same_halves(rest: &str) -> Option<String> {
    if rest.starts_with('"') {
        let (old, after) = quoted(rest)?;
        let (new, _) = quoted(after.strip_prefix(' ')?)?;
        let path = old.strip_prefix("a/")?;
        return (new.strip_prefix("b/")? == path).then(|| escaped(path));
    }
    let half = rest.len().checked_sub(5)? / 2;
    let path = rest.strip_prefix("a/")?.get(..half)?;
    (rest.get(2 + half..)?.strip_prefix(" b/")? == path).then(|| escaped(path))
}

/// The old and new paths of `---` and `+++` lines: a tab and what follows,
/// such as a timestamp, is dropped, `/dev/null` is no file, and git's `a/`
/// and `b/` prefixes are dropped when both sides carry them.
fn names(old: &str, new: &str) -> (Option<String>, Option<String>) {
    let (old, new) = (path_of(old), path_of(new));
    let (old, new) = (old.as_deref(), new.as_deref());
    let prefixed = old.is_none_or(|path| path.starts_with("a/"))
        && new.is_none_or(|path| path.starts_with("b/"))
        && (old.is_some() || new.is_some());
    let strip = |path: &str| {
        if prefixed {
            escaped(&path[2..])
        } else {
            escaped(path)
        }
    };
    (old.map(strip), new.map(strip))
}

/// The path of a `---` or `+++` line, unquoted, or none for `/dev/null`.
fn path_of(text: &str) -> Option<String> {
    let path = if text.starts_with('"') {
        unquoted(text)
    } else {
        text.split('\t').next().unwrap_or(text).to_owned()
    };
    (path != "/dev/null").then_some(path)
}

/// A path as git quotes one that holds unusual bytes, `"a/caf\303\251"`,
/// with its C escapes and octal bytes read back. Any other text is kept.
fn unquoted(text: &str) -> String {
    quoted(text).map_or_else(|| text.to_owned(), |(path, _)| path)
}

/// The path quoted at the start of `text`, read back, and the text after
/// its closing quote. Text that does not start with a closed quote is none.
fn quoted(text: &str) -> Option<(String, &str)> {
    let inner = text.strip_prefix('"')?.as_bytes();
    let mut bytes = Vec::with_capacity(inner.len());
    let mut at = 0;
    loop {
        let byte = *inner.get(at)?;
        at += 1;
        match byte {
            b'"' => break,
            b'\\' => {
                let escape = *inner.get(at)?;
                at += 1;
                bytes.push(match escape {
                    b'0'..=b'7' => {
                        let mut value = escape - b'0';
                        for _ in 0..2 {
                            match inner.get(at) {
                                Some(digit @ b'0'..=b'7') => {
                                    value = value.wrapping_mul(8).wrapping_add(digit - b'0');
                                    at += 1;
                                }
                                _ => break,
                            }
                        }
                        value
                    }
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => 0x0b,
                    b'f' => 0x0c,
                    b'r' => b'\r',
                    other => other,
                });
            }
            other => bytes.push(other),
        }
    }
    // The closing quote is ASCII, so `1 + at` is a character boundary.
    Some((
        String::from_utf8_lossy(&bytes).into_owned(),
        &text[1 + at..],
    ))
}

/// A line's text with hidden characters escaped, except tabs, which indent
/// code and cannot reorder it.
fn code(text: &str) -> String {
    text.split('\t').map(escaped).collect::<Vec<_>>().join("\t")
}

#[cfg(test)]
mod tests;
