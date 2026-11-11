//! Which of the mounts that hold a directory a run uses
//! (docs/architecture/paths/places.md).

use board_model::{LogicalPath, Source};

use super::{Matched, Place};
use crate::walk::{Entry, Status};
use crate::worktree::Reader;

/// How a place stands to the checkout it holds, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Choice {
    /// A mount whose `follow` is the branch the checkout is on.
    Followed,
    /// A mount beneath no record, such as `/repos/lib`.
    Standing,
    /// A mount beneath a record, such as a change's own, or a repo record.
    Other,
}

impl Place {
    /// What places are ordered by, in one board and across boards; the
    /// smallest is used.
    pub fn rank(&self) -> (Choice, Matched) {
        (self.choice, self.matched)
    }
}

/// The places of the smallest rank among `places`.
pub(super) fn best(places: Vec<Place>) -> Vec<Place> {
    let least = places.iter().map(Place::rank).min();
    (places.into_iter())
        .filter(|place| Some(place.rank()) == least)
        .collect()
}

/// The branch HEAD names in `git`, or `None` when HEAD is detached or
/// cannot be read.
pub(super) fn branch(git: &gix::Repository) -> Option<String> {
    Some(git.head_name().ok()??.shorten().to_string())
}

/// How the mount at `at`, which follows `follow`, stands to a checkout on
/// `branch`, or `None` when it is beneath a record among `entries`, the
/// working tree's, whose `phase` is `done` or `dropped`. A record that
/// cannot be read or has no `phase` counts as live.
pub(super) fn of(
    entries: &[Entry],
    reader: &mut Reader<'_>,
    at: &LogicalPath,
    follow: &str,
    branch: Option<&str>,
) -> Option<Choice> {
    let mut standing = true;
    let mut above = at.parent();
    while above != LogicalPath::root() {
        let records = (entries.iter()).filter(|entry| entry.logical.as_ref() == Some(&above));
        for record in records {
            if ended(reader, record) {
                return None;
            }
            standing = false;
        }
        above = above.parent();
    }
    let short = follow.strip_prefix("refs/heads/").unwrap_or(follow);
    Some(if branch == Some(short) {
        Choice::Followed
    } else if standing {
        Choice::Standing
    } else {
        Choice::Other
    })
}

/// Whether `record`'s `phase` is `done` or `dropped`.
fn ended(reader: &mut Reader<'_>, record: &Entry) -> bool {
    let mut phase = || {
        let blob = record.blob.filter(|_| record.status == Status::Ok)?;
        let read = reader.read(&Source::WorkingTree, &record.path, blob);
        let value = board_model::parse_json(&read.ok()??.bytes).ok()?;
        Some(matches!(value.get("phase")?.as_str()?, "done" | "dropped"))
    };
    phase().unwrap_or(false)
}
