//! `board call <plugin>.<capability>` with the capability's input as flags
//! (docs/plugins.md, Calling a capability).

use std::ffi::OsString;
use std::path::Path;

use board_model::LogicalPath;
use board_ops::BuiltIn;
use board_plugin::{Capability, Target};
use clap::builder::{BoolValueParser, StringValueParser};
use clap::builder::{TypedValueParser as _, ValueParser};
use clap::{Arg, ArgMatches, Command, CommandFactory as _, FromArgMatches};
use crumbkit::help::{Guide, RuntimeLeaf};
use serde_json::{Map, Value};

use crate::render::{CALL_REPORT, GUIDE};
use crate::run::{Cli, Prefix};

/// The target a command line names after `call`, and the description, the
/// flags and the help on stdin of the capability an installed plugin offers
/// under that name.
pub(crate) struct Grafted {
    name: String,
    offered: Option<(String, Vec<Flag>, String)>,
    /// Whether an input property can go only on stdin.
    stdin: bool,
}

/// The long flags that a grafted call takes for itself, which no input
/// property can be.
pub(crate) const RESERVED_FLAGS: &[&str] =
    &["capability", "fields", "full", "help", "json", "long"];

/// How a grafted call takes stdin, as its help says.
const STDIN: &str =
    "Without a flag, Board reads the input as JSON on stdin; with one, it ignores stdin.";

/// How a grafted call whose input properties are all flags takes stdin.
const NO_STDIN: &str = "Every input property is a flag, so Board does not read stdin.";

/// The format that marks a string input property as a logical path, which
/// a flag gives relative to the run's prefix.
const PATH_FORMAT: &str = "board-path";

/// What a path flag's help adds, after its own text without a final `.`.
const RELATIVE: &str = "; a path without a leading / is read from the current directory.";

/// An input property's name, help and parser, and whether it is a path.
type Flag = (String, String, ValueParser, bool);

/// A parsed command line, with the target and flags of a grafted call.
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
        .flat_map(|(_, flags, _)| flags)
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
    let offered = || {
        let manifest = (built_in.0.iter().map(|built| built.manifest()))
            .find(|manifest| manifest.name == plugin)
            .or_else(|| {
                (board_plugin::installed(plugins?).ok()?.into_iter())
                    .find(|entry| entry.name == plugin.as_str())?
                    .manifest
                    .ok()
            })?;
        let offered = manifest
            .capabilities
            .iter()
            .find(|offered| offered.name == capability)?;
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
        let open = document.get("additionalProperties") != Some(&Value::Bool(false));
        let help = match rest.as_slice() {
            [] if open => STDIN.to_owned(),
            [] => NO_STDIN.to_owned(),
            rest => format!("{STDIN} These properties go on stdin: {}.", rest.join(", ")),
        };
        let stdin = open || !rest.is_empty();
        Some(((offered.description.clone(), flags, help), stdin))
    };
    let (offered, stdin) =
        offered().map_or((None, true), |(offered, stdin)| (Some(offered), stdin));
    Some(Grafted {
        name: name.to_owned(),
        offered,
        stdin,
    })
}

/// Whether the call `found` names reads stdin when no flag is given: unless
/// the capability it offers names every input property its schema admits
/// and each is a flag.
pub(crate) fn reads_stdin(found: Option<&Grafted>) -> bool {
    found.is_none_or(|found| found.stdin)
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
            let parser: ValueParser = match kind {
                "string" => StringValueParser::new().map(Value::from).into(),
                "integer" => clap::value_parser!(i64).map(Value::from).into(),
                "number" => StringValueParser::new()
                    .try_map(|text| text.parse::<serde_json::Number>().map(Value::from))
                    .into(),
                "boolean" => BoolValueParser::new().map(Value::from).into(),
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

/// Board's command tree, with `grafted` below `call`. A target that no
/// installed plugin offers takes any words, help among them, so the run
/// reaches Board's refusal of the target.
pub(crate) fn command(grafted: Option<&Grafted>) -> Command {
    let Some(grafted) = grafted else {
        return Cli::command();
    };
    let leaf = Command::new(grafted.name.clone());
    let leaf = match &grafted.offered {
        Some((about, flags, stdin)) => {
            let leaf = leaf.about(about.clone()).after_help(stdin.clone());
            flags.iter().fold(leaf, |leaf, (name, help, parser, _)| {
                leaf.arg(
                    Arg::new(name.clone())
                        .long(name.clone())
                        .value_parser(parser.clone())
                        .help(help.clone()),
                )
            })
        }
        // Its words are values, so a flag among them keeps stdin unread.
        None => leaf.disable_help_flag(true).arg(
            Arg::new("words")
                .value_parser(StringValueParser::new().map(Value::from))
                .num_args(0..)
                .trailing_var_arg(true)
                .allow_hyphen_values(true),
        ),
    };
    Cli::command().mut_subcommand("call", |call| {
        call.subcommand_negates_reqs(true).subcommand(leaf)
    })
}

/// Board's guide, with a leaf for `grafted` that prints a call report.
pub(crate) fn guide(grafted: Option<&Grafted>) -> Guide {
    let Some(grafted) = grafted else {
        return GUIDE;
    };
    let about = format!(
        "Call {} with the input its flags give, or else the JSON on stdin, or else {{}}",
        grafted.name
    );
    let leaf = RuntimeLeaf::new(vec!["call".to_owned(), grafted.name.clone()]);
    GUIDE.with_runtime_leaves(vec![leaf.example(Vec::new(), about).document(CALL_REPORT)])
}
