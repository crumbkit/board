//! `board call <plugin>.<capability>` with the capability's input as flags
//! (docs/plugins.md, Calling a capability).

use std::ffi::OsString;
use std::path::Path;

use board_model::{Code, LogicalPath};
use board_ops::{Board, BuiltIn, PluginState};
use board_plugin::{Capability, Manifest, Target};
use clap::builder::{BoolValueParser, PossibleValue, StringValueParser};
use clap::builder::{TypedValueParser, ValueParser};
use clap::error::ErrorKind;
use clap::{Arg, ArgAction, ArgMatches, Command, FromArgMatches};
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

/// What a boolean flag's help adds, after its own text without a final `.`.
const BARE: &str = "; given bare, it means true";

/// The value name of a boolean flag, whose value may be left out.
const TRUE_OR_FALSE: &str = "true|false";

/// The value of an array flag that, given alone, sends an empty array.
const NONE: &str = "none";

/// What an array flag's help adds, after its own text without a final `.`.
const REPEATED: &str = "; repeat the flag for each value, or give none alone for an empty list.";

/// What an array flag of paths adds, after its own text without a final `.`.
const REPEATED_PATHS: &str = "; repeat the flag for each path, or give none alone for an empty \
                              list; a path without a leading / is read from the current \
                              directory, so give ./none for a path named none.";

/// A flag for one input property.
struct Flag {
    /// The property's name, which is the flag's long name.
    name: String,
    help: String,
    parser: ValueParser,
    /// Whether the property is a logical path, given relative to the prefix.
    path: bool,
    /// Whether the property is boolean, so the flag may be given bare.
    boolean: bool,
    /// Whether the property is an array of strings, so the flag may be
    /// given more than once, each value one item.
    many: bool,
}

/// One value of an array flag: an item, or `none`, which given alone
/// sends an empty array.
#[derive(Clone)]
struct Element(Option<Value>);

/// A parsed command line, with the target and flags of a grafted command.
pub(crate) struct Parsed {
    pub(crate) cli: Cli,
    pub(crate) grafted: Option<(Target, Map<String, Value>)>,
}

