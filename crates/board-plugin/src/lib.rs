//! Plugins: manifests, discovery, child processes and the schema shapes
//! built-in plugins generate (docs/plugins.md).

mod call;
pub mod child;
mod installed;
mod manifest;
mod process;
mod protocol;
pub mod shape;
mod sign;

pub use crate::call::{
    CallError, CallReport, Chosen, Context, Environment, Inherited, MAX_DEPTH, PLUGIN_VARIABLE,
    Target, TargetError, choose, input, invoke, output, passed,
};
pub use crate::installed::{
    DiscoveryError, Installed, MANIFEST_FILE, Origin, built_in, installed, shipped_schema_path,
};
pub use crate::manifest::{
    Alias, AliasError, Annotations, Capability, MANIFEST_TYPE, Manifest, ManifestError, MetaFilter,
    MetaFilterError, Side, VariableName, VariableNameError, Version, VersionError,
};
pub use crate::process::{Limits, Spawned};
#[cfg(unix)]
pub use crate::process::{clean_up_on_signals, stop_on_signals};
pub use crate::protocol::{BadOutput, CALL_RESULT_TYPE, CALL_TYPE, ERROR_TYPE, PluginFailure};
pub use crate::sign::{CANCELLED, PRINCIPAL, SignError, Verb, sign, verify};
