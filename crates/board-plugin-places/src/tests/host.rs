//! A host over a real Board whose queries, reads and puts are Board's, and
//! whose processes are answers a test sets: mxv, scutil and hostname print
//! what the test gives them, and a program it gives nothing is not on PATH.
//! It records each process it runs, each put and each warning.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use board_model::{LogicalPath, Refusal};
use board_ops::{
    Absent, Board, DecideReport, Elsewhere, GetReport, Host, Limit, MountError, MountUpdateReport,
    PutError, PutReport, QueryError, QueryReport, ReadError, RefsError, RefsReport, Run, Settings,
    Shown, SyncError, Synced, Variables,
};
use board_plugin::child::{Exited, ProcessError};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::Places;
use board_ops::Plugin;

/// The schema of a change, which the changes plugin ships.
const CHANGE_SCHEMA: &str =
    include_str!("../../../board-plugin-changes/types/changes/change/v2.schema.json");

/// How a process the host runs ends.
#[derive(Clone, Debug)]
pub(crate) enum Answer {
    Exits {
        code: i32,
        stdout: String,
        stderr: String,
    },
    Missing,
}

impl Answer {
    pub(crate) fn printing(stdout: &str) -> Self {
        Self::Exits {
            code: 0,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }
}

pub(crate) struct Real {
    pub(crate) board: Scenario,
    settings: Settings,
    answers: RefCell<BTreeMap<String, Answer>>,
    pub(crate) runs: RefCell<Vec<String>>,
    pub(crate) puts: RefCell<Vec<Value>>,
    pub(crate) warnings: RefCell<Vec<String>>,
}

/// The time every join here is written at.
pub(crate) const SINCE: &str = "2026-11-20T09:00:00Z";

/// The machine every host here is, unless a test says otherwise.
pub(crate) const MACHINE: &str = "quokka";

impl Real {
    /// A board with the places and change schemas, on the machine
    /// `quokka`, called from no directory.
    pub(crate) fn new() -> Self {
        let board = Scenario::board();
        board.identified();
        let (record_type, schema) = Places.schemas().remove(0);
        board
            .write(
                &format!(".board/types/{}.schema.json", record_type.as_str()),
                Item::File(board_model::canonical_json(&schema).as_str()),
            )
            .write(
                ".board/types/changes/change/v2.schema.json",
                Item::File(CHANGE_SCHEMA),
            );
        Self {
            board,
            settings: Settings {
                now: board_model::moment(SINCE).unwrap(),
                day_start: None,
                utc_offset: 0,
                caller: None,
                environment: Variables(vec![("BOARD_MACHINE".to_owned(), MACHINE.into())]),
                cache_home: None,
            },
            answers: RefCell::default(),
            runs: RefCell::default(),
            puts: RefCell::default(),
            warnings: RefCell::default(),
        }
    }

    /// This host with only `variables` among the variables a process sees.
    pub(crate) fn environment(mut self, variables: &[(&str, &str)]) -> Self {
        self.settings.environment = Variables(
            (variables.iter())
                .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
                .collect(),
        );
        self
    }

    /// This host called from `directory`.
    pub(crate) fn caller(mut self, directory: PathBuf) -> Self {
        self.settings.caller = Some(directory);
        self
    }

    /// This host at `now`.
    pub(crate) fn at(mut self, now: SystemTime) -> Self {
        self.settings.now = now;
        self
    }

    /// Answers `command`, a program and its arguments joined by spaces.
    pub(crate) fn answer(&self, command: &str, answer: Answer) {
        self.answers.borrow_mut().insert(command.to_owned(), answer);
    }

    /// Answers both of mxv's listings: `live` rows and `archived` ones.
    pub(crate) fn mxv(&self, live: &[Value], archived: &[Value]) {
        self.answer(LIVE, Answer::printing(&listing(live)));
        self.answer(ARCHIVED, Answer::printing(&listing(archived)));
    }

