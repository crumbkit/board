//! The rule for `BOARD_BUILD`, the suffix after Board's version, which
//! build.rs runs and `tests/build_commit.rs` includes, so that the test runs
//! the code the build runs (docs/architecture/performance/timing.md).

use std::ffi::OsStr;

const MUST: &str =
    "set it to the 7 to 40 hex digits of the commit the source was taken from, or unset it";

/// The suffix: `+` and the commit `given` names, its first 12 hex digits
/// lowercased, or all of them when it has fewer; with `given` unset or empty,
/// `+` and the commit `head` names, or nothing when git names no commit.
/// After the commit comes `.dirty.` and what `changes` returns when the local
/// git says the source differs from its own commit. A `given` that is not 7
/// to 40 hex digits is the message the build fails with.
pub(crate) fn suffix(
    given: Option<&OsStr>,
    head: impl FnOnce() -> Option<String>,
    changes: impl FnOnce() -> Option<String>,
) -> Result<String, String> {
    let commit = match given.filter(|given| !given.is_empty()) {
        None => head(),
        Some(given) => Some(named(given)?),
    };
    Ok(commit.map_or_else(String::new, |commit| match changes() {
        Some(changes) => format!("+{commit}.dirty.{changes}"),
        None => format!("+{commit}"),
    }))
}

/// The first 12 hex digits of `given`, lowercased, or the message that says
/// why it is not a commit.
fn named(given: &OsStr) -> Result<String, String> {
    let Some(text) = given.to_str() else {
        let shown = shown(&given.to_string_lossy());
        return Err(format!(
            "BOARD_BUILD_COMMIT is \"{shown}\", which is not Unicode, so it is not a commit; {MUST}"
        ));
    };
    if (7..=40).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(text.get(..12).unwrap_or(text).to_ascii_lowercase())
    } else {
        let shown = shown(text);
        Err(format!(
            "BOARD_BUILD_COMMIT is \"{shown}\", which is not a commit; {MUST}"
        ))
    }
}

/// `value` with each control character escaped, so that the message stays on
/// the one `cargo::error=` line.
fn shown(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                character.escape_default().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}
