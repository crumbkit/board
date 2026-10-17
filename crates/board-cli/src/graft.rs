//! `board call <plugin>.<capability>` with the capability's input as flags
//! (docs/plugins.md, Calling a capability).

use std::ffi::OsString;
use std::path::Path;

use board_plugin::{Capability, Target};
use clap::builder::{BoolValueParser, StringValueParser};
use clap::builder::{TypedValueParser as _, ValueParser};
use clap::{Arg, ArgMatches, Command, CommandFactory as _, FromArgMatches};
use crumbkit::help::{Guide, RuntimeLeaf};
use serde_json::{Map, Value};

use crate::render::{CALL_REPORT, GUIDE};
use crate::run::Cli;

/// The target a command line names after `call`, and the description, the
/// flags and the help on stdin of the capability an installed plugin offers
/// under that name.
pub(crate) struct Grafted {
    name: String,
    offered: Option<(String, Vec<Flag>, String)>,
}

/// The long flags that a grafted call takes for itself, which no input
/// property can be.
pub(crate) const RESERVED_FLAGS: &[&str] =
    &["capability", "fields", "full", "help", "json", "long"];

/// How a grafted call takes stdin, as its help says.
const STDIN: &str =
    "Without a flag, Board reads the input as JSON on stdin; with one, it ignores stdin.";

/// An input property's name, help and parser.
type Flag = (String, String, ValueParser);

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

/// The `<plugin>.<capability>` that `arguments` name after `call`.
pub(crate) fn find(arguments: &[OsString], plugins: Option<&Path>) -> Option<Grafted> {
    let mut words = arguments.iter().filter_map(|word| word.to_str());
    words.position(|word| word == "call")?;
    let (name, plugin, capability) = words.find_map(|word| match word.parse() {
        Ok(Target::Named { plugin, capability }) => Some((word, plugin, capability)),
        _ => None,
    })?;
    let offered = || {
        let installed = board_plugin::installed(plugins?).ok()?.into_iter();
        let manifest = installed
            .into_iter()
            .find(|entry| entry.name == plugin.as_str())?
            .manifest
            .ok()?;
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
        let stdin = match rest.as_slice() {
            [] => STDIN.to_owned(),
            rest => format!("{STDIN} These properties go on stdin: {}.", rest.join(", ")),
        };
        Some((offered.description.clone(), flags, stdin))
    };
    Some(Grafted {
        name: name.to_owned(),
        offered: offered(),
    })
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
                .unwrap_or(kind)
                .to_owned();
            Some((name.clone(), help, parser))
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
            flags.iter().fold(leaf, |leaf, (name, help, parser)| {
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
