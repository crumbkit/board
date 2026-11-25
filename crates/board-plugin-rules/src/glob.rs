//! `SQLite`'s GLOB, matched in Rust so that `check` can compare each
//! pattern only with the files that start with its literal prefix. It
//! follows `patternCompare` in `SQLite`'s func.c: `*` matches any run of
//! characters, `/` included, `?` matches one character, `[...]` matches one
//! character of a set, with `^` first to negate it and `a-z` for a range,
//! and a `[` with no closing `]` matches nothing. Case counts and nothing
//! escapes.

/// What comparing a pattern with a text from some point found. `Never`
/// means no later start in the text can match either, which ends the
/// search a `*` makes, as in `SQLite`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Match,
    NoMatch,
    Never,
}

/// Whether `text GLOB pattern` is true in `SQLite`.
pub(crate) fn glob(pattern: &str, text: &str) -> bool {
    compare(pattern, text) == Outcome::Match
}

/// The characters of `pattern` before its first `*`, `?` or `[`, with
/// which every text it matches starts.
pub(crate) fn prefix(pattern: &str) -> &str {
    let end = pattern.find(['*', '?', '[']).unwrap_or(pattern.len());
    &pattern[..end]
}

/// The first character of `text`, and `text` moves past it.
fn take(text: &mut &str) -> Option<char> {
    let c = text.chars().next()?;
    *text = &text[c.len_utf8()..];
    Some(c)
}

fn compare(mut pattern: &str, mut text: &str) -> Outcome {
    while let Some(c) = take(&mut pattern) {
        match c {
            '*' => return any(pattern, text),
            '[' => {
                let Some(t) = take(&mut text) else {
                    return Outcome::NoMatch;
                };
                if !in_set(&mut pattern, t) {
                    return Outcome::NoMatch;
                }
            }
            c => match take(&mut text) {
                Some(t) if t == c || c == '?' => {}
                _ => return Outcome::NoMatch,
            },
        }
    }
    if text.is_empty() {
        Outcome::Match
    } else {
        Outcome::NoMatch
    }
}

/// Compares what follows a `*` in `pattern` with `text`. Further `*`s are
/// skipped, each `?` among them takes one character of `text`, and the
/// rest is tried from each point in `text` where it could start.
fn any(mut pattern: &str, mut text: &str) -> Outcome {
    let next = loop {
        let before = pattern;
        match take(&mut pattern) {
            Some('*') => {}
            Some('?') => {
                if take(&mut text).is_none() {
                    return Outcome::Never;
                }
            }
            Some('[') => {
                // A set after the `*` is tried from every point in the text.
                while !text.is_empty() {
                    let found = compare(before, text);
                    if found != Outcome::NoMatch {
                        return found;
                    }
                    take(&mut text);
                }
                return Outcome::Never;
            }
            next => break next,
        }
    };
    let Some(c) = next else {
        return Outcome::Match;
    };
    while let Some(at) = text.find(c) {
        text = &text[at + c.len_utf8()..];
        let found = compare(pattern, text);
        if found != Outcome::NoMatch {
            return found;
        }
    }
    Outcome::Never
}

/// Whether `t` is in the set that `pattern` holds after its `[`, and
/// `pattern` moves past the set's `]`. A set with no `]` holds nothing.
fn in_set(pattern: &mut &str, t: char) -> bool {
    let mut seen = false;
    let mut invert = false;
    let mut c = take(pattern);
    if c == Some('^') {
        invert = true;
        c = take(pattern);
    }
    // A `]` first in the set is one of its characters.
    if c == Some(']') {
        seen = t == ']';
        c = take(pattern);
    }
    let mut prior = None;
    while let Some(each) = c.filter(|each| *each != ']') {
        let ahead = pattern.chars().next();
        match prior {
            Some(low) if each == '-' && ahead.is_some_and(|ahead| ahead != ']') => {
                let high = take(pattern);
                seen |= high.is_some_and(|high| low <= t && t <= high);
                prior = None;
            }
            _ => {
                seen |= t == each;
                prior = Some(each);
            }
        }
        c = take(pattern);
    }
    c.is_some() && seen != invert
}

#[cfg(test)]
mod tests;
