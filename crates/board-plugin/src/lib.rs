//! Plugins: manifests, discovery and child processes (docs/plugins.md).

mod call;
mod extract;
mod installed;
mod manifest;
mod process;
mod protocol;

pub use crate::call::{
    CallError, CallReport, Chosen, Context, Environment, MAX_DEPTH, Target, TargetError, choose,
    invoke,
};
pub use crate::extract::{Extraction, Record, extract};
pub use crate::installed::{
    DiscoveryError, Installed, MANIFEST_FILE, installed, shipped_schema_path,
};
pub use crate::manifest::{
    Annotations, Capability, CapabilityName, MANIFEST_TYPE, Manifest, ManifestError, MetaFilter,
    MetaFilterError, NameError, PluginName, Side, Version, VersionError,
};
pub use crate::process::{Limits, Spawned};
pub use crate::protocol::{
    BadOutput, CALL_RESULT_TYPE, CALL_TYPE, ERROR_TYPE, EXTRACT_RESULT_TYPE, EXTRACT_TYPE,
    PluginCode, PluginFailure,
};
