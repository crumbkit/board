//! Stop points for the kill tests. A test build reads `BOARD_TEST_STOP`
//! and aborts the process at the stop point it names, which leaves the disk
//! as a SIGKILL there would, lock files included. A release build contains
//! no stop point: [`at`] is empty.

/// A place in a put or a checkout where a kill test may stop the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The checkout record is written and no file has been.
    Record,
    /// The first changed file is written or removed.
    FirstFile,
    /// The last changed file is written or removed.
    LastFile,
    /// `index.lock` is held and the branch lock is not.
    IndexLock,
    /// Both locks are held and the index is not yet replaced.
    BranchLock,
    /// The index is renamed into place and the branch has not moved.
    IndexRenamed,
    /// The branch has moved and the checkout record is not yet removed.
    BranchMoved,
}

#[cfg(test)]
impl Stop {
    /// Every stop point, in the order a checkout reaches them.
    pub(crate) const ALL: [Self; 7] = [
        Self::Record,
        Self::FirstFile,
        Self::LastFile,
        Self::IndexLock,
        Self::BranchLock,
        Self::IndexRenamed,
        Self::BranchMoved,
    ];

    /// The value of `BOARD_TEST_STOP` that stops here.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Record => "record",
            Self::FirstFile => "first-file",
            Self::LastFile => "last-file",
            Self::IndexLock => "index-lock",
            Self::BranchLock => "branch-lock",
            Self::IndexRenamed => "index-renamed",
            Self::BranchMoved => "branch-moved",
        }
    }
}

/// Aborts the process when `BOARD_TEST_STOP` names `stop`.
#[cfg(test)]
pub(crate) fn at(stop: Stop) {
    if std::env::var("BOARD_TEST_STOP").is_ok_and(|name| name == stop.name()) {
        std::process::abort();
    }
}

/// A release build has no stop points.
#[cfg(not(test))]
#[inline(always)]
pub(crate) fn at(_: Stop) {}
