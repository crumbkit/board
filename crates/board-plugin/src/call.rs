//! Calling a capability (docs/plugins.md, Calling a capability).

use std::cell::RefCell;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use board_model::{Code, Fields, JsonError, RecordType, Refusal, parse_json};
use board_validate::RecordError;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::installed::{DiscoveryError, Installed};
use crate::manifest::{Capability, CapabilityName, Manifest, ManifestError, NameError, PluginName};
use crate::process::{self, Limits, Scope, Spawned, Stopped};
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
    type Err = TargetError;

    fn from_str(text: &str) -> Result<Self, TargetError> {
        match text.split_once('.') {
            Some((plugin, capability)) if !plugin.is_empty() && !capability.is_empty() => {
                Ok(Self::Named {
                    plugin: plugin.parse()?,
                    capability: capability.parse()?,
                })
            }
            _ => Err(TargetError::Form {
                text: text.to_owned(),
            }),
        }
    }
}

/// Text that is not `<plugin>.<capability>`.
#[derive(Debug, thiserror::Error)]
pub enum TargetError {
    #[error("{text:?} is not <plugin>.<capability>, such as tasks.due")]
    Form { text: String },
    #[error(transparent)]
    Name(#[from] NameError),
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
    /// `BOARD_TIMINGS`, so a run a plugin starts records as its caller does.
    pub timings: Option<OsString>,
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
    /// Each type the plugin ships and its schema, as `Installed` holds them.
    pub schemas: Vec<(RecordType, Value)>,
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
    match target {
        Target::Named { plugin, capability } => {
            let found = installed
                .into_iter()
                .find(|installed| installed.name == plugin.as_str())
                .ok_or_else(|| CallError::NotInstalled {
                    plugin: plugin.clone(),
                })?;
            let manifest = found.manifest.map_err(|source| CallError::BadManifest {
                plugin: plugin.clone(),
                source,
            })?;
            let index = offered(&manifest, capability).ok_or_else(|| CallError::NoCapability {
                plugin: plugin.clone(),
                capability: capability.clone(),
                offered: manifest
                    .capabilities
                    .iter()
                    .map(|offered| offered.name.clone())
                    .collect(),
            })?;
            if !enabled.contains(plugin) {
                return Err(CallError::NotEnabled {
                    plugin: plugin.clone(),
                });
            }
            Ok(Chosen {
                manifest,
                directory: found.directory,
                schemas: found.schemas,
                capability: index,
            })
        }
        Target::Capability(capability) => {
            let manifests: Vec<_> = installed
                .into_iter()
                .filter_map(|installed| {
                    Some((
                        installed.manifest.ok()?,
                        installed.directory,
                        installed.schemas,
                    ))
                })
                .collect();
            let nearest = nearest(capability, manifests.iter().map(|(manifest, ..)| manifest));
            let (on, off): (Vec<_>, Vec<_>) = manifests
                .into_iter()
                .filter_map(|(manifest, directory, schemas)| {
                    let index = offered(&manifest, capability)?;
                    Some(Chosen {
                        manifest,
                        directory,
                        schemas,
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
                (None, _, None) => Err(CallError::NotOffered {
                    capability: capability.clone(),
                    nearest,
                }),
            }
        }
    }
}

/// The capability of a working install whose name is nearest `capability`,
/// within two edits and fewer edits than `capability` has letters.
fn nearest<'a>(
    capability: &CapabilityName,
    manifests: impl Iterator<Item = &'a Manifest>,
) -> Option<CapabilityName> {
    let wanted = capability.as_str();
    manifests
        .flat_map(|manifest| &manifest.capabilities)
        .map(|offered| {
            (
                strsim::levenshtein(wanted, offered.name.as_str()),
                &offered.name,
            )
        })
        .filter(|(distance, _)| *distance <= 2 && *distance < wanted.len())
        .min()
        .map(|(_, name)| name.clone())
}

/// What a capability returned.
#[derive(Debug)]
pub struct CallReport {
    pub plugin: PluginName,
    pub capability: CapabilityName,
    pub output: Value,
}

/// Where a call runs: the board's root, the plugin's database and what the
/// child inherits, and where each process started is added.
#[derive(Debug)]
pub struct Context<'a> {
    pub root: &'a Path,
    pub database: &'a Path,
    pub environment: &'a Environment,
    pub limits: Limits,
    pub spawned: &'a RefCell<Vec<Spawned>>,
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
    let input = parse_json(input).map_err(|source| CallError::InputJson {
        capability: capability.name.clone(),
        source,
    })?;
    capability
        .input
        .validate(&input)
        .map_err(|source| CallError::Input {
            capability: capability.name.clone(),
            source,
        })?;
    let request = protocol::request(&capability.name, &input, context.root, context.database);
    let plugin = &chosen.manifest.name;
    let output = exchange(
        &chosen.manifest,
        &chosen.directory,
        request,
        context,
        depth,
        CALL_RESULT_TYPE,
        |result: RawCallResult| {
            capability
                .output
                .validate(&result.output)
                .map(|()| result.output)
                .map_err(|source| BadOutput::Output { source })
        },
    )?;
    Ok(CallReport {
        plugin: plugin.clone(),
        capability: capability.name.clone(),
        output,
    })
}

/// Runs `manifest`'s plugin in `directory` at `depth` with one request and
/// reads its one reply: a result of type `expected` that `check` accepts, or
/// its own error.
///
/// # Panics
///
/// Never: a manifest's `run` is never empty.
pub(crate) fn exchange<T: DeserializeOwned, U>(
    manifest: &Manifest,
    directory: &Path,
    request: Vec<u8>,
    context: &Context<'_>,
    depth: u32,
    expected: &'static str,
    check: impl FnOnce(T) -> Result<U, BadOutput>,
) -> Result<U, CallError> {
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
    // A nested Board leaves its plugin in the outermost plugin's group. A
    // new group there would escape the outermost Board's signals after they
    // stopped the nested Board that enforces this plugin's limits.
    let scope = if depth == 0 {
        Scope::Group
    } else {
        Scope::Process
    };
    let (spawned, finished) = process::run(
        &program,
        arguments,
        directory,
        &environment(context.environment, depth + 1),
        request,
        context.limits,
        scope,
    );
    context.spawned.borrow_mut().extend(spawned);
    let finished = finished.map_err(|stopped| match stopped {
        // Soft limit: a failed reap or signal-forwarder setup after the spawn
        // also lands here, as not_started without the stderr already read.
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
        (Some(0), Ok(Reply::Result(result))) => check(result).map_err(bad),
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
        ("BOARD_TIMINGS", &inherited.timings),
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
    #[error("plugin {plugin} has no database: {} cannot be made: {source}", path.display())]
    Database {
        plugin: PluginName,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "plugin {plugin} has no database: a temporary directory cannot be made in {}: {source}",
        parent.display()
    )]
    NoTemporaryDirectory {
        plugin: PluginName,
        parent: PathBuf,
        source: std::io::Error,
    },
    #[error("BOARD_CALL_DEPTH {text:?} is not a whole number")]
    BadDepth { text: OsString },
    #[error("this call is nested {depth} deep, and calls nest at most {MAX_DEPTH} deep")]
    TooDeep { depth: u32 },
    #[error("plugin {plugin} is not installed")]
    NotInstalled { plugin: PluginName },
    #[error(
        "plugin {plugin} has no capability {capability}; it offers {}",
        list(offered)
    )]
    NoCapability {
        plugin: PluginName,
        capability: CapabilityName,
        offered: Vec<CapabilityName>,
    },
    #[error(
        "no installed plugin offers capability {capability}{}",
        nearest.as_ref().map_or_else(String::new, |nearest| format!("; the nearest is {nearest}"))
    )]
    NotOffered {
        capability: CapabilityName,
        nearest: Option<CapabilityName>,
    },
    #[error("plugin {plugin} has a bad manifest: {source}")]
    BadManifest {
        plugin: PluginName,
        source: ManifestError,
    },
    #[error("plugin {plugin} is installed but this board does not enable it")]
    NotEnabled { plugin: PluginName },
    #[error(
        "capability {capability} is offered by more than one enabled plugin: {}",
        list(plugins)
    )]
    Ambiguous {
        capability: CapabilityName,
        plugins: Vec<PluginName>,
    },
    #[error("the input is not one strict JSON document: {source}")]
    InputJson {
        capability: CapabilityName,
        source: JsonError,
    },
    #[error("the input to capability {capability} {source}")]
    Input {
        capability: CapabilityName,
        source: RecordError,
    },
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

