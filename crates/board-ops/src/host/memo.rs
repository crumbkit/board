//! The snapshots one built-in plugin call keeps, so its reads share them
//! (docs/architecture/index.md). A call keeps at most one View snapshot,
//! for queries, and one Report snapshot, for `refs`, and a read-only
//! capability it calls in the same board keeps the same ones. Both are
//! dropped when the board's HEAD moves, and before anything the call does
//! that may change files HEAD does not show yet: a write, a nested call to a
//! capability that does not say it is read-only, and a process that runs in
//! the board's own tree. A process elsewhere, such as `git diff` in a
//! mounted clone, changes nothing a snapshot reads, because a snapshot
//! reads each mount at the commit its mount record names.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use board_index::{Fallback, IndexError, View};
use board_model::{LogicalPath, ObjectId};

use crate::Board;
use crate::query::{QueryError, QueryReport, retried};
use crate::refs::{Absent, RefsError, RefsReport};
use crate::snapshot::Snapshot;

/// A View snapshot's views, and why its index is in memory when it is.
type Viewed = Rc<(View, Option<Fallback>)>;

/// The snapshots one call keeps.
#[derive(Default)]
pub(crate) struct Memo(RefCell<Kept>);

#[derive(Default)]
struct Kept {
    head: Head,
    view: Option<Viewed>,
    report: Option<Snapshot>,
}

/// The HEAD the kept snapshots were taken at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Head {
    /// Nothing is kept, or HEAD could not be read, which matches nothing.
    #[default]
    Unknown,
    /// Before the first commit.
    Unborn,
    Commit(ObjectId),
}

impl Memo {
    /// Drops both snapshots.
    pub(crate) fn forget(&self) {
        *self.0.borrow_mut() = Kept::default();
    }

    /// Drops both snapshots before a process runs in `directory`, unless
    /// it runs outside `board`'s tree (none is a new temporary directory)
    /// and `board` commits on put, so HEAD shows any put the process makes.
    /// `directory` is canonicalized to compare with the canonical root.
    pub(crate) fn forget_running(&self, board: &Board, directory: Option<&Path>) {
        let root = board.repository().path();
        let outside = directory.is_none_or(|directory| {
            (directory.canonicalize()).is_ok_and(|directory| !directory.starts_with(root))
        });
        if !(outside && board.config().commit_on_put) {
            self.forget();
        }
    }

    /// Runs `sql` as [`Board::query`] does, through the kept View snapshot,
    /// taken on the first query. When another process pruned what it shows,
    /// it is dropped and the query runs again on a fresh one, three times
    /// in all.
    pub(crate) fn query(
        &self,
        board: &Board,
        cache: Option<&Path>,
        sql: &str,
        limit: usize,
    ) -> Result<QueryReport, QueryError> {
        retried(|| {
            let view = self.view(board, || board.decided(cache))?;
            let ran = board.run(&view.0, view.1, sql, limit, None);
            if let Err(QueryError::Index {
                source: IndexError::Pruned,
            }) = ran
            {
                self.0.borrow_mut().view = None;
            }
            ran
        })
    }

    /// Answers `paths` as [`Board::refs`] does, through the kept Report
    /// snapshot, taken when the first path that resolves needs it.
    pub(crate) fn refs(
        &self,
        board: &Board,
        cache: Option<&Path>,
        paths: &[LogicalPath],
        absent: Absent,
        fields: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        self.at(board);
        let kept = self.0.borrow_mut().report.take();
        let (answers, kept) = board.refs_kept(paths, absent, fields, cache, kept)?;
        self.0.borrow_mut().report = kept;
        Ok(answers)
    }

    /// Drops both snapshots unless they were taken at the board's HEAD. A
    /// HEAD that cannot be read matches nothing.
    fn at(&self, board: &Board) {
        let head = match board.repository().head() {
            Ok(Some(commit)) => Head::Commit(commit),
            Ok(None) => Head::Unborn,
            Err(_) => Head::Unknown,
        };
        let mut kept = self.0.borrow_mut();
        if head == Head::Unknown || kept.head != head {
            *kept = Kept {
                head,
                ..Kept::default()
            };
        }
    }

    /// The kept View snapshot, or the one `take` gives, kept.
    fn view(
        &self,
        board: &Board,
        take: impl FnOnce() -> Result<(View, Option<Fallback>), QueryError>,
    ) -> Result<Viewed, QueryError> {
        self.at(board);
        if let Some(view) = &self.0.borrow().view {
            return Ok(Rc::clone(view));
        }
        let view = Rc::new(take()?);
        self.0.borrow_mut().view = Some(Rc::clone(&view));
        Ok(view)
    }
}
