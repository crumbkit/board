//! `board call <plugin>.<capability>` with the capability's input as flags
//! (docs/plugins.md, Calling a capability).

use std::ffi::OsString;
use std::path::Path;

use board_model::{Code, LogicalPath};
use board_ops::{Board, BuiltIn, PluginState};
use board_plugin::{Capability, Target};
use clap::builder::{BoolValueParser, PossibleValue, StringValueParser};
use clap::builder::{TypedValueParser, ValueParser};
use clap::{Arg, ArgMatches, Command, FromArgMatches};
use crumbkit::Tool;
use crumbkit::help::{Effects, RuntimeLeaf};
use serde_json::{Map, Value};

use crate::render::{CALL_REPORT, TOOL};
use crate::run::{Cli, Prefix};
use crate::stdin::{Input, Stdin};

/// The target a command line names after `call`, and the description, the
/// flags and the help on stdin of the capability an installed plugin offers
/// under that name.
pub(crate) struct Grafted {
    name: String,
    offered: Option<(String, Vec<Flag>, String, Effects)>,
    /// Whether an input property can go only on stdin.
    stdin: bool,
}

/// The long flags that a grafted command takes for itself, which no input
/// property can be.
pub(crate) const RESERVED_FLAGS: &[&str] = &[
    "capability",
    "fields",
    "full",
    "help",
    "json",
    "long",
    "no-input",
];

/// How a grafted command takes stdin, as its help says.
const STDIN: &str =
    "Without a flag, Board reads the input as JSON on stdin; with one, it ignores stdin.";

/// How a grafted command whose input properties are all flags takes stdin.
const NO_STDIN: &str = "Every input property is a flag, so Board does not read stdin.";

/// The format that marks a string input property as a logical path, which
/// a flag gives relative to the run's prefix.
const PATH_FORMAT: &str = "board-path";

/// What a path flag's help adds, after its own text without a final `.`.
const RELATIVE: &str = "; a path without a leading / is read from the current directory.";

/// An input property's name, help and parser, and whether it is a path.
type Flag = (String, String, ValueParser, bool);

/// A parsed command line, with the target and flags of a grafted command.
pub(crate) struct Parsed {
    pub(crate) cli: Cli,
    pub(crate) grafted: Option<(Target, Map<String, Value>)>,
}

