//! The snapshots one built-in plugin call keeps, so its reads share them
//! (docs/architecture/index.md). A call keeps at most one View snapshot,
//! for queries, and one Report snapshot, for `refs`, and a read-only
//! capability it calls in the same board, or a host it opens on the same
//! board, keeps the same ones. Another session's commit does not drop
//! them: each record a plugin call writes after reading it carries the
//! blob it read as its expect, so Board refuses a write over a newer
//! record as stale. A schema line takes no expect; Board writes a schema
//! only where the board lacks it.
//! Both are dropped before anything the call does that may change what
//! they show: a write, a nested call to a capability that does not say it
//! is read-only, and a process that runs in the board's own tree. After a
//! process elsewhere, or a read-only process plugin, the next read
//! compares HEAD once and drops them if it moved, since either may have
//! run `board put`. A process elsewhere, such as `git diff` in a mounted
//! clone, changes nothing else a snapshot reads, because a snapshot reads
//! each mount at the commit its mount record names. In a board that does
//! not commit on put, every read compares HEAD. The two snapshots are
//! taken lazily, so before one is taken while the other is held, HEAD is
//! compared once and the held one dropped if it moved; both then show one
//! board. A host opened on another board that writes back through a mount
//! into this board does not drop these snapshots.

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
    /// The HEAD read before a process or read-only process plugin ran;
    /// the next read drops both snapshots if HEAD is no longer it.
    doubt: Option<Head>,
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

    /// The snapshots of a host opened on `opened` from one acting on
    /// `called` with `memo`: the same ones when both are one board, so a
    /// put through either drops them, else new ones.
    pub(crate) fn opened(memo: &Rc<Self>, called: &Board, opened: &Board) -> Rc<Self> {
        if opened.repository().path() == called.repository().path() {
            Rc::clone(memo)
        } else {
            Rc::default()
        }
    }

    /// Records HEAD before something runs that may commit to the board and
    /// shows HEAD any put it made, so the next read compares HEAD with it.
    /// The earliest record stands until a read compares it.
    pub(crate) fn doubt(&self, board: &Board) {
        let head = head_of(board);
        self.0.borrow_mut().doubt.get_or_insert(head);
    }

    /// Drops both snapshots before a process runs in `directory`, unless
    /// it runs outside `board`'s tree (none is a new temporary directory)
    /// and `board` commits on put, so HEAD shows any put the process makes;
    /// then the next read compares HEAD. `directory` is canonicalized to
    /// compare with the canonical root.
    pub(crate) fn forget_running(&self, board: &Board, directory: Option<&Path>) {
        let root = board.repository().path();
        let outside = directory.is_none_or(|directory| {
            (directory.canonicalize()).is_ok_and(|directory| !directory.starts_with(root))
        });
        if outside && board.config().commit_on_put {
            self.doubt(board);
        } else {
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
        self.at(board, Wanted::Report);
        let kept = self.0.borrow_mut().report.take();
        let (answers, kept) = board.refs_kept(paths, absent, fields, cache, kept)?;
        self.0.borrow_mut().report = kept;
        Ok(answers)
    }

    /// Drops both snapshots when HEAD moved, compared only when it is in
    /// doubt: in a board that does not commit on put or whose HEAD was
    /// unreadable, after
    /// [`Memo::doubt`] against the HEAD it recorded, and against the held
    /// snapshot's HEAD before `wanted` is taken beside it. A HEAD that
    /// cannot be read matches nothing.
    fn at(&self, board: &Board, wanted: Wanted) {
        let mut kept = self.0.borrow_mut();
        let held = kept.view.is_some() || kept.report.is_some();
        let taking = match wanted {
            Wanted::View => kept.view.is_none(),
            Wanted::Report => kept.report.is_none(),
        };
        let doubt = kept.doubt.take();
        if !held {
            if taking {
                kept.head = head_of(board);
            }
            return;
        }
        let every = !board.config().commit_on_put || kept.head == Head::Unknown;
        if !(every || doubt.is_some() || taking) {
            return;
        }
        let now = head_of(board);
        let moved = |since: Head| now == Head::Unknown || since != now;
        let dropped = doubt.is_some_and(moved) || ((every || taking) && moved(kept.head));
        if dropped {
            *kept = Kept {
                head: now,
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
        self.at(board, Wanted::View);
        if let Some(view) = &self.0.borrow().view {
            return Ok(Rc::clone(view));
        }
        let view = Rc::new(take()?);
        self.0.borrow_mut().view = Some(Rc::clone(&view));
        Ok(view)
    }
}

/// The snapshot a read is about to use.
#[derive(Clone, Copy)]
enum Wanted {
    View,
    Report,
}

/// `board`'s HEAD, or [`Head::Unknown`] when it cannot be read.
fn head_of(board: &Board) -> Head {
    match board.repository().head() {
        Ok(Some(commit)) => Head::Commit(commit),
        Ok(None) => Head::Unborn,
        Err(_) => Head::Unknown,
    }
}
