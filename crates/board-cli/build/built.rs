//! The rule for `BOARD_BUILD_DATE`, the commit date of the source a build
//! embeds, which build.rs runs and `tests/build_date.rs` includes, so that
//! the test runs the code the build runs (docs/architecture/writes/minimum.md).

use std::ffi::OsStr;

/// The seconds since the Unix epoch at which the source was committed:
/// `epoch`, the value of `SOURCE_DATE_EPOCH`, when it is set and not empty,
/// and otherwise what `head` returns, which is the committer date of git's
/// HEAD as `git log -1 --format=%ct` prints it. `None` when neither is
/// available. A `SOURCE_DATE_EPOCH` that is not whole seconds is the message
/// the build fails with.
pub(crate) fn seconds(
    epoch: Option<&OsStr>,
    head: impl FnOnce() -> Option<String>,
) -> Result<Option<i64>, String> {
    match epoch.filter(|epoch| !epoch.is_empty()) {
        Some(epoch) => epoch
            .to_str()
            .and_then(|text| text.parse().ok())
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "SOURCE_DATE_EPOCH is \"{}\", which is not whole seconds since the Unix epoch; set it to digits or unset it",
                    epoch.to_string_lossy().escape_debug()
                )
            }),
        None => Ok(head().and_then(|text| text.trim().parse().ok())),
    }
}

/// `seconds` as a UTC timestamp, such as `2026-11-13T10:00:00Z`.
pub(crate) fn utc(seconds: i64) -> String {
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's days-to-civil algorithm, for the proleptic Gregorian
    // calendar.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let (hour, minute, second) = (rest / 3_600, rest % 3_600 / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}
