//! `board <alias> <capability>`: an enabled plugin's alias runs its
//! capabilities as `board call <plugin>.<capability>` runs them
//! (docs/plugins.md, Aliases).
//!
//! A built-in command never reads an alias. Board reads them only after
//! clap refuses the first word as no command of its own, and before the
//! parse only when the words ask for help, so that `board -h` lists them.

use std::ffi::OsString;

use board_model::{Code, Fields, Refusal};
use board_plugin::Manifest;
use clap::error::ErrorKind;
use clap::{Command, CommandFactory as _};

use crate::config::Context;
use crate::run::{Cli, entered, open};

/// The word crumbkit answers with help, which no alias may take.
const HELP: &str = "help";

/// Board's built-in commands, which an alias may not be.
pub(crate) fn commands() -> Vec<String> {
    names(&Cli::command())
}

/// The built-in commands of `root`, and `help`.
fn names(root: &Command) -> Vec<String> {
    let names = root
        .get_subcommands()
        .map(|command| command.get_name().to_owned());
    names.chain([HELP.to_owned()]).collect()
}

/// What an alias makes of a command line: the words with
/// `<alias> <capability>` read as `call <plugin>.<capability>`, or why it
/// refuses.
pub(crate) type Resolved = Result<Vec<OsString>, Box<Refusal>>;

/// For root help (`board`, `board -h`, `board --help`, `board help`) and for
/// help on a first word that is no built-in command: a command for each
/// alias, which Board's tree gains so that help lists it, and the words with
/// an alias and its capability read as `board call`, whose help lists the
/// flags. Help on a built-in command reads no alias.
pub(crate) fn before(arguments: &mut Vec<OsString>, context: &Context) -> Vec<Command> {
    let helps = |word: &OsString| ["-h", "--help", HELP].map(OsString::from).contains(word);
    if arguments.len() > 1 && !arguments.iter().skip(1).any(helps) {
        return Vec::new();
    }
    let (external, manifests) = read(arguments, context);
    if let Some(external) = &external
        && let Some(Ok(called)) = resolve(arguments, external, &manifests)
    {
        *arguments = called;
    }
    help(&manifests)
}

/// After clap refused `refused` for a first word that is no command of
/// Board's own: the call an alias of that word makes, or why it refuses.
/// `None` when no enabled plugin has the alias, so clap's refusal stands.
pub(crate) fn after(
    arguments: &[OsString],
    context: &Context,
    refused: &crumbkit::Refused,
) -> Option<Resolved> {
    let unknown = refused.error()?.kind() == ErrorKind::InvalidSubcommand;
    if !unknown || !refused.path().is_empty() {
        return None;
    }
    let (external, manifests) = read(arguments, context);
    resolve(arguments, &external?, &manifests)
}

/// The first word that is no built-in command, where it is, and the words
/// after it.
struct External {
    at: usize,
    alias: String,
    rest: Vec<OsString>,
}

/// The first word of `arguments` that is no built-in command, read by clap
/// with every other root flag, and the manifest of each plugin with an
/// alias that the board those flags lead to enables. A first word that is a
/// built-in command reads no manifest.
fn read(arguments: &[OsString], context: &Context) -> (Option<External>, Vec<Manifest>) {
    let root = Cli::command();
    let commands = names(&root);
    let root = root
        .allow_external_subcommands(true)
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .ignore_errors(true);
    let Ok(matches) = root.try_get_matches_from(arguments) else {
        return (None, Vec::new());
    };
    let directories: Vec<OsString> = (matches
        .try_get_many::<OsString>("directories")
        .ok()
        .flatten())
    .map(|values| values.cloned().collect())
    .unwrap_or_default();
    let personal = matches.get_flag("personal");
    let first = matches.subcommand();
    if first.is_some_and(|(word, _)| word != HELP && commands.iter().any(|c| c == word)) {
        return (None, Vec::new());
    }
    let external = first.and_then(|(alias, words)| {
        let rest: Vec<OsString> = (words.try_get_many::<OsString>("").ok().flatten())
            .map(|values| values.cloned().collect())
            .unwrap_or_default();
        let at = arguments.len().checked_sub(rest.len() + 1)?;
        let alias = alias.to_owned();
        Some(External { at, alias, rest })
    });
    (
        external,
        aliases(context, &directories, personal, &commands),
    )
}

