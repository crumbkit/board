//! `board <plugin> <capability>`: an enabled plugin's name, or its alias,
//! runs its capabilities as `board call <plugin>.<capability>` runs them
//! (docs/plugins.md, Plugin commands).
//!
//! A first word is a built-in command first, then an enabled plugin's
//! name, then an enabled plugin's alias. A built-in command never reads a
//! plugin's manifest. Board reads them only after
//! clap refuses the first word as no command of its own, and before the
//! parse only when the words ask for help, so that `board -h` lists them.

use std::ffi::OsString;

use board_model::{Code, Fields, Refusal};
use board_ops::MountsAt;
use board_plugin::Manifest;
use clap::error::ErrorKind;
use clap::{ArgMatches, Command, CommandFactory as _};

use crate::config::Context;
use crate::run::{Cli, entered};

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
/// help on a first word that is no built-in command: `root` with a hidden
/// command for each enabled plugin, whose own help lists its capabilities,
/// and a line naming them, and the words with a plugin's word and its
/// capability read as `board call`, whose help lists the flags. Help on a
/// built-in command reads no manifest.
pub(crate) fn before(arguments: &mut Vec<OsString>, context: &Context, root: Command) -> Command {
    let helps = |word: &OsString| ["-h", "--help", HELP].map(OsString::from).contains(word);
    if arguments.len() > 1 && !arguments.iter().skip(1).any(helps) {
        return root;
    }
    let (external, manifests) = read(arguments, context);
    if let Some(external) = &external
        && let Some(Ok(called)) = resolve(arguments, external, &manifests)
    {
        *arguments = called;
    }
    let commands = help(&manifests);
    let Some(line) = listed(&commands) else {
        return root;
    };
    let after = root.get_after_help().map(ToString::to_string);
    let after = after.map_or_else(|| line.clone(), |after| format!("{after}\n\n{line}"));
    root.subcommands(commands.into_iter().map(|command| command.hide(true)))
        .after_help(after)
}

/// The most plugin commands the root page names before it counts the rest.
const NAMED: usize = 6;

/// The line that names the plugin commands among `commands`, at most
/// [`NAMED`] of them, and counts the rest; `None` when there are none.
fn listed(commands: &[Command]) -> Option<String> {
    if commands.is_empty() {
        return None;
    }
    let names: Vec<&str> = (commands.iter().take(NAMED))
        .map(Command::get_name)
        .collect();
    let left = commands.len().saturating_sub(NAMED);
    let more = if left == 0 {
        String::new()
    } else {
        format!(" and {left} more")
    };
    Some(format!(
        "Plugins: {}{more}; `board <plugin> -h` lists one's capabilities.",
        names.join(", ")
    ))
}

/// After clap refused `refused` for a first word that is no command of
/// Board's own: the call an alias of that word makes, or why it refuses.
/// `None` when no enabled plugin has the alias, so clap's refusal stands.
pub(crate) fn after(
    arguments: &[OsString],
    context: &Context,
    refused: &crumbkit::Refused,
) -> Option<Resolved> {
    // The first word alone decides: crumbkit's path may name a built-in
    // command that comes later, as `check` does in `board controls check`.
    if refused.error()?.kind() != ErrorKind::InvalidSubcommand {
        return None;
    }
    let (external, manifests) = read(arguments, context);
    let external = external?;
    resolve(arguments, &external, &manifests).or_else(|| built_in(arguments, &external, context))
}

/// The `board call` line of a built-in plugin's name and capability when no
/// enabled plugin took the word, as when no board opens or the board does
/// not enable the plugin, so the call reports why it cannot run.
fn built_in(arguments: &[OsString], external: &External, context: &Context) -> Option<Resolved> {
    let capability = external.rest.first()?.to_str()?;
    let named =
        (context.built_in.0.iter()).any(|plugin| answers_to(&plugin.manifest(), &external.alias));
    if !named || capability.starts_with('-') {
        return None;
    }
    let mut called = arguments.to_vec();
    let target = format!("{}.{capability}", external.alias);
    called.splice(external.at..external.at + 2, ["call".into(), target.into()]);
    Some(Ok(called))
}

/// The first word that is no built-in command, where it is, and the words
/// after it.
struct External {
    at: usize,
    alias: String,
    rest: Vec<OsString>,
}

/// The first word of `arguments` that is no built-in command, read by clap
/// with every other root flag, and the manifest of each plugin that the
/// board those flags lead to enables. A first word that is a built-in
/// command reads no manifest.
fn read(arguments: &[OsString], context: &Context) -> (Option<External>, Vec<Manifest>) {
    let commands = commands();
    let Some(matches) = loose(arguments) else {
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
        let (at, rest) = placed(arguments, words)?;
        let alias = alias.to_owned();
        Some(External { at, alias, rest })
    });
    (external, enabled(context, &directories, personal))
}

/// `arguments` read by Board's root with any first word admitted, or `None`
/// when they do not read.
fn loose(arguments: &[OsString]) -> Option<ArgMatches> {
    let root = (Cli::command().allow_external_subcommands(true))
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .ignore_errors(true);
    root.try_get_matches_from(arguments).ok()
}

