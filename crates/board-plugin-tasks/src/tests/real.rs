//! A host over a real Board: a board that mounts a notes clone at
//! `/repos/work`, whose queries, refs, reads, puts and pins are Board's.
//! It counts the queries and refs a call makes, and can stand another
//! writer in the way of the plugin's puts.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::time::{Duration, SystemTime};

use board_model::{LogicalPath, Object, Refusal};
use board_ops::{
    Absent, Board, DecideReport, Elsewhere, GetReport, Host, Limit, MountError, MountUpdateReport,
    PutError, PutReport, QueryError, QueryReport, ReadError, RefsError, RefsReport, Settings,
    Shown, SyncError, Synced, Variables,
};
use board_repos::Clones;
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

/// The schema the plugin ships, which the clone holds.
const SCHEMA: &str = include_str!("../../types/tasks/task/v2.schema.json");

/// What stands in the way of the plugin's puts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Race {
    /// Nothing.
    Alone,
    /// Before each of the plugin's first `n` puts, another Board puts a
    /// change of its own to the same file, through the same mount.
    Changes(usize),
    /// At the plugin's first put, the file is written and not committed,
    /// and the put is refused, as Board leaves a put whose HEAD another
    /// writer moved while it committed. Through the mount, HEAD moves past
    /// the file with a commit of another file.
    HeadMoves,
}

pub(crate) struct Real {
    pub(crate) board: Scenario,
    _remote: Scenario,
    pub(crate) clone: Scenario,
    settings: Settings,
    pub(crate) race: Race,
    pub(crate) queries: Cell<usize>,
    pub(crate) refs: Cell<usize>,
    pub(crate) puts: RefCell<Vec<Value>>,
}

/// The notes repo's files at its first commit, with `tasks`.
fn files(tasks: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut files = vec![
        (
            ".board/board.json".to_owned(),
            r#"{"$type": "board/config/v1"}"#.to_owned(),
        ),
        (
            ".board/types/tasks/task/v2.schema.json".to_owned(),
            SCHEMA.to_owned(),
        ),
        ("daily/.gitkeep".to_owned(), String::new()),
    ];
    files.extend((tasks.iter()).map(|(path, text)| ((*path).to_owned(), (*text).to_owned())));
    files
}

impl Real {
    /// The board, with `own` in its own tree and `tasks` in the mounted
    /// clone, each by path.
    pub(crate) fn new(race: Race, own: &[(&str, &str)], tasks: &[(&str, &str)]) -> Self {
        let remote = Scenario::new();
        let files = files(tasks);
        let pairs: Vec<(&str, &str)> = (files.iter())
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let started = remote.commit(&pairs);
        let clone = Scenario::clone_of(&remote);
        clone.identified();
        let board = Scenario::board();
        board
            .identified()
            .write(".board/types/tasks/task/v2.schema.json", Item::File(SCHEMA));
        let mount = json!({"$type": "board/mount/v1", "repo": clone.path(), "follow": "main", "commit": started.to_string()});
        board.write("repos/work.json", Item::File(&mount.to_string()));
        for (path, text) in own {
            board.write(path, Item::Fresh(text));
        }
        Self {
            board,
            _remote: remote,
            clone,
            settings: Settings {
                now: SystemTime::now(),
                day_start: None,
                utc_offset: 0,
                caller: None,
                environment: Variables::default(),
                cache_home: None,
            },
            race,
            queries: Cell::new(0),
            refs: Cell::new(0),
            puts: RefCell::default(),
        }
    }

    fn open(&self) -> Board {
        Board::open(self.board.workdir(), None).unwrap()
    }

    /// The file at `path` in the clone's working tree.
    pub(crate) fn text(&self, path: &str) -> String {
        std::fs::read_to_string(self.clone.workdir().join(path)).unwrap()
    }

    /// The file at `path` in the board's own working tree.
    pub(crate) fn own(&self, path: &str) -> String {
        std::fs::read_to_string(self.board.workdir().join(path)).unwrap()
    }

    /// Puts `lines` as a second Board would.
    fn batch(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        self.open().put_batch(&mut text.join("\n").as_bytes())
    }

    /// Another writer's change to the file at `path`, put through Board.
    fn change(&self, path: &str) {
        let logical: LogicalPath = path.parse().unwrap();
        let report = self
            .open()
            .get_each(&[logical])
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let Object::Blob(blob) = report.location.object else {
            panic!("{path} is a file")
        };
        let text = String::from_utf8(report.bytes).unwrap() + "Theirs.\n";
        let line = json!({"path": path, "expect": blob.to_string(), "text": text});
        self.batch(&[line]).unwrap();
    }

