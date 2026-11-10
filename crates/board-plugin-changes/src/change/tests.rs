//! What a change's references read as when the tree stops one of them, such
//! as a reference beneath a mount whose repository is gone.

use board_model::{Code, LogicalPath};
use board_ops::{Host, Limit, QueryError, QueryReport, ReadError, RefsError, Settings};
use board_tree::ResolveError;

use crate::change::{NOT_FOUND, referenced, referenced_in_walk};

pub(crate) const STOPPED: &str = "/repos/gone/tasks/t.md";
/// A path whose repository git cannot read, which every path meets.
pub(crate) const GIT_FAILS: &str = "/repos/failing/tasks/t.md";
const ABSENT: &str = "/tasks/absent.md";

/// A host whose index holds no entries, and whose tree stops [`STOPPED`] at
/// a mount whose repository is gone, fails in git at [`GIT_FAILS`], and
/// finds nothing at any other path.
pub(crate) struct Stopping(Settings);

impl Host for Stopping {
    fn query(&self, _sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        Ok(QueryReport {
            columns: vec![
                "logical".into(),
                "type".into(),
                "code".into(),
                "body".into(),
            ],
            rows: Vec::new(),
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn refs(
        &self,
        paths: &[LogicalPath],
        _absent: board_ops::Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, RefsError>>, RefsError> {
        Ok((paths.iter())
            .map(|path| {
                let source = if path.as_str() == STOPPED {
                    ResolveError::RepoMissing {
                        path: "/repos/gone".parse().unwrap(),
                        repo: "/gone".to_owned(),
                    }
                } else if path.as_str() == GIT_FAILS {
                    ResolveError::Git(board_tree::GitError::NoWorkingTree {
                        repository: "/failing".into(),
                    })
                } else {
                    ResolveError::NotFound {
                        path: path.clone(),
                        ancestor: LogicalPath::root(),
                    }
                };
                Err(RefsError::Read {
                    source: ReadError::Resolve { source },
                })
            })
            .collect())
    }

    fn get_each(
        &self,
        _paths: &[LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, ReadError>>, ReadError> {
        unreachable!("referenced reads no files")
    }

    fn put(
        &self,
        _lines: &[serde_json::Value],
    ) -> Result<board_ops::PutReport, board_ops::PutError> {
        unreachable!("referenced writes nothing")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &serde_json::Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        unreachable!("referenced calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        unreachable!("referenced opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        unreachable!("referenced asks the owner nothing")
    }

    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("referenced syncs nothing")
    }

    fn mount_update(
        &self,
        _: &LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("referenced updates no mount")
    }

    fn settings(&self) -> &Settings {
        &self.0
    }
}

pub(crate) fn host() -> Stopping {
    Stopping(Settings {
        now: std::time::SystemTime::UNIX_EPOCH,
        day_start: None,
        utc_offset: 0,
        caller: None,
        environment: board_ops::Variables::default(),
        cache_home: None,
    })
}

#[test]
fn a_walk_passes_over_a_reference_the_tree_stopped_with_the_code_that_stopped_it() {
    let refs = [STOPPED.to_owned(), ABSENT.to_owned()];
    let found = referenced_in_walk(&host(), &refs).unwrap();
    assert_eq!(
        found.unreadable,
        vec![
            (
                STOPPED.to_owned(),
                Code::MountRepoMissing.as_str().to_owned()
            ),
            (ABSENT.to_owned(), NOT_FOUND.to_owned()),
        ]
    );
}

#[test]
fn one_changes_reference_the_tree_stopped_refuses_and_one_that_reaches_nothing_is_not_found() {
    let refused = referenced(&host(), &[STOPPED.to_owned()]).unwrap_err();
    assert_eq!(refused.code, "changes::board::refused");
    let found = referenced(&host(), &[ABSENT.to_owned()]).unwrap();
    assert_eq!(
        found.unreadable,
        vec![(ABSENT.to_owned(), NOT_FOUND.to_owned())]
    );
}
