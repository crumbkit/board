//! The rule for `BOARD_BUILD_DATE`, the commit date of the source a build
//! embeds, which build.rs runs and `tests/build_date.rs` includes, so that
//! the test runs the code the build runs (docs/architecture/writes/minimum.md).

use std::ffi::OsStr;

/// The seconds since the Unix epoch at which the source was committed:
/// `epoch`, the value of `SOURCE_DATE_EPOCH`, when it is set and not empty,
/// and otherwise what `log` returns, which is the committer date as
/// `git log -1 --format=%ct` prints it, for `commit` (the value of
/// `BOARD_BUILD_COMMIT`) when that is set and not empty, and for git's HEAD
/// when it is not. `None` when neither is available, including when `log`
/// cannot read `commit`. A `SOURCE_DATE_EPOCH` that is not whole seconds is the message
/// the build fails with.
pub(crate) fn seconds(
    epoch: Option<&OsStr>,
    commit: Option<&OsStr>,
    log: impl FnOnce(Option<&str>) -> Option<String>,
) -> Result<Option<i64>, String> {
    if let Some(epoch) = epoch.filter(|epoch| !epoch.is_empty()) {
        return epoch
            .to_str()
            .and_then(|text| text.parse().ok())
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "SOURCE_DATE_EPOCH is \"{}\", which is not whole seconds since the Unix epoch; set it to digits or unset it",
                    epoch.to_string_lossy().escape_debug()
                )
            });
    }
    let commit = commit.filter(|commit| !commit.is_empty());
    // A commit name that is not text cannot name a commit.
    let text = match commit.map(OsStr::to_str) {
        Some(None) => None,
        Some(Some(name)) => log(Some(name)),
        None => log(None),
    };
    Ok(text.and_then(|text| text.trim().parse().ok()))
}
