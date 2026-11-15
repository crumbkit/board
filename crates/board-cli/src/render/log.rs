//! `log`: the commits that changed a path.

use std::fmt;

use board_ops::LogReport;
use crumbkit::help::Leaf;
use crumbkit::{Document, Line, Listing, Row};
use jiff::Timestamp;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::kit::Printer;
use super::location;
use crate::report::Output;

/// `board log --help` after its options.
pub(crate) const LOG_HELP: &str = "\
The walk follows merges as `git log -- <path>` does, not renames. Through a
mount it lists the mounted repository's commits, which get --at refuses,
not pin moves; log the mount record's directory for those.

--json prints board/log-report/v2. `truncated` means older rows may exist.
`summary.complete` is false past 100000 commits or at a shallow one.

Examples:
  board log /tasks/a
  board log /tasks/a --limit 1 --json";

/// `log`'s rows and summary: as text, one line a commit.
pub(crate) fn log(printer: Printer, report: &LogReport) -> Output {
    let history = &report.history;
    let rows = (history.changes.iter())
        .map(|change| Version {
            commit: change.commit.to_string(),
            time: utc(change.time),
            subject: change.subject.clone(),
            blob: change.blob.map(|blob| blob.to_string()),
        })
        .collect();
    let summary = Reach {
        at: report.at.to_string(),
        walked: history.walked,
        complete: history.complete(),
        location: location(&report.location, &report.display.to_string()),
    };
    let mut listing = Listing::uncounted(rows, history.truncated())
        .bounded(board_ops::MAX_LIMIT)
        .described("commits")
        .singular("commit")
        .summary(summary);
    if !history.complete() {
        listing = listing.note(format!(
            "note: the walk stopped after {} commits, at its bound or a shallow commit; older commits are out of its reach, so run log again with --at set to an older commit",
            history.walked
        ));
    }
    printer.print(LOG, Ok(listing))
}

/// `log`'s document. crumbkit's `truncated` says rows were left out; here
/// it says older changes may exist, which the description states.
const LOG: Document<Listing<Version, Reach>> = Document::new(
    "board",
    "log-report",
    2,
    "The commits that changed a path, newest first. `truncated` is true when the walk stopped before the end of the history, at its limit, its bound or a shallow commit, so older changes may exist.",
)
.when_empty("no commit changed the path");

pub(super) const LEAF: Leaf = Leaf::new(&["log"]).document(LOG);

/// One commit that changed the path.
#[derive(Serialize, JsonSchema)]
struct Version {
    /// The commit's full id.
    commit: String,
    /// The committer time in UTC, as RFC 3339 with seconds.
    time: String,
    /// The first line of the commit's message, at most 256 bytes; empty for an empty message.
    subject: String,
    /// The file at the path in that commit, or null when the commit removed it or the path is a directory there.
    blob: Option<String>,
}

impl Row for Version {
    const COLUMNS: &'static [&'static str] = &["commit", "time", "subject", "blob"];
    const LINE: Option<Line<Self>> = Some(Line::new("<COMMIT> <TIME> <SUBJECT>", |row: &Self| {
        Some(format!("{} {} {}", row.commit, row.time, row.subject))
    }));
}

/// How the walk went, under `summary`. The text prints no line for it.
#[derive(Serialize, JsonSchema)]
struct Reach {
    /// The commit of the board's own repository the walk started at.
    at: String,
    /// How many commits the walk read.
    walked: usize,
    /// False when older commits were out of the walk's reach: at its bound of 100000 commits, or at a shallow commit.
    complete: bool,
    /// Where the path resolved in the start commit.
    #[schemars(with = "Value")]
    location: board_types::Location,
}

impl fmt::Display for Reach {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

/// `seconds` since the Unix epoch in UTC, as RFC 3339 with seconds, held to
/// the years jiff can write.
fn utc(seconds: i64) -> String {
    let held = seconds.clamp(Timestamp::MIN.as_second(), Timestamp::MAX.as_second());
    Timestamp::from_second(held)
        .expect("a second within jiff's range is a timestamp")
        .to_string()
}
