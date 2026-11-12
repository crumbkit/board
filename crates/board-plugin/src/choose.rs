//! Choosing the plugin and capability a call names (docs/plugins.md,
//! Finding plugins).

use board_model::{CapabilityName, PluginName, RecordType};
use serde_json::Value;

use crate::call::{CallError, Target};
use crate::former;
use crate::installed::{Installed, Origin};
use crate::manifest::{Capability, Manifest};

/// The plugin and capability a call runs.
#[derive(Debug)]
pub struct Chosen {
    pub manifest: Manifest,
    pub origin: Origin,
    /// Each type the plugin ships and its schema, as `Installed` holds them.
    pub schemas: Vec<(RecordType, Value)>,
    capability: usize,
    /// What the call printed beside its output: each former name that
    /// reached the plugin, as `board::plugin::former_name` lines.
    pub warnings: Vec<String>,
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
            let (found, renamed) = former::named(installed, plugin, capability)?;
            let manifest = found.manifest.map_err(|source| CallError::BadManifest {
                plugin: plugin.clone(),
                source,
            })?;
            let index = offered(&manifest, capability).ok_or_else(|| CallError::NoCapability {
                plugin: manifest.name.clone(),
                capability: capability.clone(),
                offered: manifest
                    .capabilities
                    .iter()
                    .map(|offered| offered.name.clone())
                    .collect(),
            })?;
            let Some(entry) = manifest.enabling(enabled) else {
                return Err(CallError::NotEnabled {
                    plugin: manifest.name,
                });
            };
            let mut chosen = Chosen {
                manifest,
                origin: found.origin,
                schemas: found.schemas,
                capability: index,
                warnings: Vec::new(),
            };
            chosen.warnings = former::warnings(&chosen, renamed.then_some(plugin), entry);
            Ok(chosen)
        }
        Target::Capability(capability) => {
            let manifests: Vec<_> = installed
                .into_iter()
                .filter_map(|installed| {
                    Some((
                        installed.manifest.ok()?,
                        installed.origin,
                        installed.schemas,
                    ))
                })
                .collect();
            let nearest = nearest(capability, manifests.iter().map(|(manifest, ..)| manifest));
            let (on, off): (Vec<_>, Vec<_>) = manifests
                .into_iter()
                .filter_map(|(manifest, origin, schemas)| {
                    let index = offered(&manifest, capability)?;
                    Some(Chosen {
                        manifest,
                        origin,
                        schemas,
                        capability: index,
                        warnings: Vec::new(),
                    })
                })
                .partition(|chosen| chosen.manifest.enabling(enabled).is_some());
            let mut on = on.into_iter();
            match (on.next(), on.next(), off.into_iter().next()) {
                (Some(mut chosen), None, _) => {
                    if let Some(entry) = chosen.manifest.enabling(enabled) {
                        chosen.warnings = former::warnings(&chosen, None, entry);
                    }
                    Ok(chosen)
                }
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
pub(crate) fn nearest<'a>(
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
