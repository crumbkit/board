//! Calling a capability (docs/plugins.md, Calling a capability).

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use board_model::{Code, JsonError, parse_json};
use board_validate::RecordError;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::installed::{DiscoveryError, Installed};
use crate::manifest::{Capability, CapabilityName, Manifest, ManifestError, NameError, PluginName};
use crate::process::{self, Limits, Stopped};
use crate::protocol::{self, BadOutput, CALL_RESULT_TYPE, PluginFailure, RawCallResult, Reply};

/// The deepest nesting of calls: a call that arrives at this depth is refused.
pub const MAX_DEPTH: u32 = 8;

const DEPTH_VARIABLE: &str = "BOARD_CALL_DEPTH";

/// What a call names: a plugin's capability, or a capability alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Named {
        plugin: PluginName,
        capability: CapabilityName,
    },
    Capability(CapabilityName),
}

/// `<plugin>.<capability>`.
impl FromStr for Target {
    type Err = NameError;

    fn from_str(text: &str) -> Result<Self, NameError> {
        let (plugin, capability) = text.split_once('.').unwrap_or((text, ""));
        Ok(Self::Named {
            plugin: plugin.parse()?,
            capability: capability.parse()?,
        })
    }
}

impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named { plugin, capability } => write!(formatter, "{plugin}.{capability}"),
            Self::Capability(capability) => write!(formatter, "capability {capability}"),
        }
    }
}

/// The variables a plugin inherits, as Board's own environment holds them.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    pub path: Option<OsString>,
    pub home: Option<OsString>,
    pub lang: Option<OsString>,
    pub cache: Option<OsString>,
    pub data: Option<OsString>,
    /// `BOARD_CALL_DEPTH`.
    pub depth: Option<OsString>,
}

impl Environment {
    /// How deeply this call is nested; unset is 0.
    pub fn depth(&self) -> Result<u32, CallError> {
        let Some(text) = &self.depth else {
            return Ok(0);
        };
        let depth = text
            .to_str()
            .and_then(|text| text.parse::<u32>().ok())
            .ok_or_else(|| CallError::BadDepth { text: text.clone() })?;
        if depth >= MAX_DEPTH {
            return Err(CallError::TooDeep { depth });
        }
        Ok(depth)
    }
}

/// The plugin and capability a call runs.
#[derive(Debug)]
pub struct Chosen {
    pub manifest: Manifest,
    pub directory: PathBuf,
    capability: usize,
}

impl Chosen {
    pub fn capability(&self) -> &Capability {
        &self.manifest.capabilities[self.capability]
    }
}

/// The installed plugin and capability `target` names, which the board
/// must enable. By capability alone, exactly one enabled plugin may offer it;
/// a broken install offers nothing.
pub fn choose(
    installed: Vec<Installed>,
    enabled: &[PluginName],
    target: &Target,
) -> Result<Chosen, CallError> {
    let offered = |manifest: &Manifest, name: &CapabilityName| {
        manifest
            .capabilities
            .iter()
            .position(|capability| capability.name == *name)
    };
    let not_offered = || CallError::NotOffered {
        target: target.clone(),
    };
    match target {
        Target::Named { plugin, capability } => {
            let found = installed
                .into_iter()
                .find(|installed| installed.name == plugin.as_str())
                .ok_or_else(not_offered)?;
            let manifest = found.manifest.map_err(|source| CallError::BadManifest {
                plugin: plugin.clone(),
                source,
            })?;
            let index = offered(&manifest, capability).ok_or_else(not_offered)?;
            if !enabled.contains(plugin) {
                return Err(CallError::NotEnabled {
                    plugin: plugin.clone(),
                });
            }
            Ok(Chosen {
                manifest,
                directory: found.directory,
                capability: index,
            })
        }
        Target::Capability(capability) => {
            let (on, off): (Vec<_>, Vec<_>) = installed
                .into_iter()
                .filter_map(|installed| {
                    let manifest = installed.manifest.ok()?;
                    let index = offered(&manifest, capability)?;
                    Some(Chosen {
                        manifest,
                        directory: installed.directory,
                        capability: index,
                    })
                })
                .partition(|chosen| enabled.contains(&chosen.manifest.name));
            let mut on = on.into_iter();
            match (on.next(), on.next(), off.into_iter().next()) {
                (Some(chosen), None, _) => Ok(chosen),
                (Some(first), Some(second), _) => Err(CallError::Ambiguous {
                    capability: capability.clone(),
                    plugins: [first, second]
                        .into_iter()
                        .chain(on)
                        .map(|chosen| chosen.manifest.name)
                        .collect(),
                }),
                (None, _, Some(installed)) => Err(CallError::NotEnabled {
                    plugin: installed.manifest.name,
                }),
                (None, _, None) => Err(not_offered()),
            }
        }
    }
}

/// What a capability returned.
#[derive(Debug)]
pub struct CallReport {
    pub plugin: PluginName,
    pub capability: CapabilityName,
    pub output: Value,
}