impl FromArgMatches for Parsed {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let call = matches.subcommand_matches("call");
        let grafted = match call.and_then(ArgMatches::subcommand) {
            Some((name, flags)) => match name.parse() {
                Ok(target) => Some((target, given(flags)?)),
                Err(_) => None,
            },
            None => None,
        };
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

/// The value of each flag given in `flags`: a scalar flag's one value, or
/// an array flag's values in the order given, with `none` alone as an
/// empty array. `none` beside another value is a usage error.
fn given(flags: &ArgMatches) -> Result<Map<String, Value>, clap::Error> {
    let mut given = Map::new();
    for id in flags.ids() {
        let id = id.as_str();
        if let Ok(Some(value)) = flags.try_get_one::<Value>(id) {
            given.insert(id.to_owned(), value.clone());
        } else if let Ok(Some(elements)) = flags.try_get_many::<Element>(id) {
            let items: Option<Vec<Value>> = elements.map(|element| element.0.clone()).collect();
            let items = match items {
                Some(items) => items,
                None if flags.get_raw_occurrences(id).into_iter().flatten().count() == 1 => {
                    Vec::new()
                }
                None => {
                    return Err(clap::Error::raw(
                        ErrorKind::ArgumentConflict,
                        format!(
                            "--{id} {NONE} means an empty list, so it cannot be given beside \
                             another --{id}\n"
                        ),
                    ));
                }
            };
            given.insert(id.to_owned(), Value::Array(items));
        }
    }
    Ok(given)
}

/// Reads each of `found`'s path flags in `flags` from the prefix `at`, or
/// `/` without one, and each item of its path array flags. A value that
/// does not read as a logical path stays as given, for the input schema to
/// judge.
pub(crate) fn resolve(
    found: Option<&Grafted>,
    flags: &mut Map<String, Value>,
    at: Option<&Prefix>,
) {
    let offered = found.and_then(|found| found.offered.as_ref()).into_iter();
    let Ok(from) = at.cloned().unwrap_or(Ok(LogicalPath::root())) else {
        return;
    };
    let read = |value: &mut Value| {
        if let Value::String(text) = value
            && let Ok(path) = from.join(text)
        {
            *text = path.into();
        }
    };
    for flag in offered
        .flat_map(|(_, flags, ..)| flags)
        .filter(|flag| flag.path)
    {
        match flags.get_mut(&flag.name) {
            Some(Value::Array(items)) => items.iter_mut().for_each(read),
            Some(value) => read(value),
            None => {}
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
    // A former name reaches the renamed plugin, as `call` does.
    let answers =
        |manifest: &Manifest| manifest.name == plugin || manifest.former_names.contains(&plugin);
    let manifest = (built_in.0.iter().map(|built| built.manifest()))
        .find(answers)
        .or_else(|| {
            (board_plugin::installed(plugins?).ok()?.into_iter())
                .filter_map(|entry| entry.manifest.ok())
                .find(answers)
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
    .filter(|name| flags.iter().all(|flag| flag.name != *name))
    .collect();
    // A schema that admits properties it does not name takes them only on stdin.
    let open = document.get("additionalProperties") != Some(&Value::Bool(false))
        || document.get("patternProperties").is_some();
    // A list may still come on stdin, as it did before it was a flag.
    let listed = flags.iter().any(|flag| flag.many);
    let help = match rest.as_slice() {
        [] if open || listed => STDIN.to_owned(),
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
        stdin: open || listed || !rest.is_empty(),
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
            .map(|flag| format!("--{}", flag.name))
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

/// Each top-level string, integer, number or boolean input property, and
/// each array whose items are strings, whose name is lowercase letters,
/// digits, `-` and `_` and is not in `RESERVED_FLAGS`. A flag's help is the
/// property's description, or its type; a boolean flag's help says that it
/// may be given bare, and an array flag's that it may be repeated.
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
            let many = kind == "array";
            // An array flag takes its items' strings, one per value.
            let item = match property.get("items") {
                Some(items) if many && items.get("type")?.as_str()? == "string" => items,
                _ if many => return None,
                _ => property,
            };
            let path = item.get("format").and_then(Value::as_str) == Some(PATH_FORMAT);
            let choices = item.get("enum").and_then(Value::as_array);
            let choices: Option<Vec<String>> = choices.and_then(|choices| {
                (choices.iter())
                    .map(|choice| choice.as_str().map(str::to_owned))
                    .collect()
            });
            let parser: ValueParser = match (kind, choices) {
                ("array", choices) => Items {
                    name: name.clone(),
                    choices,
                    path,
                }
                .into(),
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
            let boolean = kind == "boolean";
            let added = match (path, boolean) {
                _ if many && path => REPEATED_PATHS.to_owned(),
                _ if many => REPEATED.to_owned(),
                (true, _) => RELATIVE.to_owned(),
                // A bare flag sends true, whatever the default.
                (_, true) if property.get("default") == Some(&Value::Bool(true)) => format!(
                    "{BARE}, which is already the default; give --{name} false to turn it off."
                ),
                (_, true) => format!("{BARE}."),
                _ => String::new(),
            };
            let help = help
                .strip_suffix('.')
                .filter(|_| !added.is_empty())
                .unwrap_or(help);
            Some(Flag {
                name: name.clone(),
                help: format!("{help}{added}"),
                parser,
                path,
                boolean,
                many,
            })
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

/// An array flag's value: one item of the property `name`, whose `enum`, if
/// any, lists `choices`, as [`Choices`] takes a string, or `none`. An item of
/// paths that holds a comma is refused, because each path is its own value.
#[derive(Clone)]
struct Items {
    name: String,
    choices: Option<Vec<String>>,
    path: bool,
}

impl TypedValueParser for Items {
    type Value = Element;

    fn parse_ref(
        &self,
        command: &Command,
        arg: Option<&Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Element, clap::Error> {
        let text = StringValueParser::new().parse_ref(command, arg, value)?;
        if text == NONE {
            return Ok(Element(None));
        }
        if self.path && text.contains(',') {
            let name = &self.name;
            return Err(clap::Error::raw(
                ErrorKind::ValueValidation,
                format!(
                    "--{name} takes one path per value, and {text} holds a comma; repeat \
                     --{name} for each path, as --{name} /a.md --{name} /b.md\n"
                ),
            )
            .with_cmd(command));
        }
        Ok(Element(Some(Value::from(text))))
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        let choices = self.choices.as_ref()?;
        Some(Box::new(
            choices
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
                flags.iter().fold(leaf, |leaf, flag| {
                    let arg = Arg::new(flag.name.clone())
                        .long(flag.name.clone())
                        .value_parser(flag.parser.clone())
                        .allow_negative_numbers(true)
                        .help(flag.help.clone());
                    // A bare boolean flag reads as `true`, as `--flag true` does.
                    let arg = if flag.boolean {
                        arg.num_args(0..=1)
                            .default_missing_value("true")
                            .value_name(TRUE_OR_FALSE)
                    } else if flag.many {
                        arg.action(ArgAction::Append)
                    } else {
                        arg
                    };
                    leaf.arg(arg)
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
