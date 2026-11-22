//! `board init`, which makes the git repository at the directory a board
//! without opening one. With `--personal` it also enables the plugins a
//! colleague needs and names the board as the personal one in the known
//! boards file (docs/architecture/paths/personal.md).

use std::path::{Path, PathBuf};

use board_model::{Code, Fields, PluginName, Refusal};
use board_ops::{InitError, InitReport, MountsAt, Naming, PERSONAL_PLUGINS};

use crate::config::Context;
use crate::render::{self, Printer};
use crate::report::Output;
use crate::run::plain;

/// `init`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Initing {
    /// Also enable tasks, runs, evals, rules, plans and share, and name this board as your personal board.
    ///
    /// It sets `personal` in `$XDG_CONFIG_HOME/board/config.json`, adds the board to `boards`, and refuses before writing anything when the file names another personal board. The global `board --personal`, given before the command, runs a command as the personal board instead.
    #[arg(long)]
    pub(crate) personal: bool,
}

/// What `init --personal` did beyond `init`.
#[derive(Debug)]
pub(crate) struct Personal {
    /// The plugins this run enabled, in order.
    pub(crate) enabled: Vec<PluginName>,
    /// The known boards file.
    pub(crate) file: PathBuf,
    /// Whether this run wrote the file.
    pub(crate) named: bool,
}

impl Personal {
    /// The two lines text output adds after `init`'s own.
    pub(crate) fn lines(&self, root: &Path) -> String {
        let plugins: Vec<&str> = self.enabled.iter().map(PluginName::as_str).collect();
        let enabled = match plugins.as_slice() {
            [] => "the six plugins were already enabled".to_owned(),
            [one] => format!("enabled {one}"),
            [rest @ .., last] => format!("enabled {} and {last}", rest.join(", ")),
        };
        let (root, file) = (root.display(), self.file.display());
        let named = if self.named {
            format!("named {root} as the personal board in {file}")
        } else {
            format!("{file} already names {root}")
        };
        format!("{enabled}\n{named}\n")
    }

    /// Whether this run changed the board or the file.
    pub(crate) fn changed(&self) -> bool {
        self.named || !self.enabled.is_empty()
    }
}

/// `board init`, with `--personal` as `initing` asks.
pub(crate) fn init(printer: Printer, context: &Context, start: &Path, initing: &Initing) -> Output {
    let done = if initing.personal {
        personal(context, start).map(|(report, personal)| (report, Some(personal)))
    } else {
        board_ops::init(start)
            .map(|report| (report, None))
            .map_err(|error| Box::new(refused(&error)))
    };
    match done {
        Ok((report, personal)) => render::init(printer, &report, personal.as_ref()),
        Err(refusal) => render::refused(printer, *refusal),
    }
}

/// The refusal of `init`; outside a git repository it says to run `git init`.
fn refused(error: &InitError) -> Refusal {
    let outside = matches!(error, InitError::NotARepository { .. });
    board_ops::unopened(error.code(), error, &Fields::default(), outside)
}

/// `init --personal`: reads the known boards file and refuses before
/// writing anything when it cannot name this board, makes the board,
/// enables each of [`PERSONAL_PLUGINS`] as `plugin enable` does, one commit
/// each, then names the board in the file.
fn personal(context: &Context, start: &Path) -> Result<(InitReport, Personal), Box<Refusal>> {
    let file = context.boards.clone().ok_or_else(|| {
        let message = "no known boards file: neither HOME nor XDG_CONFIG_HOME is set";
        Box::new(plain(Code::ConfigNotFound, &message))
    })?;
    let plan = |root: &Path| Naming::plan(&file, std::fs::read(&file), root);
    let planned = board_tree::repository_root(start)
        .map(|root| plan(&root.canonicalize().unwrap_or(root)))
        .transpose()?;
    let report = board_ops::init(start).map_err(|error| Box::new(refused(&error)))?;
    let naming = match planned {
        Some(naming) => naming,
        None => plan(&report.root)?,
    };
    let (mut board, _) = context.open(&report.root, MountsAt::Pins)?;
    board.build_in(context.built_in.0);
    let commands = crate::alias::commands();
    let commands: Vec<&str> = commands.iter().map(String::as_str).collect();
    let (plugins, cache) = (context.plugins.as_deref(), context.cache.as_deref());
    let mut enabled = Vec::new();
    for name in PERSONAL_PLUGINS {
        let name: PluginName =
            (name.parse()).map_err(|error| Box::new(plain(Code::CliUsage, &error)))?;
        let wrote = board
            .plugin_enable(plugins, cache, &name, &commands)
            .map_err(|error| Box::new(plain(error.code(), &error)))?;
        if wrote.put.commit.is_some() || !wrote.put.writes.is_empty() {
            enabled.push(name);
        }
    }
    let named = naming.write()?;
    let file = naming.file().to_path_buf();
    let personal = Personal {
        enabled,
        file,
        named,
    };
    Ok((report, personal))
}
