//! Plugins built into Board, and what Board offers them
//! (docs/plugins/built-in.md).

use std::ffi::OsString;
use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use board_model::{CapabilityName, Code, Fields, LogicalPath, PluginName, RecordType, Refusal};
use board_plugin::{CallReport, Manifest, Target};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::effects::{Asker, DecideReport, Shown};
use crate::known::{self, KnownBoards};
use crate::plugins::Plugins;
use crate::put::{PutError, PutReport};
use crate::query::{MAX_LIMIT, QueryError, QueryReport};
use crate::read::{GetReport, ReadError};
use crate::refs::{Absent, RefsError, RefsReport};
use crate::{Board, OpenError};

/// A plugin compiled into Board, which runs in Board's own process.
pub trait Plugin: Sync {
    /// Its manifest, which `Manifest::built_in` reads.
    fn manifest(&self) -> Manifest;
    /// The schema it ships for each type its manifest declares.
    fn schemas(&self) -> Vec<(RecordType, Value)>;
    /// Runs `capability` with `input`, which its input schema admitted.
    /// Board checks what it returns as it checks a process plugin's reply.
    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError>;
}

/// What Board offers a built-in plugin while it runs a capability.
pub trait Host {
    /// Runs `sql` as `board query` does, returning at most `limit` rows.
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError>;
    /// Answers `paths` as `board refs` does, each in order, through one
    /// snapshot.
    fn refs(
        &self,
        paths: &[LogicalPath],
        absent: Absent,
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError>;
    /// Reads `paths` as `board get` does, each in order, through one
    /// snapshot.
    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError>;
    /// Writes `lines`, each one line of `board put --batch`, as that
    /// command does under `BOARD_PLUGIN`: every line checked, the effects
    /// check run as the plugin, and one commit or nothing.
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError>;
    /// Runs `target` with `input` as the plugin's own `board call` does:
    /// one call deeper, under the `BOARD_PLUGIN` it would set, with only
    /// the capability's declared `env` among the variables. Board's
    /// refusal, or the called plugin's, is returned as `board call` prints
    /// it.
    fn call(&self, target: &Target, input: &Value) -> Result<CallReport, Box<Refusal>>;
    /// The board that `board --personal` or `board -C <directory>` opens
    /// when the plugin runs it, as the same plugin at the same depth.
    fn board(&self, elsewhere: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>>;
    /// Whether the plugin may run `op`, `<plugin>.<capability>`, at `path`,
    /// showing `shown`, as `board decide` answers under `BOARD_PLUGIN`: the
    /// owner's signed approval admits it, their refusal refuses it, and with
    /// no answer the pending effect is written in the personal board and
    /// it is refused as asked.
    fn decide(
        &self,
        op: &str,
        path: &LogicalPath,
        shown: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>>;
    fn settings(&self) -> &Settings;
}

/// Another board a built-in plugin acts on, as its own `board` opens it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Elsewhere {
    /// `--personal`: the personal board the known boards file names.
    Personal,
    /// `-C <directory>`: the board holding the directory, read against
    /// the called board's root, or the one known board that mounts it.
    Directory(PathBuf),
}

/// What a built-in plugin reads in place of a process plugin's environment.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The time the call started.
    pub now: SystemTime,
    /// `BOARD_DAY_START`: when the person's day starts, such as `07:00`.
    pub day_start: Option<String>,
    /// `BOARD_CALLER_DIR`: the directory Board was started in.
    pub caller: Option<PathBuf>,
}

/// How many rows a query returns: 1 to `MAX_LIMIT`, or all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit(usize);

impl Limit {
    pub const ALL: Self = Self(usize::MAX);

    /// At most `rows` rows; `None` outside 1 to `MAX_LIMIT`.
    pub fn rows(rows: usize) -> Option<Self> {
        (1..=MAX_LIMIT).contains(&rows).then_some(Self(rows))
    }
}

/// A built-in plugin's own refusal, whose code is
/// `<plugin>::<area>::<reason>` with the plugin's own name, as in
/// `board/error/v1`.
#[derive(Clone, Debug)]
pub struct PluginError {
    pub code: String,
    pub message: String,
    pub fields: Map<String, Value>,
}

impl PluginError {
    /// `<plugin>::board::refused`, as a process plugin's Python helper reports it.
    pub fn board_refused(plugin: &str, command: &str, refusal: &Refusal) -> Self {
        let code = refusal.code.as_str();
        let then = (refusal.hint.as_deref().filter(|hint| !hint.is_empty()))
            .map_or_else(String::new, |hint| format!("; {hint}"));
        let mut fields = refusal.fields.clone();
        fields.insert("code".to_owned(), Value::from(code));
        Self {
            code: format!("{plugin}::board::refused"),
            message: format!(
                "`board {command}` refused with {code}: {}{then}",
                refusal.message
            ),
            fields,
        }
    }

    /// Reads `input`, which passed `capability`'s input schema, as `T`, or
    /// refuses with `<plugin>::call::bad_input`, since then `T` and the
    /// schema disagree.
    pub fn read_input<T: DeserializeOwned>(
        plugin: &str,
        capability: &str,
        input: &Value,
    ) -> Result<T, Self> {
        T::deserialize(input).map_err(|error| Self {
            code: format!("{plugin}::call::bad_input"),
            message: format!(
                "{capability}'s input {input} passed its schema but does not read: {error}; \
                 the input struct and its schema disagree, so report this to Board's developers"
            ),
            fields: Map::new(),
        })
    }
}

/// The plugins built into a Board.
#[derive(Clone, Copy, Default)]
pub struct BuiltIn(pub &'static [&'static dyn Plugin]);

