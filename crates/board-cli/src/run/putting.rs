//! `put`'s arguments, and what a command-line put fills from.

use std::ffi::OsString;
use std::time::Duration;

use board_ops::{Expect, Mending, Settings, Variables};

use super::Given;
use crate::config::Context;
use crate::values::{duration, expect, var};

/// `put`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Putting {
    /// Where to write: a logical path that leaves out .json, such as /tasks/a, or a, run in /tasks.
    ///
    /// At a path ending .md, stdin that is a JSON object is a record, and other stdin is the file.
    #[arg(required_unless_present_any = ["batch", "new"])]
    pub(crate) path: Option<Given>,
    /// Write at a new name in the folder PATH names, which ends in /, or in the type's folder
    ///
    /// The name is the put's UTC minute and four random hex digits, such as
    /// 20261030T1405Z-c2e1. The put never overwrites a file, and its report names the path.
    /// With no PATH, the record's schema names the folder in `x-board-folder`.
    #[arg(long, conflicts_with_all = ["expect", "batch", "file"])]
    pub(crate) new: bool,
    /// Fill {name} in the type's folder; repeatable.
    ///
    /// The value fills one path segment, so it holds no /. A later --var of the same name wins.
    #[arg(long, value_name = "NAME=VALUE", requires = "new", conflicts_with_all = ["path", "batch"], value_parser = var)]
    pub(crate) var: Vec<(String, String)>,
    /// The blob the file must hold, or `none` for a new file.
    #[arg(long, value_parser = expect)]
    pub(crate) expect: Option<Expect>,
    /// Read JSON Lines of `{path, expect}` with a record, a text or `remove: true`; apply all or none.
    ///
    /// A line `{new, record}` writes the record in the folder `new` names, as --new does. A record line at `/.board/types/<namespace>/<kind>/v<n>.schema` adds that schema and takes
    /// no expect. Schema lines are checked before the other lines, and a schema the board holds
    /// is committed if HEAD lacks it, unless a symlink holds it. Every line writes in one board: this board, or the board one mount leads to. A
    /// batch through a mount commits in the mounted board and moves the mount's pin.
    #[arg(long, conflicts_with_all = ["path", "expect"])]
    pub(crate) batch: bool,
    /// Mark the batch's commit as a bulk one, ending its message in `Board-Bulk: <REASON>`.
    ///
    /// The reason is one line of 1 to 80 characters, such as `migrate flows`, so a reader of
    /// history can tell a commit that touches many files from an edit.
    #[arg(long, value_name = "REASON", requires = "batch")]
    pub(crate) bulk: Option<String>,
    /// Write stdin as a file at a path that is its whole name, such as /notes/plan.txt.
    ///
    /// A path ending .md needs no --file.
    ///
    /// With a `store` setting, --file uploads bytes over 1 MiB or not UTF-8 under their digest and
    /// writes a board/attachment/v1 record.
    #[arg(long, conflicts_with = "batch")]
    pub(crate) file: bool,
    /// How long to wait for the board's lock, with a unit: 500ms, 30s, 2m or 1h.
    #[arg(long, value_name = "DURATION", value_parser = duration, default_value = "5s")]
    pub(crate) wait: Duration,
    #[command(flatten)]
    pub(crate) checking: Checking,
}

/// How `put` checks a record: as given with `--strict`, or after filling
/// and dropping fields.
#[derive(Debug, clap::Args)]
pub(crate) struct Checking {
    /// Fill and drop no field, and refuse what the schema refuses.
    ///
    /// Without it, a record's missing `by`, `asked`, `decided`, question `status` and review
    /// `patch_ids` are filled, and a field its schema does not name is dropped with a warning.
    #[arg(long)]
    pub(crate) strict: bool,
}

/// What a command-line put fills from: the session the environment names,
/// the time now, the directory Board started in, and the `PATH` and `HOME`
/// a derived fill runs git with.
pub(super) fn mending(context: &Context) -> Mending {
    let environment = &context.environment;
    let inherited = |name| environment.inherited.get(name).map(OsString::as_os_str);
    let session = Mending::session(
        inherited("BOARD_SESSION"),
        inherited("CLAUDE_CODE_HOST_SESSION_ID"),
    );
    let passed = [("PATH", &environment.path), ("HOME", &environment.home)];
    let variables = (passed.into_iter())
        .filter_map(|(name, value)| Some((name.to_owned(), value.clone()?)))
        .collect();
    Mending {
        session,
        settings: Settings {
            now: std::time::SystemTime::now(),
            day_start: None,
            utc_offset: environment.utc_offset,
            caller: environment.caller.as_ref().map(std::path::PathBuf::from),
            environment: Variables(variables),
            cache_home: None,
        },
    }
}
