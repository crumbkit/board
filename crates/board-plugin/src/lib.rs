//! Plugins: manifests, discovery and child processes (docs/plugins.md).

mod call;
mod installed;
mod manifest;
mod process;
mod protocol;

pub use crate::call::{
    CallError, CallReport, Chosen, Context, Environment, MAX_DEPTH, Target, choose, invoke,
};
pub use crate::installed::{DiscoveryError, Installed, MANIFEST_FILE, installed};
pub use crate::manifest::{
    Capability, CapabilityName, MANIFEST_TYPE, Manifest, ManifestError, MetaFilter,
    MetaFilterError, NameError, PluginName, Side, Version, VersionError,
};
pub use crate::process::Limits;
pub use crate::protocol::{
    BadOutput, CALL_RESULT_TYPE, CALL_TYPE, ERROR_TYPE, PluginCode, PluginFailure,
};
