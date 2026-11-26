//! The field of a review report that a command-line `board put` fills when
//! the writer leaves it out: `patch_ids`, from the commits of its `range`
//! in the checkout the put runs in, as `review.run` records them.

use std::path::Path;

use board_ops::{Fill, Settings, With};
use board_plugin::shape::Record;
use serde_json::{Map, Value, json};

use crate::git;
use crate::records::ReportRecord;

pub(crate) static FILLS: [Fill; 1] = [Fill {
    record_type: ReportRecord::TYPE,
    field: "patch_ids",
    with: With::Derived(patch_ids),
}];

/// The patch id of each commit in the report's `range`, oldest first, read
/// in the directory the put started in; nothing when the report has no
/// range of two commits; or why the commits cannot be read.
fn patch_ids(report: &Map<String, Value>, settings: &Settings) -> Result<Option<Value>, String> {
    let Some((base, head)) =
        (report.get("range").and_then(Value::as_str)).and_then(|range| range.split_once(".."))
    else {
        return Ok(None);
    };
    let Some(checkout) = settings.caller.as_deref() else {
        return Err("the put has no checkout to read the range's commits in".to_owned());
    };
    covered(settings, checkout, base, head).map(|ids| Some(json!(ids)))
}

/// What a report of `base..head` covers in the repository at `checkout`,
/// or why it cannot be read, naming a commit the checkout lacks.
fn covered(
    settings: &Settings,
    checkout: &Path,
    base: &str,
    head: &str,
) -> Result<Vec<String>, String> {
    let repository = git::with(settings, checkout);
    for commit in [base, head] {
        repository.commit(commit).map_err(|error| error.message)?;
    }
    (repository.covered(base, head)).map_err(|error| error.message)
}

#[cfg(test)]
mod tests;