    /// A change record at `path` in `phase`.
    pub(crate) fn change(&self, path: &str, phase: &str) {
        let record = json!({"$type": "changes/change/v2", "title": format!("Work on {path}"), "phase": phase});
        self.board.write(
            &format!("{}.json", path.trim_start_matches('/')),
            Item::File(&record.to_string()),
        );
    }

    /// The record at `path`, or none.
    pub(crate) fn record(&self, path: &str) -> Option<Value> {
        let file = self
            .board
            .workdir()
            .join(format!("{}.json", path.trim_start_matches('/')));
        let text = std::fs::read_to_string(file).ok()?;
        Some(serde_json::from_str(&text).unwrap())
    }

    /// Writes `record` at `path` as another writer would.
    pub(crate) fn write(&self, path: &str, record: &Value) {
        self.board.write(
            &format!("{}.json", path.trim_start_matches('/')),
            Item::File(&record.to_string()),
        );
    }

    fn open(&self) -> Board {
        Board::open(self.board.workdir(), None).unwrap()
    }
}

/// `mxv list` as the plugin runs it.
pub(crate) const LIVE: &str = "mxv list --all --json --fields name,id,path";
/// `mxv list --archived` as the plugin runs it.
pub(crate) const ARCHIVED: &str = "mxv list --archived --all --json --fields name,id,path";

/// An `mxv/list/v1` document holding `rows`.
pub(crate) fn listing(rows: &[Value]) -> String {
    json!({"schema": "mxv/list/v1", "data": rows, "total": rows.len(), "truncated": false})
        .to_string()
}

/// A row of `mxv list --fields name,id,path`.
pub(crate) fn row(name: &str, path: &str, id: Option<&str>) -> Value {
    json!({"name": name, "id": id, "path": path})
}

/// A 32-digit id made of `digit`.
pub(crate) fn id(digit: char) -> String {
    std::iter::repeat_n(digit, 32).collect()
}

impl Host for Real {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError> {
        assert_eq!(limit, Limit::ALL);
        self.open().query(sql, usize::MAX, None, None)
    }
    fn refs(
        &self,
        _: &[LogicalPath],
        _: Absent,
        _: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        panic!("places resolves no references")
    }
    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        Ok(self.open().get_each(paths)?.collect())
    }
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        self.puts.borrow_mut().extend(lines.iter().cloned());
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        self.open().put_batch(&mut text.join("\n").as_bytes())
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        panic!("places calls no plugin")
    }
    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        panic!("places opens no other board")
    }
    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        panic!("places asks the owner nothing")
    }
    fn sync(&self, _: bool, _: Duration) -> Result<Synced, SyncError> {
        panic!("places syncs nothing")
    }
    fn mount_update(&self, _: &LogicalPath, _: Duration) -> Result<MountUpdateReport, MountError> {
        panic!("places updates no mount")
    }
    fn settings(&self) -> &Settings {
        &self.settings
    }
    fn warn(&self, refusal: &Refusal) {
        let line = format!("{}: {}", refusal.code.as_str(), refusal.message);
        let mut warnings = self.warnings.borrow_mut();
        if !warnings.contains(&line) {
            warnings.push(line);
        }
    }
    fn run_hiding(&self, run: &Run<'_>, hidden: &[&str]) -> Result<Exited, ProcessError> {
        assert_eq!(hidden, [] as [&str; 0]);
        assert_eq!(run.deadline, Duration::from_secs(10));
        let command = std::iter::once(run.program)
            .chain(run.arguments.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        self.runs.borrow_mut().push(command.clone());
        let answer = self.answers.borrow().get(&command).cloned();
        match answer.unwrap_or(Answer::Missing) {
            Answer::Exits {
                code,
                stdout,
                stderr,
            } => Ok(Exited {
                code: Some(code),
                stdout: stdout.into_bytes(),
                stderr: stderr.into_bytes(),
            }),
            Answer::Missing => Err(ProcessError::NotStarted {
                source: io::Error::from(io::ErrorKind::NotFound),
            }),
        }
    }
}
