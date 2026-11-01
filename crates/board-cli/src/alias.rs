//! `board <alias> <capability>`: an enabled plugin's alias runs its
//! capabilities as `board call <plugin>.<capability>` runs them
//! (docs/plugins.md, Aliases).

use std::ffi::OsString;

use board_ops::{AliasConflict, Collision};
use board_plugin::Manifest;
use clap::{Arg, Command, CommandFactory as _};

use crate::config::Context;
use crate::run::{Cli, entered, open};

/// The word crumbkit answers with help, which no alias may take.
const HELP: &str = "help";

/// A command line once its alias is read.
pub(crate) struct Aliased {
    /// The words, with `<alias> <capability>` read as
    /// `call <plugin>.<capability>`.
    pub(crate) arguments: Vec<OsString>,
    /// A command for each alias, which Board's tree gains for help.
    pub(crate) commands: Vec<Command>,
    /// Why the alias the words name refuses to run.
    pub(crate) conflict: Option<AliasConflict>,
}

/// Board's built-in commands, which an alias may not be.
pub(crate) fn commands() -> Vec<String> {
    let root = Cli::command();
    let names = root
        .get_subcommands()
        .map(|command| command.get_name().to_owned());
    names.chain([HELP.to_owned()]).collect()
}

/// `arguments` with an alias of a plugin that the board Board runs as
/// enables read as `board call`. Only a command line whose first word is
/// no built-in command reads the board's aliases: `board -h`, `board help`
/// and `board <alias>`.
pub(crate) fn read(arguments: Vec<OsString>, context: &Context) -> Aliased {
    let root = Cli::command();
    let scan = scan(&root, &arguments);
    let word = scan
        .word
        .and_then(|at| Some(arguments.get(at)?.to_str()?.to_owned()));
    let built_in =
        (word.as_deref()).is_some_and(|word| word != HELP && root.find_subcommand(word).is_some());
    let manifests = if built_in {
        Vec::new()
    } else {
        aliases(context, &scan)
    };
    let mut aliased = Aliased {
        commands: help(&manifests),
        arguments,
        conflict: None,
    };
    let (Some(at), Some(word)) = (scan.word, word) else {
        return aliased;
    };
    let mut named = (manifests.iter())
        .filter_map(|manifest| Some((manifest, manifest.alias.as_ref()?)))
        .filter(|(_, alias)| alias.as_str() == word);
    let Some((manifest, alias)) = named.next() else {
        return aliased;
    };
    aliased.conflict = named.next().map(|(other, _)| AliasConflict {
        plugin: manifest.name.clone(),
        alias: alias.clone(),
        with: Collision::Plugin(other.name.clone()),
    });
    let capability = aliased.arguments.get(at + 1).and_then(|word| word.to_str());
    if let Some(capability) = capability.filter(|word| !word.starts_with('-')) {
        let target = format!("{}.{capability}", manifest.name);
        aliased
            .arguments
            .splice(at..at + 2, ["call".into(), target.into()]);
    }
    aliased
}

/// Where a command line names its first word, and how it moves the run's
/// start: `-C` and `--personal`.
struct Scan {
    word: Option<usize>,
    directories: Vec<OsString>,
    personal: bool,
}

/// The first word of `arguments` after the program that is neither one of
/// `root`'s own flags nor a flag's value.
fn scan(root: &Command, arguments: &[OsString]) -> Scan {
    let mut scan = Scan {
        word: None,
        directories: Vec::new(),
        personal: false,
    };
    let mut at = 1;
    while let Some(word) = arguments.get(at) {
        let text = word.to_string_lossy();
        let flag: Option<(&Arg, Option<&str>)> = if let Some(long) = text.strip_prefix("--") {
            let (name, value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            (root.get_arguments())
                .find(|arg| arg.get_long() == Some(name))
                .map(|arg| (arg, value))
        } else if let Some(short) = text.strip_prefix('-').filter(|short| !short.is_empty()) {
            let mut chars = short.chars();
            let letter = chars.next();
            let rest = chars.as_str().trim_start_matches('=');
            (root.get_arguments())
                .find(|arg| arg.get_short() == letter)
                .map(|arg| (arg, (!rest.is_empty()).then_some(rest)))
        } else {
            scan.word = Some(at);
            return scan;
        };
        at += 1;
        let Some((arg, value)) = flag else {
            continue;
        };
        let takes = arg.get_action().takes_values();
        let value = match value {
            Some(value) => Some(OsString::from(value)),
            None if takes => {
                at += 1;
                arguments.get(at - 1).cloned()
            }
            None => None,
        };
        match arg.get_id().as_str() {
            "directories" => scan.directories.extend(value),
            "personal" => scan.personal = true,
            _ => {}
        }
    }
    scan
}

/// The manifest of each plugin the board Board runs as enables that has
/// an alias, or none when no board can be opened there.
fn aliases(context: &Context, scan: &Scan) -> Vec<Manifest> {
    let base = if scan.personal {
        context.personal().ok()
    } else {
        Some(context.start.clone())
    };
    let start = base.and_then(|base| entered(&base, &scan.directories).ok());
    let Some((mut board, _)) = start.and_then(|start| open(context, &start).ok()) else {
        return Vec::new();
    };
    board.build_in(context.built_in.0);
    let mut manifests = board
        .aliases(context.plugins.as_deref())
        .unwrap_or_default();
    // A built-in command always wins, so its alias names nothing.
    let commands = commands();
    manifests.retain(|manifest| {
        alias(manifest).is_some_and(|alias| !commands.iter().any(|c| c == alias))
    });
    manifests
}

fn alias(manifest: &Manifest) -> Option<&str> {
    manifest.alias.as_ref().map(board_plugin::Alias::as_str)
}

/// A command for each alias among `manifests`, the first plugin's where
/// two share one, listing that plugin's capabilities.
fn help(manifests: &[Manifest]) -> Vec<Command> {
    let mut commands: Vec<Command> = Vec::new();
    for manifest in manifests {
        let Some(alias) = alias(manifest) else {
            continue;
        };
        if commands.iter().any(|command| command.get_name() == alias) {
            continue;
        }
        let plugin = &manifest.name;
        let about = format!(
            "Run a capability of the {plugin} plugin: `board {alias} <capability>` is `board call {plugin}.<capability>`."
        );
        let capabilities = (manifest.capabilities.iter()).map(|capability| {
            Command::new(capability.name.as_str().to_owned()).about(capability.description.clone())
        });
        commands.push(
            Command::new(alias.to_owned())
                .about(about)
                .subcommand_required(true)
                .subcommands(capabilities),
        );
    }
    commands
}
