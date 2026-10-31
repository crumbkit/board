//! `board put --new <folder>/`: a record named for the put's UTC minute and
//! four random hex digits (docs/architecture/paths/writes.md).

use std::hash::{BuildHasher, Hasher, RandomState};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use board_model::LogicalPath;

use super::{Expect, PutError, PutReport};
use crate::board::Board;

/// How many more names a put draws after the one it drew is taken.
pub(crate) const REDRAWS: usize = 3;

/// What draws each name a `--new` put tries.
pub(crate) type Draw<'a> = &'a mut dyn FnMut() -> String;

impl Board {
    /// Writes the record `bytes` at a new name under `folder`, as a put
    /// that expects no file there.
    pub fn put_new_waiting(
        &self,
        folder: &LogicalPath,
        bytes: &[u8],
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        self.put_named(folder, bytes, wait, &mut drawn())
    }

    /// [`Board::put_new_waiting`] with the names `draw` gives.
    pub(crate) fn put_named(
        &self,
        folder: &LogicalPath,
        bytes: &[u8],
        wait: Duration,
        draw: Draw<'_>,
    ) -> Result<PutReport, PutError> {
        let stale = |error: &PutError| matches!(error, PutError::Stale { .. });
        let put = || self.put_waiting(&under(folder, draw)?, Expect::Absent, bytes, wait);
        match redrawing(put, stale) {
            Err(error) if stale(&error) => Err(taken(folder)),
            put => put,
        }
    }
}

/// A name `draw` gives, under `folder`. Only a folder within a name's
/// length of the longest path is refused, with its code.
pub(crate) fn under(folder: &LogicalPath, draw: Draw<'_>) -> Result<LogicalPath, PutError> {
    (folder.join(&draw())).map_err(|_| PutError::TooDeep {
        folder: folder.clone(),
    })
}

/// `put`'s result, tried up to [`REDRAWS`] more times while `taken` finds
/// that its error is only names already taken.
pub(crate) fn redrawing(
    mut put: impl FnMut() -> Result<PutReport, PutError>,
    taken: impl Fn(&PutError) -> bool,
) -> Result<PutReport, PutError> {
    let mut last = put();
    for _ in 0..REDRAWS {
        match &last {
            Err(error) if taken(error) => last = put(),
            _ => break,
        }
    }
    last
}

/// The refusal of a put under `folder` whose every name was taken.
pub(crate) fn taken(folder: &LogicalPath) -> PutError {
    PutError::NameTaken {
        folder: folder.clone(),
        tries: REDRAWS + 1,
    }
}

/// `given` without its one trailing `/`, or `/` for the root.
pub fn folder_text(given: &str) -> &str {
    match given.strip_suffix('/') {
        Some("") => "/",
        Some(folder) => folder,
        None => given,
    }
}

/// Names for a put made now: its UTC minute, and four hex digits drawn
/// afresh for each name.
pub(crate) fn drawn() -> impl FnMut() -> String {
    let seconds = (SystemTime::now().duration_since(UNIX_EPOCH)).map_or(0, |since| since.as_secs());
    move || {
        // `RandomState` keys come from the OS once per thread, then from a
        // counter. The suffix need not resist guessing; a taken one is redrawn.
        let [high, low, ..] = RandomState::new().build_hasher().finish().to_be_bytes();
        name(seconds, u16::from_be_bytes([high, low]))
    }
}

/// `YYYYMMDDTHHMMZ-xxxx`: the UTC minute `seconds` after 1970 falls in,
/// then `suffix` as four lowercase hex digits.
pub(crate) fn name(seconds: u64, suffix: u16) -> String {
    let (year, month, day) = civil(seconds / 86_400);
    let (hour, minute) = (seconds % 86_400 / 3_600, seconds % 3_600 / 60);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}Z-{suffix:04x}")
}

/// The date `days` after 1970-01-01 in the proleptic Gregorian calendar,
/// by Howard Hinnant's `civil_from_days`.
fn civil(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let of_era = shifted - era * 146_097;
    let year_of_era = (of_era - of_era / 1_460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (era * 400 + year_of_era + u64::from(month <= 2), month, day)
}
