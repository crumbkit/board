//! A host over a real board in a temporary repository, which can refuse
//! the next puts as scripted, and clones that record each sync and pin.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use board_model::{LogicalPath, Refusal};
use board_ops::{
    Absent, Board, DecideReport, Elsewhere, GetReport, Host, Limit, PutError, PutReport,
    QueryError, QueryReport, ReadError, RefsError, RefsReport, Settings, Shown, Variables,
};
use board_scenario::{Item, Scenario};
use serde_json::Value;

pub(crate) struct Local {
    pub(crate) scenario: Scenario,
    board: Board,
    settings: Settings,
    /// Refusals the next puts give in place of writing, in order; a put
    /// past them writes.
    pub(crate) refusing: RefCell<VecDeque<PutError>>,
    /// Each put's lines.
    pub(crate) puts: RefCell<Vec<Vec<Value>>>,
}

/// A board that commits on put, holding `files`.
pub(crate) fn local(files: &[(&str, &str)]) -> Local {
    let scenario = Scenario::new();
    let config = r#"{"$type": "board/config/v1"}"#;
    scenario.write(".board/board.json", Item::File(config));
    // A committing put needs an identity, which CI's git does not have.
    let identity = "[user]\n\tname = Board\n\temail = board@example.com\n";
    scenario.write(".git/config", Item::Appended(identity));
    for (path, text) in files {
        scenario.write(path, Item::File(text));
    }
    let board = Board::open(scenario.workdir(), None).unwrap();
    Local {
        scenario,
        board,
        settings: Settings {
            now: std::time::SystemTime::UNIX_EPOCH,
            day_start: None,
            caller: None,
            environment: Variables::default(),
            cache_home: None,
        },
        refusing: RefCell::default(),
        puts: RefCell::default(),
    }
}

impl Host for Local {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError> {
        assert_eq!(limit, Limit::ALL, "a listing reads every row");
        self.board.query(sql, usize::MAX, None, None)
    }
    fn refs(
        &self,
        _: &[LogicalPath],
        _: Absent,
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        panic!("board-repos resolves no refs")
    }
    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        Ok(self.board.get_each(paths)?.collect())
    }
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        self.puts.borrow_mut().push(lines.to_vec());
        if let Some(error) = self.refusing.borrow_mut().pop_front() {
            return Err(error);
        }
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        self.board.put_batch(&mut text.join("\n").as_bytes())
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        panic!("board-repos calls no plugin")
    }
    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        panic!("board-repos opens no other board")
    }
    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        panic!("board-repos asks the owner nothing")
    }
    fn sync(&self, _: bool, _: Duration) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("this host syncs nothing")
    }
    fn mount_update(
        &self,
        _: &LogicalPath,
        _: Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("this host updates no mount")
    }
    fn settings(&self) -> &Settings {
        &self.settings
    }
}

/// One call made of [`Recorded`] clones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Cloned {
    Sync { clone: String, push: bool },
    Pin(String),
}

/// Clones that record each call; a sync refuses with the next of
/// `syncs`, and a pin gives `head`.
#[derive(Default)]
pub(crate) struct Recorded {
    pub(crate) calls: RefCell<Vec<Cloned>>,
    pub(crate) syncs: RefCell<VecDeque<Refusal>>,
    pub(crate) head: String,
}

impl crate::Clones for Recorded {
    fn sync(&self, clone: &Path, push: bool, _: Duration) -> Result<(), Box<Refusal>> {
        let clone = clone.display().to_string();
        self.calls.borrow_mut().push(Cloned::Sync { clone, push });
        self.syncs
            .borrow_mut()
            .pop_front()
            .map_or(Ok(()), |refusal| Err(Box::new(refusal)))
    }
    fn mount_update(&self, path: &LogicalPath, _: Duration) -> Result<String, Box<Refusal>> {
        self.calls.borrow_mut().push(Cloned::Pin(path.to_string()));
        Ok(self.head.clone())
    }
}
