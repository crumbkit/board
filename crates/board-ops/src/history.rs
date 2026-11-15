//! `board get --at` and `board log`: reading a board at one commit, and the
//! commits that changed a path (docs/architecture/records/history.md).

use board_model::{Location, LogicalPath, ObjectId, Source};
use board_tree::{GitError, History, MAX_WALKED, Walk};

use crate::board::Board;
use crate::read::{GetReport, ReadError, display, read_at};

/// The commit of the board's own repository a read starts at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    Head,
    /// A full commit id.
    Commit(ObjectId),
}

/// What `log` found: the commits that changed what a path names, newest
/// first, walked from `at`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogReport {
    /// The commit of the board's own repository the walk started at.
    pub at: ObjectId,
    /// Where the path resolved in `at`, with every pointer followed.
    pub location: Location,
    pub display: LogicalPath,
    pub history: History,
}

impl Board {
    /// Each of `paths` as [`Board::get_each`] gives it, read in the tree
    /// of the commit `at` names instead of the working tree. A mount is
    /// read at the commit its pointer pins in that tree. An `at` that names
    /// no commit is the one error, for every path.
    pub fn get_each_at<'a>(
        &'a self,
        at: At,
        paths: &'a [LogicalPath],
    ) -> Result<impl Iterator<Item = Result<GetReport, ReadError>> + 'a, ReadError> {
        let commit = self.start(at)?.ok_or(ReadError::NoHead)?;
        let repositories = self.repositories();
        let mut view = (repositories.in_commit(repositories.root(), commit))
            .map_err(|source| ReadError::Git { source })?;
        Ok(paths.iter().map(move |path| {
            let location = {
                let _timed = self.timed("resolve");
                Self::located(path, |path| view.resolve_record(path))?
            };
            read_at(&mut view, location, path, true)
        }))
    }

    /// The commits that changed what `path` names, newest first, walked
    /// from the commit `at` names: at most `limit` when it is given. The
    /// path resolves in that commit with every pointer followed, and a path
    /// through a mount lists the mounted repository's commits from the
    /// commit the mount pins.
    pub fn log(
        &self,
        path: &LogicalPath,
        at: At,
        limit: Option<usize>,
    ) -> Result<LogReport, ReadError> {
        let git = |source: GitError| ReadError::Git { source };
        let start = self
            .start(at)?
            .ok_or_else(|| ReadError::NoHistory { path: path.clone() })?;
        let repositories = self.repositories();
        let mut view = (repositories.in_commit(repositories.root(), start)).map_err(git)?;
        let location = Self::located(path, |path| view.resolve(path))?;
        let Source::Commit(from) = location.board.source else {
            unreachable!("a resolution in a commit reads each mount at a commit")
        };
        let repository = repositories.repository(&location.board).map_err(git)?;
        let walk = Walk {
            limit,
            bound: MAX_WALKED,
        };
        let history = (repository.history(from, &location.path, walk)).map_err(git)?;
        Ok(LogReport {
            at: start,
            display: display(&location, path),
            location,
            history,
        })
    }

    /// The commit `at` names in the board's own repository, or `None` for
    /// HEAD before the first commit.
    fn start(&self, at: At) -> Result<Option<ObjectId>, ReadError> {
        let root = self.repositories().root();
        match at {
            At::Head => root.head(),
            At::Commit(id) => root.commit(id).map(Some),
        }
        .map_err(|source| ReadError::Git { source })
    }
}
