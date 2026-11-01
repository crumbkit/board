//! Plugins built into Board, and what Board offers them
//! (docs/plugins/built-in.md).

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use board_model::{CapabilityName, LogicalPath, PluginName, RecordType, Refusal};
use board_plugin::Manifest;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::Board;
use crate::effects::Asker;
use crate::put::{PutError, PutReport};
use crate::query::{MAX_LIMIT, QueryError, QueryReport};
use crate::read::{GetReport, ReadError};
use crate::refs::{Absent, RefsError, RefsReport};

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
    fn settings(&self) -> &Settings;
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
    pub(crate) board: &'a Board,
    pub(crate) settings: Settings,
    /// Where the caller keeps the index.
    pub(crate) cache: Option<&'a Path>,
    /// The plugin a put is checked as, or the `BOARD_PLUGIN` that names
    /// none.
    pub(crate) plugin: Result<PluginName, OsString>,
}

impl Host for Hosted<'_> {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError> {
        self.board.query(sql, limit.0, None, self.cache)
    }

    fn refs(
        &self,
        paths: &[LogicalPath],
        absent: Absent,
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        Ok(self
            .board
            .refs(paths, absent, &[], None, self.cache)?
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
    /// Its timings stay in the board it opens, outside the call's trace.
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        let plugin = (self.plugin.clone()).map_err(|text| PutError::BadPlugin { text })?;
        let home = self.board.repositories().home().map(Into::into);
        let mut board = Board::open(self.board.repository().path(), home)
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
