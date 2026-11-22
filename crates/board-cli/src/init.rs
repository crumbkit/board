//! `board init`, which makes the git repository at the directory a board
//! without opening one. With `--personal` it also enables the plugins a
//! colleague needs and names the board as the personal one in the known
//! boards file (docs/architecture/paths/personal.md).

use std::path::{Path, PathBuf};

use board_model::{Code, Fields, PluginName, Refusal};
use board_ops::{InitError, InitReport, MountsAt, Naming, PERSONAL_PLUGINS};

use crate::config::Context;
use serde_json::json;

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
        Ok((report, personal)) => rendered(printer, &report, personal.as_ref()),
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
    // `init` refuses the same directory with the same error.
    let Some(root) = board_tree::repository_root(start) else {
        let start = start.to_path_buf();
        return Err(Box::new(refused(&InitError::NotARepository { start })));
    };
    let naming = Naming::plan(&file, std::fs::read(&file), &root)?;
    let report = board_ops::init(start).map_err(|error| Box::new(refused(&error)))?;
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

fn rendered(printer: Printer, report: &InitReport, personal: Option<&Personal>) -> Output {
    let root = report.root.display();
    let mut text = if report.created {
        format!("created .board/board.json in {root}\n")
    } else {
        format!("{root} is already a board\n")
    };
    let mut value = json!({"root": report.root.to_string_lossy(), "created": report.created});
    let note = match personal {
        None => report.created.then_some(INIT_NEXT),
        Some(personal) => {
            text.push_str(&personal.lines(&report.root));
            let enabled: Vec<&str> = personal.enabled.iter().map(PluginName::as_str).collect();
            value["enabled"] = json!(enabled);
            let file = personal.file.to_string_lossy();
            value["personal"] = json!({"file": file, "named": personal.named});
            (report.created || personal.changed()).then_some(PERSONAL_NEXT)
        }
    };
    let answer = render::said(value, text);
    let answer = match note {
        Some(note) => answer.note(note),
        None => answer,
    };
    printer.print(render::INIT, Ok(answer))
}

/// The note after `init` made a board.
const INIT_NEXT: &str = "next: run `board check`; it checks every file and says how to fix any it cannot read. Then commit .board/board.json";
/// The note after `init --personal` changed something.
const PERSONAL_NEXT: &str = "next: run `board --personal tasks add --kind project --title '<title>'`, or ask Claude Code to add a project with the tasks skill";
