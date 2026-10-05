//! How much of a listing is shown (STANDARDS.md 23).

use board_ops::MAX_LIMIT;
use serde_json::{Value, json};

/// The entries of a listing that are shown, out of how many there are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shown {
    pub(crate) shown: usize,
    pub(crate) total: usize,
}

impl Shown {
    /// The first `limit` of `total` entries, or all of them when `limit` is
    /// `None`.
    pub(crate) fn of(total: usize, limit: Option<usize>) -> Self {
        Self {
            shown: limit.map_or(total, |limit| limit.min(total)),
            total,
        }
    }

    pub(crate) fn truncated(self) -> bool {
        self.shown < self.total
    }

    /// The note for stderr: how many `entries` were left out and how to see
    /// them, or `empty` when there were none at all. Nothing when every
    /// entry is shown.
    pub(crate) fn note(self, entries: &str, empty: &str) -> String {
        if self.total == 0 {
            format!("note: {empty}\n")
        } else if self.truncated() {
            format!(
                "note: showing {} of {} {entries}; --limit <n> (at most {MAX_LIMIT}) or --all shows more\n",
                self.shown, self.total
            )
        } else {
            String::new()
        }
    }

    /// Adds `total` and `truncated` to a JSON report.
    pub(crate) fn mark(self, mut report: Value) -> Value {
        report["total"] = json!(self.total);
        report["truncated"] = json!(self.truncated());
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_within_its_limit_is_whole_and_says_nothing() {
        let shown = Shown::of(100, Some(100));
        assert_eq!(
            shown,
            Shown {
                shown: 100,
                total: 100
            }
        );
        assert!(!shown.truncated());
        assert_eq!(shown.note("entries", "none"), "");
    }

    #[test]
    fn a_listing_past_its_limit_says_how_many_it_left_out() {
        let shown = Shown::of(101, Some(100));
        assert!(shown.truncated());
        assert_eq!(
            shown.note("entries", "none"),
            "note: showing 100 of 101 entries; --limit <n> (at most 10000) or --all shows more\n"
        );
    }

    #[test]
    fn an_unlimited_listing_shows_everything_and_an_empty_one_says_so() {
        assert!(!Shown::of(20_000, None).truncated());
        assert_eq!(
            Shown::of(0, Some(5)).note("entries", "the directory is empty"),
            "note: the directory is empty\n"
        );
    }
}
