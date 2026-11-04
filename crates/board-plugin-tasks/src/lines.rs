//! The line editor the write calls change a task's frontmatter with
//! (plugins/tasks/README.md, Writing tasks). It replaces or adds single
//! top-level `key: value` lines and leaves every other byte as it was, so a
//! comment, a key order or a field another tool wrote survives. It reads
//! lines, never YAML, so it refuses a key whose line is not one plain
//! scalar rather than guess how to rewrite it.

use board_ops::PluginError;
use serde_json::{Value, json};

use crate::refusal;

/// `text` with each key's line set to its value, written as JSON, which is
/// YAML; a missing key goes last. A value of none removes the key's line,
/// and adds none when the key is missing.
pub(crate) fn with_fields(
    text: &str,
    values: &[(&str, Option<String>)],
    path: &str,
) -> Result<String, PluginError> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let not_plain = |why: String, key: Option<&str>| {
        let mut fields = vec![("path", json!(path))];
        fields.extend(key.map(|key| ("key", json!(key))));
        refusal("tasks::task::not_plain", why, fields)
    };
    let first = lines.first().copied().unwrap_or("");
    let newline = if first.ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    if bare(first.strip_prefix('\u{feff}').unwrap_or(first)) != "---" {
        let why = format!("{path} has no frontmatter; start it with ---");
        return Err(not_plain(why, None));
    }
    let Some(end) = (1..lines.len()).find(|&at| bare(lines[at]) == "---") else {
        let why =
            format!("{path} has no line --- closing its frontmatter; add one after its last field");
        return Err(not_plain(why, None));
    };
    let mut written: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
    let mut added = Vec::new();
    for (key, value) in values {
        let line = value
            .as_ref()
            .map(|value| format!("{key}: {}{newline}", Value::from(value.as_str())));
        let found: Vec<usize> = (1..end).filter(|&at| keyed(lines[at], key)).collect();
        let Some(&at) = found.first() else {
            added.extend(line);
            continue;
        };
        let held = lines[at]
            .split_once(':')
            .map_or("", |(_, rest)| rest)
            .trim_matches(python_space);
        let continued = at + 1 < end && lines[at + 1].starts_with([' ', '\t']);
        if found.len() > 1 || continued || !one_line(held) {
            let shape = if found.len() > 1 {
                "twice"
            } else if held.is_empty() && !continued {
                "with no value"
            } else {
                "as a block, a list or a tag"
            };
            let why = format!(
                "{path} holds {key} {shape}, and this call rewrites only one line holding one \
                 plain value; make it one line such as `{key}: value`, then call again"
            );
            return Err(not_plain(why, Some(key)));
        }
        written[at] = line.unwrap_or_default();
    }
    let mut out = written[..end].concat();
    out.extend(added);
    out.push_str(&written[end..].concat());
    Ok(out)
}

/// A line without its line ending.
fn bare(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

/// Whether `line` is `key`'s top-level line: the key, any spaces or tabs,
/// a colon, then whitespace or the end.
fn keyed(line: &str, key: &str) -> bool {
    let Some(rest) = line.strip_prefix(key) else {
        return false;
    };
    let rest = rest.trim_start_matches(python_space);
    rest.strip_prefix(':')
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(python_space))
}

/// Whether `held` is a value a line holds on its own: a double- or
/// single-quoted string closed on the line, or a plain scalar that starts
/// with none of YAML's indicators of a block, a collection, an anchor or a
/// tag; then an optional comment.
fn one_line(held: &str) -> bool {
    let rest = if let Some(inner) = held.strip_prefix('"') {
        let mut chars = inner.char_indices();
        let mut closed = None;
        while let Some((at, c)) = chars.next() {
            match c {
                '\\' => {
                    if chars.next().is_none() {
                        return false;
                    }
                }
                '"' => {
                    closed = Some(at + 1);
                    break;
                }
                _ => {}
            }
        }
        let Some(at) = closed else { return false };
        &inner[at..]
    } else if let Some(inner) = held.strip_prefix('\'') {
        let bytes = inner.as_bytes();
        let mut at = 0;
        loop {
            match bytes.get(at) {
                None => return false,
                Some(b'\'') if bytes.get(at + 1) == Some(&b'\'') => at += 2,
                Some(b'\'') => break,
                Some(_) => at += 1,
            }
        }
        &inner[at + 1..]
    } else {
        let Some(first) = held.chars().next() else {
            return false;
        };
        if python_space(first) || "|>[{&*!\"'#".contains(first) {
            return false;
        }
        // A plain scalar runs to a comment, whose `#` follows whitespace.
        let mut previous = first;
        for c in held[first.len_utf8()..].chars() {
            if c == '#' {
                return python_space(previous);
            }
            previous = c;
        }
        return true;
    };
    comment_only(rest)
}

/// Whether `rest` is whitespace, then optionally whitespace and a comment.
fn comment_only(rest: &str) -> bool {
    let trimmed = rest.trim_start_matches(python_space);
    trimmed.is_empty() || (trimmed.starts_with('#') && trimmed.len() < rest.len())
}

fn python_space(c: char) -> bool {
    board_repos::python_space(c)
}

#[cfg(test)]
mod tests;