/// The call that `external`'s alias makes among `manifests`, or why it
/// refuses. `None` when no plugin has the alias, or when help follows the
/// alias, which its command in the tree answers.
fn resolve(
    arguments: &[OsString],
    external: &External,
    manifests: &[Manifest],
) -> Option<Resolved> {
    let alias = external.alias.as_str();
    let mut named = (manifests.iter()).filter(|manifest| manifest_alias(manifest) == Some(alias));
    let manifest = named.next()?;
    let plugin = &manifest.name;
    let capability = external.rest.first().and_then(|word| word.to_str());
    if let Some(other) = named.next() {
        let message = format!(
            "the alias {alias} is declared by both the {plugin} and {} plugins, which the board enables",
            other.name
        );
        let next = capability
            .filter(|word| !word.starts_with('-'))
            .map_or_else(
                || "`board plugin list` lists the plugins".to_owned(),
                |capability| format!("run `board call {plugin}.{capability}`"),
            );
        let hint = format!("{next}; change or remove `alias` in one plugin's plugin.json");
        let fields = Fields {
            plugin: Some(plugin.clone()),
            ..Fields::default()
        };
        let refusal = Refusal::new(Code::PluginAliasConflict, message, &fields).with_hint(hint);
        return Some(Err(Box::new(refusal)));
    }
    match capability {
        Some("-h" | "--help") => None,
        Some(capability) if !capability.starts_with('-') => {
            let mut called = arguments.to_vec();
            let target = format!("{plugin}.{capability}");
            called.splice(external.at..external.at + 2, ["call".into(), target.into()]);
            Some(Ok(called))
        }
        given => {
            let given = given.map_or_else(
                || "no capability".to_owned(),
                |flag| format!("{flag} in its place"),
            );
            let message = format!(
                "board {alias} takes the capability right after the alias, as in board {alias} <capability> [flags], and was given {given}"
            );
            let hint = format!("`board {alias} -h` lists the {plugin} plugin's capabilities");
            let refusal = Refusal::new(Code::CliUsage, message, &Fields::default()).with_hint(hint);
            Some(Err(Box::new(refusal)))
        }
    }
}

/// The manifest of each plugin that the board Board runs as from
/// `directories` and `personal` enables and that has an alias no built-in
/// command takes, or none when no board can be opened there.
fn aliases(
    context: &Context,
    directories: &[OsString],
    personal: bool,
    commands: &[String],
) -> Vec<Manifest> {
    let base = if personal {
        context.personal().ok()
    } else {
        Some(context.start.clone())
    };
    let start = base.and_then(|base| entered(&base, directories).ok());
    let Some((mut board, _)) = start.and_then(|start| open(context, &start).ok()) else {
        return Vec::new();
    };
    board.build_in(context.built_in.0);
    let mut manifests = board
        .aliases(context.plugins.as_deref())
        .unwrap_or_default();
    // A built-in command always wins, so its alias names nothing.
    manifests.retain(|manifest| {
        manifest_alias(manifest).is_some_and(|alias| !commands.iter().any(|c| c == alias))
    });
    manifests
}

fn manifest_alias(manifest: &Manifest) -> Option<&str> {
    manifest.alias.as_ref().map(board_plugin::Alias::as_str)
}

/// A command for each alias only one of `manifests` has, listing that
/// plugin's capabilities. An alias two plugins share has none, so running
/// it reaches its refusal.
fn help(manifests: &[Manifest]) -> Vec<Command> {
    let shared = |alias: &str| {
        (manifests.iter())
            .filter(|manifest| manifest_alias(manifest) == Some(alias))
            .count()
            > 1
    };
    let commands = manifests.iter().filter_map(|manifest| {
        let alias = manifest_alias(manifest).filter(|alias| !shared(alias))?;
        let plugin = &manifest.name;
        let about = format!(
            "Run a capability of the {plugin} plugin: `board {alias} <capability>` is `board call {plugin}.<capability>`."
        );
        let capabilities = (manifest.capabilities.iter()).map(|capability| {
            Command::new(capability.name.as_str().to_owned()).about(capability.description.clone())
        });
        Some(
            Command::new(alias.to_owned())
                .about(about)
                .subcommand_required(true)
                .subcommands(capabilities),
        )
    });
    commands.collect()
}
