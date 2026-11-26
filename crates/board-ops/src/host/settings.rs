//! What a built-in plugin reads in place of a process plugin's
//! environment.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::time::SystemTime;

use board_plugin::child::{Exited, Process, ProcessError};

use super::Run;

/// What a built-in plugin reads in place of a process plugin's environment.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The time the call started.
    pub now: SystemTime,
    /// `BOARD_DAY_START`: when the person's day starts, such as `07:00`.
    pub day_start: Option<String>,
    /// How many seconds the local clock runs ahead of UTC, negative west of
    /// it, by the time zone board-cli's config module read. A process
    /// plugin reads `TZ` instead.
    pub utc_offset: i32,
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

impl Variables {
    /// The value of the variable `name`, when it is set.
    pub fn get(&self, name: &str) -> Option<&std::ffi::OsStr> {
        (self.0.iter())
            .find(|(set, _)| set == name)
            .map(|(_, value)| value.as_os_str())
    }
}

impl fmt::Debug for Variables {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        (formatter.debug_list())
            .entries(self.0.iter().map(|(name, _)| name))
            .finish()
    }
}

impl Settings {
    /// Runs `run` as [`super::Host::run_hiding`] says, with this environment.
    pub fn run_hidden(&self, run: &Run<'_>, hidden: &[&str]) -> Result<Exited, ProcessError> {
        let empty;
        let directory = if let Some(directory) = run.directory {
            directory
        } else {
            empty = board_index::empty_directory()
                .map_err(|source| ProcessError::NotStarted { source })?;
            empty.path()
        };
        let process = Process {
            program: run.program,
            arguments: run.arguments,
            directory,
            input: run.input,
            deadline: run.deadline,
        };
        let added = run.added.iter().map(|(name, _)| *name);
        let replaced: Vec<&str> = hidden.iter().copied().chain(added).collect();
        let environment: Vec<_> = (self.environment.0.iter())
            .filter(|(name, _)| !replaced.contains(&name.as_str()))
            .cloned()
            .chain(
                (run.added.iter()).map(|(name, value)| ((*name).to_owned(), OsString::from(value))),
            )
            .collect();
        board_plugin::child::run(&process, &environment)
    }
}