impl fmt::Debug for BuiltIn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.0.iter().map(|plugin| plugin.manifest().name))
            .finish()
    }
}

/// The host a built-in plugin sees during one call.
pub(crate) struct Hosted<'a> {
    pub(crate) board: HostBoard<'a>,
    pub(crate) settings: Settings,
    /// The plugin a put is checked as, or the `BOARD_PLUGIN` that names
    /// none.
    pub(crate) plugin: Result<PluginName, OsString>,
    /// What the plugin's own `board` reads: the caller's plugins directory,
    /// cache, known boards and signing, and the environment
    /// `Environment::nested` gives it.
    pub(crate) plugins: Plugins<'a>,
    /// Where a process plugin's `board` runs: the called board's root.
    pub(crate) root: &'a Path,
}

/// The board a host acts on: the called one, or one it opened.
pub(crate) enum HostBoard<'a> {
    Called(&'a Board),
    Opened(Box<Board>),
}

impl Deref for HostBoard<'_> {
    type Target = Board;

    fn deref(&self) -> &Board {
        match self {
            Self::Called(board) => board,
            Self::Opened(board) => board,
        }
    }
}

impl Hosted<'_> {
    /// The board opened again, with the same built-in plugins and build,
    /// as the plugin's own `board` opens it. Its timings stay in the board
    /// it opens, outside the call's trace.
    fn again(&self) -> Result<Board, OpenError> {
        let home = self.board.repositories().home().map(Into::into);
        let mut board = Board::open(self.board.repository().path(), home)?;
        board.build_in(self.board.built_in.0);
        board.built_by(self.board.build);
        Ok(board)
    }

    fn plugin(&self) -> Result<PluginName, PutError> {
        (self.plugin.clone()).map_err(|text| PutError::BadPlugin { text })
    }
}

fn opened(error: &OpenError) -> Box<Refusal> {
    Box::new(Refusal::new(error.code(), error, &error.fields()))
}

impl Host for Hosted<'_> {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError> {
        self.board.query(sql, limit.0, None, self.plugins.cache)
    }

    fn refs(
        &self,
        paths: &[LogicalPath],
        absent: Absent,
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        Ok(self
            .board
            .refs(paths, absent, &[], None, self.plugins.cache)?
            .collect())
    }

    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        Ok(self.board.get_each(paths)?.collect())
    }

    /// Opens the board again as the plugin, as a put through a mount opens
    /// the mounted board, so every check reads the plugin where it already
    /// does.
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        let plugin = self.plugin()?;
        let mut board = self
            .again()
            .map_err(|error| PutError::Open(Box::new(error)))?;
        board.ask_as(Asker {
            plugin: Some(plugin),
            ..Asker::default()
        });
        let text = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        board.put_batch(&mut text.as_bytes())
    }

    fn call(&self, target: &Target, input: &Value) -> Result<CallReport, Box<Refusal>> {
        let board = self.again().map_err(|error| opened(&error))?;
        // As bytes, so the input passes the size limit and parse a process
        // plugin's `board call` reads it through.
        let called = board.call(target, input.to_string().as_bytes(), &self.plugins);
        // The processes it started count in this call's trace.
        let started = board.take_trace().work.get("started").copied();
        (self.board.trace.borrow_mut()).count("started", started.unwrap_or(0));
        called.map_err(|error| Box::new(error.refusal()))
    }

    fn board(&self, elsewhere: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        let home = self.board.repositories().home();
        let known = self.plugins.known.cloned().unwrap_or_default();
        let start = match elsewhere {
            Elsewhere::Personal => known.personal(home)?,
            Elsewhere::Directory(directory) => {
                known::entered(self.root, &[directory.as_os_str().to_owned()])?
            }
        };
        let (mut board, _) = known::open(&start, home, self.plugins.cache, || Ok(known))?;
        board.build_in(self.board.built_in.0);
        board.built_by(self.board.build);
        Ok(Box::new(Hosted {
            board: HostBoard::Opened(Box::new(board)),
            settings: self.settings.clone(),
            plugin: self.plugin.clone(),
            plugins: self.plugins.clone(),
            root: self.root,
        }))
    }

    /// Opens the board again asking as the plugin, with the personal
    /// board and signing `board decide` reads.
    fn decide(
        &self,
        op: &str,
        path: &LogicalPath,
        shown: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        if !matches!(op.parse(), Ok(Target::Named { .. })) {
            let message = format!(
                "the op {op:?} to decide must be `<plugin>.<capability>`, such as review.comment"
            );
            let refusal = Refusal::new(Code::CliUsage, message, &Fields::default());
            return Err(Box::new(refusal.with_hint(
                "name the op as the plugin's name, a dot and the capability, as `board decide --op` takes it",
            )));
        }
        let plugin = self.plugin().map_err(|error| Box::new(error.refusal()))?;
        let default = KnownBoards::default();
        let known = self.plugins.known.unwrap_or(&default);
        let home = self.board.repositories().home().map(Path::to_path_buf);
        let asker = known::asker(
            Some(plugin),
            Some(known),
            home,
            self.plugins.signing.clone(),
        )?;
        let mut board = self.again().map_err(|error| opened(&error))?;
        board.ask_as(asker);
        (board.decide_op(op, path, shown)).map_err(|error| Box::new(error.refusal()))
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}

impl Board {
    /// Builds `plugins` into this board's calls, ahead of installed ones.
    pub fn build_in(&mut self, plugins: &'static [&'static dyn Plugin]) {
        self.built_in = BuiltIn(plugins);
    }
}

#[cfg(test)]
mod tests;
