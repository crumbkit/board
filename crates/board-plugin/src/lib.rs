//! Plugins: manifests, discovery and child processes (docs/plugins.md).

mod call;
mod installed;
mod manifest;
mod process;
mod protocol;
mod sign;

pub use crate::call::{
    CallError, CallReport, Chosen, Context, Environment, Inherited, MAX_DEPTH, PLUGIN_VARIABLE,
    Target, TargetError, choose, invoke,
};
pub use crate::installed::{
    DiscoveryError, Installed, MANIFEST_FILE, installed, shipped_schema_path,
};
pub use crate::manifest::{
    Annotations, Capability, MANIFEST_TYPE, Manifest, ManifestError, MetaFilter, MetaFilterError,
    Side, VariableName, VariableNameError, Version, VersionError,
};
#[cfg(unix)]
pub use crate::process::stop_on_signals;
pub use crate::process::{Limits, Spawned};
pub use crate::protocol::{BadOutput, CALL_RESULT_TYPE, CALL_TYPE, ERROR_TYPE, PluginFailure};
pub use crate::sign::{SignError, sign, verify};