    /// Leaves `text` at `path` uncommitted in the clone, and moves HEAD
    /// past it with a commit of another file.
    fn head_moves(&self, path: &str, text: &str) {
        let Some(file) = path.strip_prefix("/repos/work/") else {
            self.board
                .write(path.trim_start_matches('/'), Item::Fresh(text));
            return;
        };
        let committed = self.text(file);
        self.clone.write(file, Item::Fresh(text));
        let head = self.clone.head().unwrap();
        let mut tree: Vec<(String, String)> = (head.files.into_iter())
            .map(|(path, bytes)| (path, String::from_utf8(bytes).unwrap()))
            .collect();
        for (at, held) in &mut tree {
            if at == file {
                held.clone_from(&committed);
            }
        }
        tree.push(("other.txt".to_owned(), "Another writer's.\n".to_owned()));
        let pairs: Vec<(&str, &str)> = (tree.iter())
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        self.clone.commit(&pairs);
    }
}

impl Host for Real {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError> {
        assert_eq!(limit, Limit::ALL);
        self.queries.set(self.queries.get() + 1);
        self.open().query(sql, usize::MAX, None, None)
    }
    fn refs(
        &self,
        paths: &[LogicalPath],
        absent: Absent,
        fields: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        self.refs.set(self.refs.get() + 1);
        Ok(self
            .open()
            .refs(paths, absent, fields, None, None)?
            .collect())
    }
    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        Ok(self.open().get_each(paths)?.collect())
    }
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        let path = lines[0]["path"].as_str().unwrap().to_owned();
        let count = self.puts.borrow().len();
        self.puts.borrow_mut().push(lines[0].clone());
        match self.race {
            Race::Changes(times) if count < times => self.change(&path),
            Race::HeadMoves if count == 0 => {
                self.head_moves(&path, lines[0]["text"].as_str().unwrap());
                return Err(PutError::HeadMoved { writes: Vec::new() });
            }
            _ => {}
        }
        self.batch(lines)
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        panic!("tasks calls no plugin")
    }
    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        Ok(Box::new(Cloned(self)))
    }
    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        panic!("tasks asks the owner nothing")
    }
    fn sync(&self, _: bool, _: Duration) -> Result<Synced, SyncError> {
        panic!("a call through this host syncs through `Pinning`")
    }
    fn mount_update(
        &self,
        path: &LogicalPath,
        deadline: Duration,
    ) -> Result<MountUpdateReport, MountError> {
        self.open().mount_update(path, deadline)
    }
    fn settings(&self) -> &Settings {
        &self.settings
    }
}

/// The clones of a [`Real`] board: a sync finds nothing to do, since the
/// clone's remote holds nothing new, and a pin is Board's.
pub(crate) struct Pinning<'a>(pub(crate) &'a Real);

impl Clones for Pinning<'_> {
    fn sync(&self, _: &Path, _: bool, _: Duration) -> Result<(), Box<Refusal>> {
        Ok(())
    }

    fn mount_update(&self, path: &LogicalPath, deadline: Duration) -> Result<String, Box<Refusal>> {
        (self.0.mount_update(path, deadline))
            .map(|report| report.new.to_string())
            .map_err(|error| Box::new(error.refusal()))
    }

    fn conflicts(&self) -> Vec<board_ops::Conflicted> {
        Vec::new()
    }
}

/// The host a call gets for the notes clone: it only syncs, as `board -C
/// <clone> sync` does.
struct Cloned<'a>(&'a Real);

impl Host for Cloned<'_> {
    fn query(&self, _: &str, _: Limit) -> Result<QueryReport, QueryError> {
        panic!("only a sync is asked of the clone")
    }
    fn refs(
        &self,
        _: &[LogicalPath],
        _: Absent,
        _: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        panic!("only a sync is asked of the clone")
    }
    fn get_each(&self, _: &[LogicalPath]) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        panic!("only a sync is asked of the clone")
    }
    fn put(&self, _: &[Value]) -> Result<PutReport, PutError> {
        panic!("only a sync is asked of the clone")
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        panic!("only a sync is asked of the clone")
    }
    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        panic!("only a sync is asked of the clone")
    }
    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        panic!("only a sync is asked of the clone")
    }
    fn sync(&self, push: bool, deadline: Duration) -> Result<Synced, SyncError> {
        self.sync_waiting(push, deadline, Duration::ZERO)
    }
    fn sync_waiting(
        &self,
        push: bool,
        deadline: Duration,
        wait: Duration,
    ) -> Result<Synced, SyncError> {
        let mut board = Board::open(self.0.clone.workdir(), None).unwrap();
        board.built_by("test");
        board.sync_waiting(push, deadline, wait)
    }
    fn mount_update(&self, _: &LogicalPath, _: Duration) -> Result<MountUpdateReport, MountError> {
        panic!("only a sync is asked of the clone")
    }
    fn settings(&self) -> &Settings {
        &self.0.settings
    }
}