fn list<T: fmt::Display>(names: &[T]) -> String {
    if names.is_empty() {
        return "none".to_owned();
    }
    names
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn exit_text(exit: Option<i32>) -> String {
    exit.map_or_else(|| "by signal".to_owned(), |code| code.to_string())
}

impl CallError {
    /// This error as Board reports it, with the end of the
    /// plugin's stderr when it ran, also under `stderr` in its fields. The
    /// plugin's own refusal keeps its code and fields, with `plugin` and
    /// `stderr` added, and has no hint from Board.
    pub fn refusal(&self) -> Refusal {
        let stderr = match self {
            Self::Timeout { stderr, .. }
            | Self::TooLarge { stderr, .. }
            | Self::BadOutput { stderr, .. }
            | Self::Failed { stderr, .. }
            | Self::Plugin { stderr, .. } => Some(stderr),
            _ => None,
        };
        let (mut extra, mut first, mut at) = (Map::new(), None, None);
        if let Some(stderr) = stderr {
            extra.insert("stderr".to_owned(), json!(String::from_utf8_lossy(stderr)));
        }
        let (code, plugin, capability) = match self {
            Self::Plugin {
                plugin, failure, ..
            } => {
                let mut fields = failure.fields.clone();
                fields.insert("plugin".to_owned(), json!(plugin));
                fields.extend(extra);
                let (code, message) = (failure.code.as_str(), failure.message.clone());
                return Refusal {
                    stderr: stderr.cloned().unwrap_or_default(),
                    ..Refusal::plugin(code.to_owned(), message, fields)
                };
            }
            Self::Discovery { source } => (source.code(), None, None),
            Self::BadDepth { .. } | Self::TooDeep { .. } => (Code::PluginTooDeep, None, None),
            Self::NotInstalled { plugin } => (Code::PluginNotInstalled, Some(plugin), None),
            Self::NoCapability {
                plugin,
                capability,
                offered,
            } => {
                extra.insert("offered".to_owned(), json!(offered));
                (Code::PluginNotInstalled, Some(plugin), Some(capability))
            }
            Self::NotOffered {
                capability,
                nearest,
            } => {
                if let Some(nearest) = nearest {
                    extra.insert("nearest".to_owned(), json!(nearest));
                }
                (Code::PluginNotInstalled, None, Some(capability))
            }
            Self::BadManifest { plugin, .. } => (Code::PluginBadManifest, Some(plugin), None),
            Self::NotEnabled { plugin } => (Code::PluginNotEnabled, Some(plugin), None),
            Self::Ambiguous {
                capability,
                plugins,
            } => {
                extra.insert("plugins".to_owned(), json!(plugins));
                first = plugins.first();
                (Code::PluginAmbiguous, None, Some(capability))
            }
            Self::InputJson { capability, .. } => (Code::PluginBadInput, None, Some(capability)),
            Self::Input {
                capability,
                source:
                    RecordError::Invalid { pointer, .. }
                    | RecordError::MalformedReference { pointer, .. },
            } => {
                at = Some(pointer.clone());
                (Code::PluginBadInput, None, Some(capability))
            }
            Self::Database { plugin, .. } | Self::NoTemporaryDirectory { plugin, .. } => {
                (Code::PluginNoDatabase, Some(plugin), None)
            }
            Self::Failed { plugin, .. } => (Code::PluginFailed, Some(plugin), None),
            Self::Start { plugin, .. } => (Code::PluginNotStarted, Some(plugin), None),
            Self::Timeout { plugin, .. } => (Code::PluginTimeout, Some(plugin), None),
            Self::TooLarge { plugin, .. } => (Code::PluginTooLarge, Some(plugin), None),
            Self::BadOutput { plugin, .. } => (Code::PluginBadOutput, Some(plugin), None),
        };
        let fields = Fields {
            plugin: plugin.map(ToString::to_string),
            capability: capability.map(ToString::to_string),
            pointer: at,
            ..Fields::default()
        };
        let mut json = fields.json();
        json.extend(extra);
        // An ambiguous capability's hint shows a call naming the first plugin.
        let hinted = Fields {
            plugin: first.map(ToString::to_string).or(fields.plugin),
            ..fields
        };
        Refusal {
            fields: json,
            stderr: stderr.cloned().unwrap_or_default(),
            ..Refusal::new(code, self, &hinted)
        }
    }
}

#[cfg(test)]
mod tests;
