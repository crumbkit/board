//! The values the command line's flags take, parsed and checked.

use std::time::Duration;

use board_model::ObjectId;
use board_ops::{Expect, Removing, Unmatched};

/// What `board rm`'s `-r`, `--expect` and `--ignore-unmatch` ask it to do.
pub(crate) fn removal(
    recursive: bool,
    expect: Option<Expect>,
    ignore_unmatch: bool,
) -> (Removing, Unmatched) {
    let removing = if recursive {
        Removing::Beneath
    } else {
        Removing::File(expect.unwrap_or(Expect::Any))
    };
    let unmatched = if ignore_unmatch {
        Unmatched::Ignore
    } else {
        Unmatched::Refuse
    };
    (removing, unmatched)
}

pub(crate) fn expect(text: &str) -> Result<Expect, String> {
    if text == "none" {
        return Ok(Expect::Absent);
    }
    ObjectId::from_hex(text)
        .map(Expect::Blob)
        .ok_or_else(|| "must be `none` or a blob id in lowercase hex".to_owned())
}

/// `put` or `<plugin>.<capability>`.
pub(crate) fn op(text: &str) -> Result<String, String> {
    board_model::is_op(text)
        .then(|| text.to_owned())
        .ok_or_else(|| {
            "must be `put` or `<plugin>.<capability>`, such as review.comment".to_owned()
        })
}

/// A count of milliseconds, seconds, minutes or hours with its unit.
pub(crate) fn duration(text: &str) -> Result<Duration, String> {
    let count = text.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let scale = match &text[count.len()..] {
        "ms" => Some(1),
        "s" => Some(1_000),
        "m" => Some(60_000),
        "h" => Some(3_600_000),
        _ => None,
    };
    scale
        .zip(count.parse::<u64>().ok())
        .and_then(|(scale, count)| count.checked_mul(scale))
        .map(Duration::from_millis)
        .ok_or_else(|| "must be a whole number with a unit: 500ms, 30s, 2m or 1h".to_owned())
}

/// A JSON pointer, by RFC 6901: empty, or each token after a `/`, in which
/// `~` is followed by `0` or `1`.
pub(crate) fn pointer(text: &str) -> Result<String, String> {
    let mut escapes = text.split('~').skip(1);
    if (text.is_empty() || text.starts_with('/'))
        && escapes.all(|after| after.starts_with(['0', '1']))
    {
        Ok(text.to_owned())
    } else {
        Err("must be a JSON pointer, such as /status, or /a~1b for the key a/b: empty or starting with /, with ~ only as ~0 or ~1".to_owned())
    }
}