/// Where a call runs: the board's root, the plugin's database and what the
/// child inherits.
#[derive(Debug)]
pub struct Context<'a> {
    pub root: &'a Path,
    pub database: &'a Path,
    pub environment: &'a Environment,
    pub limits: Limits,
}

/// Checks `input` against the capability's schema, runs the plugin with one
/// request, and checks its one reply.
pub fn invoke(
    chosen: &Chosen,
    input: &[u8],
    context: &Context<'_>,
) -> Result<CallReport, CallError> {
    let depth = context.environment.depth()?;
    let capability = chosen.capability();
    let input = parse_json(input).map_err(|source| CallError::InputJson { source })?;
    capability
        .input
        .validate(&input)
        .map_err(|source| CallError::Input { source })?;
    let request = protocol::request(&capability.name, &input, context.root, context.database);
    let plugin = &chosen.manifest.name;
    let result: RawCallResult = exchange(
        &chosen.manifest,
        &chosen.directory,
        request,
        context,
        depth,
        CALL_RESULT_TYPE,
    )?;
    capability
        .output
        .validate(&result.output)
        .map_err(|source| CallError::BadOutput {
            plugin: plugin.clone(),
            reason: BadOutput::Output { source },
            stderr: Vec::new(),
        })?;
    Ok(CallReport {
        plugin: plugin.clone(),
        capability: capability.name.clone(),
        output: result.output,
    })
}

/// Runs `manifest`'s plugin in `directory` at `depth` with one request and
/// reads its one reply: a result of type `expected`, or its own error.
///
/// # Panics
///
/// Never: a manifest's `run` is never empty.
pub(crate) fn exchange<T: DeserializeOwned>(
    manifest: &Manifest,
    directory: &Path,
    request: Vec<u8>,
    context: &Context<'_>,
    depth: u32,
    expected: &'static str,
) -> Result<T, CallError> {
    let (program, arguments) = manifest
        .run
        .split_first()
        .expect("a manifest's run is never empty: Manifest::parse refuses it");
    let program = if program.contains('/') {
        directory.join(program)
    } else {
        PathBuf::from(program)
    };
    let plugin = manifest.name.clone();
    let finished = process::run(
        &program,
        arguments,
        directory,
        &environment(context.environment, depth + 1),
        request,
        context.limits,
    )
    .map_err(|stopped| match stopped {
        Stopped::Io(source) => CallError::Start {
            plugin: plugin.clone(),
            source,
        },
        Stopped::Timeout(stderr) => CallError::Timeout {
            plugin: plugin.clone(),
            stderr,
        },
        Stopped::TooLarge(stderr) => CallError::TooLarge {
            plugin: plugin.clone(),
            stderr,
        },
    })?;
    let stderr = finished.stderr;
    let bad = |reason| CallError::BadOutput {
        plugin: plugin.clone(),
        reason,
        stderr: stderr.clone(),
    };
    let written = !finished.stdout.trim_ascii().is_empty();
    match (
        finished.code,
        protocol::reply::<T>(&plugin, &finished.stdout, expected),
    ) {
        (Some(0), Ok(Reply::Result(result))) => Ok(result),
        (Some(1), Ok(Reply::Error(failure))) => Err(CallError::Plugin {
            plugin: plugin.clone(),
            failure,
            stderr,
        }),
        (Some(0), Ok(Reply::Error(_))) => Err(bad(BadOutput::Exit {
            exit: 0,
            written: "an error",
        })),
        (Some(1), Ok(Reply::Result(_))) => Err(bad(BadOutput::Exit {
            exit: 1,
            written: "a result",
        })),
        (Some(0), Err(reason)) => Err(bad(reason)),
        (Some(1), Err(reason)) if written => Err(bad(reason)),
        (exit, _) => Err(CallError::Failed {
            plugin: plugin.clone(),
            exit,
            stderr,
        }),
    }
}

/// The only variables a plugin sees.
fn environment(inherited: &Environment, depth: u32) -> Vec<(&'static str, OsString)> {
    [
        ("PATH", &inherited.path),
        ("HOME", &inherited.home),
        ("LANG", &inherited.lang),
        ("XDG_CACHE_HOME", &inherited.cache),
        ("XDG_DATA_HOME", &inherited.data),
    ]
    .into_iter()
    .filter_map(|(key, value)| Some((key, value.clone()?)))
    .chain([(DEPTH_VARIABLE, OsString::from(depth.to_string()))])
    .collect()
}

