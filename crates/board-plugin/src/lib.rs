//! Plugins: manifests and discovery (docs/plugins.md).

mod installed;
mod manifest;

pub use crate::installed::{DiscoveryError, Installed, MANIFEST_FILE, installed};
pub use crate::manifest::{
    Capability, CapabilityName, MANIFEST_TYPE, Manifest, ManifestError, MetaFilter,
    MetaFilterError, NameError, PluginName, Side, Version, VersionError,
};
