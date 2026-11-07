//! The rule for `BOARD_BUILD`, the suffix after Board's version, which
//! build.rs runs and `tests/build_commit.rs` includes, so that the test runs
//! the code the build runs (docs/architecture/performance/timing.md).

/// The suffix: `+` and the commit `given` names, its first 12 hex digits
/// lowercased, or all of them when it has fewer; with `given` unset or empty,
/// `+` and the commit `head` names, then `.dirty.` and what `changes` returns
/// when the source differs from it, or nothing when git names no commit.
/// A `given` that is not 7 to 40 hex digits is the message the build fails
/// with.
pub(crate) fn suffix(
    given: Option<&str>,
    head: impl FnOnce() -> Option<String>,
    changes: impl FnOnce() -> Option<String>,
) -> Result<String, String> {
    match given.filter(|given| !given.is_empty()) {
        Some(given)
            if (7..=40).contains(&given.len())
                && given.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            Ok(format!(
                "+{}",
                given.get(..12).unwrap_or(given).to_ascii_lowercase()
            ))
        }
        Some(given) => Err(format!(
            "BOARD_BUILD_COMMIT is {given:?}, which is not a commit; set it to the 7 to 40 hex digits of the commit the source was taken from, or unset it"
        )),
        None => Ok(head().map_or_else(String::new, |commit| match changes() {
            Some(changes) => format!("+{commit}.dirty.{changes}"),
            None => format!("+{commit}"),
        })),
    }
}