impl FromArgMatches for Parsed {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let call = matches.subcommand_matches("call");
        let grafted = call
            .and_then(ArgMatches::subcommand)
            .and_then(|(name, flags)| {
                let value = |id: &str| flags.try_get_one::<Value>(id).ok().flatten().cloned();
                let given = flags
                    .ids()
                    .filter_map(|id| Some((id.to_string(), value(id.as_str())?)));
                Some((name.parse().ok()?, given.collect()))
            });
        Ok(Self {
            cli: Cli::from_arg_matches(matches)?,
            grafted,
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

/// Reads each of `found`'s path flags in `flags` from the prefix `at`, or
/// `/` without one. A value that does not read as a logical path stays as
/// given, for the input schema to judge.
pub(crate) fn resolve(
    found: Option<&Grafted>,
    flags: &mut Map<String, Value>,
    at: Option<&Prefix>,
) {
    let offered = found.and_then(|found| found.offered.as_ref()).into_iter();
    for (name, ..) in offered
        .flat_map(|(_, flags, ..)| flags)
        .filter(|flag| flag.3)
    {
        if let Some(Value::String(text)) = flags.get_mut(name)
            && let Ok(from) = at.cloned().unwrap_or(Ok(LogicalPath::root()))
            && let Ok(path) = from.join(text)
        {
            *text = path.into();
        }
    }
}

/// The `<plugin>.<capability>` that `arguments` name after `call`, from the
/// plugins `built_in` or, after them, those installed in `plugins`.
pub(crate) fn find(
    arguments: &[OsString],
    built_in: BuiltIn,
    plugins: Option<&Path>,
) -> Option<Grafted> {
    let mut words = arguments.iter().filter_map(|word| word.to_str());
    words.position(|word| word == "call")?;
    let (name, plugin, capability) = words.find_map(|word| match word.parse() {
        Ok(Target::Named { plugin, capability }) => Some((word, plugin, capability)),
        _ => None,
    })?;
    let manifest = (built_in.0.iter().map(|built| built.manifest()))
        .find(|manifest| manifest.name == plugin)
        .or_else(|| {
            (board_plugin::installed(plugins?).ok()?.into_iter())
                .find(|entry| entry.name == plugin.as_str())?
                .manifest
                .ok()
        });
    let offered = (manifest.iter().flat_map(|manifest| &manifest.capabilities))
        .find(|offered| offered.name == capability);
    // Board refuses a target nobody offers, whatever its input.
    Some(offered.map_or_else(
        || Grafted {
            name: name.to_owned(),
            offered: None,
            stdin: false,
        },
        |offered| graft(name, offered),
    ))
}

/// Every capability of each plugin that `board` enables, built in or
/// installed in `plugins`, and a note for each enabled plugin whose
/// capabilities cannot be read.
pub(crate) fn enabled(board: &Board, plugins: Option<&Path>) -> (Vec<Grafted>, Vec<String>) {
    let mut notes = Vec::new();
    let listed = board.plugin_list(plugins).or_else(|error| {
        notes.push(format!(
            "{}: the plugins directory cannot be read, so only built-in plugins are listed: {error}",
            error.code()
        ));
        board.plugin_list(None)
    });
    let mut grafted = Vec::new();
    for entry in listed.map(|listed| listed.plugins).unwrap_or_default() {
        match &entry.state {
            _ if !entry.enabled => {}
            PluginState::Installed(manifest) => grafted.extend(
                (manifest.capabilities.iter())
                    .map(|offered| graft(&format!("{}.{}", entry.name, offered.name), offered)),
            ),
            PluginState::BadManifest(error) => notes.push(format!(
                "{}: plugin {} is skipped: {error}",
                error.code(),
                entry.name
            )),
            PluginState::NotInstalled => notes.push(format!(
                "{}: plugin {} is enabled but not installed, so it is skipped",
                Code::PluginNotInstalled,
                entry.name
            )),
        }
    }
    (grafted, notes)
}

/// The graft of `offered`, the capability that `call` names `name`.
fn graft(name: &str, offered: &Capability) -> Grafted {
    let flags = flags(offered);
    let document = offered.input.document();
    let properties = document.get("properties").and_then(Value::as_object);
    let rest: Vec<&str> = (properties
        .into_iter()
        .flatten()
        .map(|(name, _)| name.as_str()))
    .filter(|name| flags.iter().all(|(flag, ..)| flag != name))
    .collect();
    // A schema that admits properties it does not name takes them only on stdin.
    let open = document.get("additionalProperties") != Some(&Value::Bool(false))
        || document.get("patternProperties").is_some();
    let help = match rest.as_slice() {
        [] if open => STDIN.to_owned(),
        [] => NO_STDIN.to_owned(),
        rest => format!("{STDIN} These properties go on stdin: {}.", rest.join(", ")),
    };
    // MCP's hints, with MCP's defaults: a capability that declares none
    // is destructive and open-world, as the `call` leaf declares.
    let hints = offered.annotations.clone().unwrap_or_default();
    let effects = match (hints.read_only_hint, hints.destructive_hint) {
        (Some(true), _) => Effects::read_only(),
        (_, Some(false)) => Effects::additive(),
        _ => Effects::destructive(),
    };
    let effects = match hints.idempotent_hint {
        Some(true) => effects.idempotent(),
        _ => effects,
    };
    let effects = match hints.open_world_hint {
        Some(false) => effects,
        _ => effects.open_world(),
    };
    Grafted {
        name: name.to_owned(),
        offered: Some((offered.description.clone(), flags, help, effects)),
        stdin: open || !rest.is_empty(),
    }
}

/// Where the call `found` names with `flags` takes its input: from `stdin`
/// when no flag is given and it names no target, or a capability whose
/// schema admits a property that is not a flag. Otherwise Board leaves
/// `stdin` unread.
pub(crate) fn input<'a>(
    found: Option<&Grafted>,
    flags: &Map<String, Value>,
    stdin: &'a mut dyn Stdin,
) -> Input<'a> {
    if flags.is_empty() && found.is_none_or(|found| found.stdin) {
        Input::Stdin(stdin)
    } else {
        Input::Ignored(stdin, ignored(found, flags))
    }
}

/// Why Board leaves stdin unread for the call `found` names with `flags`,
/// or `None` when it reads stdin. A `bad_input` refusal adds it to its help
/// when stdin held data.
fn ignored(found: Option<&Grafted>, flags: &Map<String, Value>) -> Option<String> {
    let found = found?;
    found.offered.as_ref()?;
    if !found.stdin {
        let names = (found.offered.iter())
            .flat_map(|(_, flags, ..)| flags)
            .map(|(name, ..)| format!("--{name}"))
            .collect::<Vec<_>>();
        let names = match names.as_slice() {
            // A capability without flags takes no input that stdin could give.
            [] => return None,
            [.., last] if names.len() > 1 => {
                format!("{} and {last}", names[..names.len() - 1].join(", "))
            }
            _ => names.join(""),
        };
        return Some(format!(
            "Board ignored stdin because {} takes every input as a flag; give the input as {names}",
            found.name
        ));
    }
    let given = flags
        .keys()
        .map(|name| format!("--{name}"))
        .collect::<Vec<_>>();
    let (noun, given, was) = match given.as_slice() {
        [] => return None,
        [one] => ("the flag", one.clone(), "was"),
        _ => ("the flags", given.join(", "), "were"),
    };
    Some(format!(
        "Board ignored stdin because {noun} {given} {was} given, and a call with a flag takes \
         its whole input from flags; give every input as a flag, or give no flag and put the \
         whole input on stdin"
    ))
}

/// Each top-level string, integer, number or boolean input property whose
/// name is lowercase letters, digits, `-` and `_` and is not in
/// `RESERVED_FLAGS`. A flag's help is the property's description, or its type.
fn flags(capability: &Capability) -> Vec<Flag> {
    let document = capability.input.document();
    let properties = document
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten();
    properties
        .filter(|(name, _)| {
            let safe = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_".contains(c);
            name.starts_with(|c: char| c.is_ascii_alphanumeric())
                && name.chars().all(safe)
                && !RESERVED_FLAGS.contains(&name.as_str())
        })
        .filter_map(|(name, property)| {
            let kind = property.get("type")?.as_str()?;
            let choices = property.get("enum").and_then(Value::as_array);
            let choices: Option<Vec<String>> = choices.and_then(|choices| {
                (choices.iter())
                    .map(|choice| choice.as_str().map(str::to_owned))
                    .collect()
            });
            let parser: ValueParser = match (kind, choices) {
                ("string", Some(choices)) => Choices(choices).into(),
                ("string", None) => StringValueParser::new().map(Value::from).into(),
                ("integer", _) => clap::value_parser!(i64).map(Value::from).into(),
                ("number", _) => StringValueParser::new()
                    .try_map(|text| text.parse::<serde_json::Number>().map(Value::from))
                    .into(),
                ("boolean", _) => BoolValueParser::new().map(Value::from).into(),
                _ => return None,
            };
            let help = property
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or(kind);
            let path = property.get("format").and_then(Value::as_str) == Some(PATH_FORMAT);
            let relative = if path { RELATIVE } else { "" };
            let help = help.strip_suffix('.').filter(|_| path).unwrap_or(help);
            let help = format!("{help}{relative}");
            Some((name.clone(), help, parser, path))
        })
        .collect()
}

/// A string flag whose property's `enum` lists `.0`, all strings. Help and
/// `commands list` show them, and it takes any string, so the input schema
/// judges the value as it judges stdin.
#[derive(Clone)]
struct Choices(Vec<String>);

impl TypedValueParser for Choices {
    type Value = Value;