/// Where the first word of `arguments` sits when it is no built-in command,
/// and the words after it.
pub(crate) fn first_word(arguments: &[OsString]) -> Option<(usize, Vec<OsString>)> {
    let matches = loose(arguments)?;
    let (word, words) = matches.subcommand()?;
    let built_in = commands().iter().any(|command| command == word);
    if built_in {
        None
    } else {
        placed(arguments, words)
    }
}

/// Where the first word whose own words are `words` sits in `arguments`,
/// and those words.
fn placed(arguments: &[OsString], words: &ArgMatches) -> Option<(usize, Vec<OsString>)> {
    let rest: Vec<OsString> = (words.try_get_many::<OsString>("").ok().flatten())
        .map(|values| values.cloned().collect())
        .unwrap_or_default();
    Some((arguments.len().checked_sub(rest.len() + 1)?, rest))
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
    if alias == HELP {
        return None;
    }
    let mut named = named(manifests, alias).into_iter();
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
            // A former name stays in the call, so the call warns of it.
            let word = if answers_to(manifest, alias) {
                alias
            } else {
                plugin.as_str()
            };
            let target = format!("{word}.{capability}");
            called.splice(external.at..external.at + 2, ["call".into(), target.into()]);
            Some(Ok(called))
        }
        given => {
            let given = given.map_or_else(
                || "no capability".to_owned(),
                |flag| format!("{flag} in its place"),
            );
            let message = format!(
                "board {alias} takes the capability right after {alias}, as in board {alias} <capability> [flags], and was given {given}"
            );
            let hint = format!("`board {alias} -h` lists the {plugin} plugin's capabilities");
            let refusal = Refusal::new(Code::CliUsage, message, &Fields::default()).with_hint(hint);
            Some(Err(Box::new(refusal)))
        }
    }
}

/// The manifest of each plugin that the board Board runs as from
/// `directories` and `personal` enables, or none when no board can be
/// opened there.
fn enabled(context: &Context, directories: &[OsString], personal: bool) -> Vec<Manifest> {
    let base = if personal {
        context.personal().ok()
    } else {
        Some(context.start.clone())
    };
    let start = base.and_then(|base| entered(&base, directories).ok());
    let Some((mut board, _)) = start.and_then(|start| context.open(&start, MountsAt::Pins).ok())
    else {
        return Vec::new();
    };
    board.build_in(context.built_in.0);
    (board.aliases(context.plugins.as_deref())).unwrap_or_default()
}

fn manifest_alias(manifest: &Manifest) -> Option<&str> {
    manifest.alias.as_ref().map(board_plugin::Alias::as_str)
}

/// Whether `word` is `manifest`'s name or one of its former names.
fn answers_to(manifest: &Manifest, word: &str) -> bool {
    manifest.name.as_str() == word
        || (manifest.former_names.iter()).any(|name| name.as_str() == word)
}

/// The plugins among `manifests` that `word` runs: the one whose name or
/// former name it is, or else each whose alias it is.
fn named<'m>(manifests: &'m [Manifest], word: &str) -> Vec<&'m Manifest> {
    let by_name = (manifests.iter()).find(|manifest| manifest.name.as_str() == word);
    let by_name =
        by_name.or_else(|| (manifests.iter()).find(|manifest| answers_to(manifest, word)));
    by_name.map_or_else(
        || {
            (manifests.iter())
                .filter(|manifest| manifest_alias(manifest) == Some(word))
                .collect()
        },
        |manifest| vec![manifest],
    )
}

/// A command for each plugin in `manifests`, named for the first of its
/// name and alias that is no built-in command and runs it alone, with the
/// other as a visible alias. A plugin with no such word has none, and an
/// alias two plugins share runs neither, so running it reaches its
/// refusal. Each lists the plugin's capabilities.
fn help(manifests: &[Manifest]) -> Vec<Command> {
    let commands = commands();
    let runs = |manifest: &Manifest, word: &str| {
        !commands.iter().any(|command| command == word)
            && matches!(named(manifests, word).as_slice(), [only] if only.name == manifest.name)
    };
    let commands = manifests.iter().filter_map(|manifest| {
        let alias = manifest_alias(manifest).filter(|alias| *alias != manifest.name.as_str());
        let words = [Some(manifest.name.as_str()), alias];
        let mut words = (words.into_iter().flatten()).filter(|word| runs(manifest, word));
        let first = words.next()?;
        let plugin = &manifest.name;
        let about = format!(
            "Run a capability of the {plugin} plugin, as `board call {plugin}.<capability>`"
        );
        let capabilities = (manifest.capabilities.iter()).map(|capability| {
            Command::new(capability.name.as_str().to_owned()).about(capability.description.clone())
        });
        let command = Command::new(first.to_owned())
            .about(about)
            .long_about(manifest.description.clone())
            .subcommand_required(true)
            .subcommands(capabilities);
        let command = words.fold(command, |command, word| {
            command.visible_alias(word.to_owned())
        });
        let former = (manifest.former_names.iter()).filter(|word| runs(manifest, word.as_str()));
        Some(former.fold(command, |command, word| command.alias(word.to_string())))
    });
    commands.collect()
}
