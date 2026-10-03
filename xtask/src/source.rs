//! The source file length cap and the `#[expect]` ratchet. Both cover
//! production source only: files named `tests.rs` and files under a `tests`
//! directory are exempt.

use std::path::{Component, Path};

use crate::config::{ExpectBudget, SourceBudget};
use crate::files::{self, FilesError, Root};
use crate::report::{Check, CheckKind, Limit, Part};

pub(crate) fn checks(
    root: &Root,
    length: &SourceBudget,
    expect: &ExpectBudget,
) -> Result<[Check; 2], FilesError> {
    let mut lines = Vec::new();
    let mut expects = Vec::new();
    for path in root.rust_sources()? {
        if is_test(root.relative(&path)) {
            continue;
        }
        let text = files::read(&path)?;
        lines.push(Part::new(root.display(&path), text.lines().count()));
        let count = expect_attributes(&text);
        if count > 0 {
            expects.push(Part::new(root.display(&path), count));
        }
    }
    Ok([
        Check::largest(
            CheckKind::SourceLines,
            Limit::AtMost(length.max_lines),
            lines,
        ),
        Check::total(CheckKind::Expect, Limit::Exactly(expect.count), expects),
    ])
}

fn is_test(relative: &Path) -> bool {
    relative.file_name().is_some_and(|name| name == "tests.rs")
        || relative
            .components()
            .any(|component| component == Component::Normal("tests".as_ref()))
}

/// The number of lint `expect` attributes in `text`, counting one inside a
/// `cfg_attr` too. An attribute is recognised where a line starts with `#[` or
/// `#![`, which is where rustfmt puts one; it may continue over several lines.
pub(crate) fn expect_attributes(text: &str) -> usize {
    let mut count = 0;
    let mut open: Option<Attribute> = None;
    for line in text.lines() {
        let (mut attribute, rest) = if let Some(attribute) = open.take() {
            (attribute, line)
        } else {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed
                .strip_prefix("#![")
                .or_else(|| trimmed.strip_prefix("#["))
            else {
                continue;
            };
            (Attribute::default(), rest)
        };
        if attribute.feed(rest) {
            count += usize::from(attribute.is_expect());
        } else {
            open = Some(attribute);
        }
    }
    count
}

/// The text of one attribute between its outer brackets, with string literals
/// removed so that a `reason` cannot look like an `expect`.
#[derive(Debug, Default)]
struct Attribute {
    text: String,
    depth: usize,
    in_string: bool,
    escaped: bool,
}

impl Attribute {
    /// Reads the next part of the attribute; true once its closing bracket has
    /// been read.
    fn feed(&mut self, chunk: &str) -> bool {
        for character in chunk.chars() {
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if character == '\\' {
                    self.escaped = true;
                } else if character == '"' {
                    self.in_string = false;
                }
                continue;
            }
            match character {
                '"' => self.in_string = true,
                '[' => self.depth += 1,
                ']' if self.depth == 0 => return true,
                ']' => self.depth -= 1,
                _ => self.text.push(character),
            }
        }
        self.text.push(' ');
        false
    }

    fn is_expect(&self) -> bool {
        let body = self.text.trim_start();
        body.starts_with("expect(") || (body.starts_with("cfg_attr(") && body.contains("expect("))
    }
}

#[cfg(test)]
mod tests;