    fn parse_ref(
        &self,
        command: &Command,
        arg: Option<&Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Value, clap::Error> {
        StringValueParser::new()
            .parse_ref(command, arg, value)
            .map(Value::from)
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(
            self.0
                .iter()
                .map(|choice| PossibleValue::new(choice.clone())),
        ))
    }
}

/// Board's command tree, with each of `grafted` below `call`. A target
/// that no installed plugin offers takes any words, help among them, so
/// the run reaches Board's refusal of the target.
pub(crate) fn command(grafted: &[Grafted], root: Command) -> Command {
    if grafted.is_empty() {
        return root;
    }
    let leaves = grafted.iter().map(|grafted| {
        let leaf = Command::new(grafted.name.clone());
        match &grafted.offered {
            Some((about, flags, stdin, _)) => {
                let leaf = leaf.about(about.clone()).after_help(stdin.clone());
                flags.iter().fold(leaf, |leaf, (name, help, parser, _)| {
                    leaf.arg(
                        Arg::new(name.clone())
                            .long(name.clone())
                            .value_parser(parser.clone())
                            .allow_negative_numbers(true)
                            .help(help.clone()),
                    )
                })
            }
            // Its words are values; Board refuses the target whatever they are.
            None => leaf.disable_help_flag(true).arg(
                Arg::new("words")
                    .value_parser(StringValueParser::new().map(Value::from))
                    .num_args(0..)
                    .trailing_var_arg(true)
                    .allow_hyphen_values(true),
            ),
        }
    });
    root.mut_subcommand("call", |call| {
        call.subcommand_negates_reqs(true).subcommands(leaves)
    })
}

/// Board's tool record, with a leaf for each of `grafted` that prints a call
/// report, with the effects its capability's hints declare.
pub(crate) fn tool(grafted: &[Grafted]) -> Tool {
    if grafted.is_empty() {
        return TOOL;
    }
    let leaves = grafted.iter().map(|grafted| {
        let about = format!(
            "Call {} with the input its flags give, or else the JSON on stdin, or else {{}}",
            grafted.name
        );
        let leaf = RuntimeLeaf::new(vec!["call".to_owned(), grafted.name.clone()]);
        let leaf = leaf.example(Vec::new(), about).document(CALL_REPORT);
        match &grafted.offered {
            Some((.., effects)) => leaf.effects(*effects),
            None => leaf,
        }
    });
    TOOL.with_runtime_leaves(leaves.collect())
}
