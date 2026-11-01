//! Plugins built into Board, and what Board offers them
//! (docs/plugins/built-in.md).

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use board_model::{CapabilityName, LogicalPath, RecordType, Refusal};
use board_plugin::Manifest;
use board_plugin::child::{Exited, Process, ProcessError};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::Board;
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
    fn settings(&self) -> &Settings;
    /// Runs `process` through board-plugin's process module with the
    /// settings' `environment`, as the leader of its own process group,
    /// killed when it exits, past its deadline or when Board is stopped.
    fn run(&self, process: &Process<'_>) -> Result<Exited, ProcessError> {
        board_plugin::child::run(process, &self.settings().environment.0)
    }
    /// The value cached under `digest` in `place`, a directory under the
    /// cache home such as `board-controls/judged`, or none when there is
    /// none, it cannot be read, or the cache home is unknown.
    fn cached(&self, place: &str, digest: &[u8]) -> Option<Vec<u8>> {
        board_index::cached(&cache_place(self.settings(), place)?, digest)
    }
    /// Caches `value` under `digest` in `place`. Deleting the cache loses
    /// nothing but the time to compute the value again. With no cache home,
    /// or a `place` that is not plain relative names, nothing is cached.
    fn cache(&self, place: &str, digest: &[u8], value: &[u8]) -> std::io::Result<()> {
        match cache_place(self.settings(), place) {
            Some(directory) => board_index::cache(&directory, digest, value),
            None => Ok(()),
        }
    }
}

/// `place` under the settings' cache home, when there is one and `place`
/// is relative names only, none of them `.` or `..`.
fn cache_place(settings: &Settings, place: &str) -> Option<PathBuf> {
    let plain = !place.is_empty()
        && (place.split('/')).all(|name| !name.is_empty() && name != "." && name != "..");
    plain.then(|| settings.cache_home.as_ref().map(|home| home.join(place)))?
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
    /// The variables a process plugin offering the capability would see,
    /// its `env` among them, which `Host::run` gives a process it starts.
    pub environment: Variables,
    /// Where the person's caches go: `XDG_CACHE_HOME` when it is absolute,
    /// else `~/.cache`; none when neither is known.
    pub cache_home: Option<PathBuf>,
}

/// Variables by name and value. Its `Debug` shows only the names, since the
/// values may be secrets.
#[derive(Clone, Default)]
pub struct Variables(pub Vec<(String, std::ffi::OsString)>);

impl fmt::Debug for Variables {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        (formatter.debug_list())
            .entries(self.0.iter().map(|(name, _)| name))
            .finish()
    }
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
