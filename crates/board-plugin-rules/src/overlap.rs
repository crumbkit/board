//! Whether two `SQLite` GLOBs can match one path. SQL has no operator for
//! this, since GLOB matches a pattern against a path, never against another
//! pattern, so this module asks it of the two patterns' characters.

use std::collections::HashSet;

/// One step of a GLOB: `*`, a single character of a set, or one character.
#[derive(Clone, Copy)]
enum Step {
    Any,
    One,
    Char(char),
}

/// Whether some path matches both `one` and `other`, read as `SQLite`'s
/// GLOB reads them, where `*` also matches `/`. A set such as `[ab]` is
/// read as any one character, so the answer can be yes for two patterns
/// no path matches, never no for two that one path matches.
pub(crate) fn overlap(one: &str, other: &str) -> bool {
    let (one, other) = (steps(one), steps(other));
    let mut seen = HashSet::new();
    let mut pending = vec![(0, 0)];
    while let Some((i, j)) = pending.pop() {
        if !seen.insert((i, j)) {
            continue;
        }
        match (one.get(i), other.get(j)) {
            (None, None) => return true,
            (Some(Step::Any), _) => {
                pending.push((i + 1, j));
                if j < other.len() {
                    pending.push((i, j + 1));
                }
            }
            (_, Some(Step::Any)) => {
                pending.push((i, j + 1));
                if i < one.len() {
                    pending.push((i + 1, j));
                }
            }
            (Some(Step::Char(a)), Some(Step::Char(b))) if a != b => {}
            (Some(_), Some(_)) => pending.push((i + 1, j + 1)),
            (None, Some(_)) | (Some(_), None) => {}
        }
    }
    false
}

/// The steps of `pattern`. A `[` with no closing `]` is read as one
/// character of a set.
fn steps(pattern: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        steps.push(match c {
            '*' => Step::Any,
            '?' => Step::One,
            '[' => {
                // A `]` first in the set, or after `^`, is one of its characters.
                if chars.peek() == Some(&'^') {
                    chars.next();
                }
                if chars.peek() == Some(&']') {
                    chars.next();
                }
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
                Step::One
            }
            c => Step::Char(c),
        });
    }
    steps
}

#[cfg(test)]
mod tests;
