//! What a built-in plugin reads in place of a process plugin's
//! environment.

use std::fmt;
use std::path::PathBuf;
use std::time::SystemTime;

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
        (self.0.iter()).find(|(set, _)| set == name).map(|(_, value)| value.as_os_str())
    }
}

impl fmt::Debug for Variables {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        (formatter.debug_list())
            .entries(self.0.iter().map(|(name, _)| name))
            .finish()
    }
}