/// A call Board refuses, or the plugin's own refusal.
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error(transparent)]
    Discovery { source: DiscoveryError },
    #[error("plugin {plugin} has no database: {source}")]
    Database {
        plugin: PluginName,
        source: std::io::Error,
    },
    #[error("BOARD_CALL_DEPTH {text:?} is not a whole number")]
    BadDepth { text: OsString },
    #[error("this call is nested {depth} deep, and calls nest at most {MAX_DEPTH} deep")]
    TooDeep { depth: u32 },
    #[error("no installed plugin offers {target}")]
    NotOffered { target: Target },
    #[error("plugin {plugin} has a bad manifest: {source}")]
    BadManifest {
        plugin: PluginName,
        source: ManifestError,
    },
    #[error("plugin {plugin} is installed but this board does not enable it")]
    NotEnabled { plugin: PluginName },
    #[error(
        "capability {capability} is offered by more than one enabled plugin: {}",
        names(plugins)
    )]
    Ambiguous {
        capability: CapabilityName,
        plugins: Vec<PluginName>,
    },
    #[error("the input is not one strict JSON document: {source}")]
    InputJson { source: JsonError },
    #[error("the input fails the capability's input schema: {source}")]
    Input { source: RecordError },
    #[error("plugin {plugin} cannot be run: {source}")]
    Start {
        plugin: PluginName,
        source: std::io::Error,
    },
    #[error("plugin {plugin} took longer than its time limit and was stopped")]
    Timeout { plugin: PluginName, stderr: Vec<u8> },
    #[error("plugin {plugin} wrote more stdout than its limit and was stopped")]
    TooLarge { plugin: PluginName, stderr: Vec<u8> },
    #[error("plugin {plugin} gave a bad answer: {reason}")]
    BadOutput {
        plugin: PluginName,
        reason: BadOutput,
        stderr: Vec<u8>,
    },
    #[error("plugin {plugin} failed with exit {} and no error document", exit_text(*exit))]
    Failed {
        plugin: PluginName,
        exit: Option<i32>,
        stderr: Vec<u8>,
    },
    #[error("{}", failure.message)]
    Plugin {
        plugin: PluginName,
        failure: PluginFailure,
        stderr: Vec<u8>,
    },
}

fn names(plugins: &[PluginName]) -> String {
    plugins
        .iter()
        .map(PluginName::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn exit_text(exit: Option<i32>) -> String {
    exit.map_or_else(|| "by signal".to_owned(), |code| code.to_string())
}

impl CallError {
    /// Board's code; `None` for the plugin's own refusal, whose code is in
    /// [`CallError::code_text`].
    pub fn code(&self) -> Option<Code> {
        Some(match self {
            Self::Discovery { source } => source.code(),
            Self::BadDepth { .. } | Self::TooDeep { .. } => Code::PluginTooDeep,
            Self::NotOffered { .. } => Code::PluginNotInstalled,
            Self::BadManifest { .. } => Code::PluginBadManifest,
            Self::NotEnabled { .. } => Code::PluginNotEnabled,
            Self::Ambiguous { .. } => Code::PluginAmbiguous,
            Self::InputJson { .. } | Self::Input { .. } => Code::PluginBadInput,
            Self::Database { .. } | Self::Start { .. } | Self::Failed { .. } => Code::PluginFailed,
            Self::Timeout { .. } => Code::PluginTimeout,
            Self::TooLarge { .. } => Code::PluginTooLarge,
            Self::BadOutput { .. } => Code::PluginBadOutput,
            Self::Plugin { .. } => return None,
        })
    }

    /// The code as `board/error/v1` spells it.
    pub fn code_text(&self) -> &str {
        match (self, self.code()) {
            (Self::Plugin { failure, .. }, _) => failure.code.as_str(),
            (_, Some(code)) => code.as_str(),
            (_, None) => "",
        }
    }

    /// The typed fields of `board/error/v1`: the plugin's own with `plugin`
    /// added, or Board's, with the end of the plugin's stderr when it ran.
    pub fn fields(&self) -> Map<String, Value> {
        let mut fields = Map::new();
        match self {
            Self::Plugin {
                failure, plugin, ..
            } => {
                fields.clone_from(&failure.fields);
                fields.insert("plugin".to_owned(), json!(plugin));
            }
            Self::Ambiguous { plugins, .. } => {
                fields.insert("plugins".to_owned(), json!(plugins));
            }
            Self::BadManifest { plugin, .. }
            | Self::Database { plugin, .. }
            | Self::NotEnabled { plugin }
            | Self::Start { plugin, .. }
            | Self::Timeout { plugin, .. }
            | Self::TooLarge { plugin, .. }
            | Self::BadOutput { plugin, .. }
            | Self::Failed { plugin, .. } => {
                fields.insert("plugin".to_owned(), json!(plugin));
            }
            Self::Discovery { .. }
            | Self::BadDepth { .. }
            | Self::TooDeep { .. }
            | Self::NotOffered { .. }
            | Self::InputJson { .. }
            | Self::Input { .. } => {}
        }
        if let (Some(stderr), false) = (self.stderr(), matches!(self, Self::Plugin { .. })) {
            fields.insert("stderr".to_owned(), json!(String::from_utf8_lossy(stderr)));
        }
        fields
    }

    /// The end of the plugin's stderr, when it ran.
    pub fn stderr(&self) -> Option<&[u8]> {
        match self {
            Self::Timeout { stderr, .. }
            | Self::TooLarge { stderr, .. }
            | Self::BadOutput { stderr, .. }
            | Self::Failed { stderr, .. }
            | Self::Plugin { stderr, .. } => Some(stderr),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
