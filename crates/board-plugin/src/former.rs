//! Reaching a renamed plugin by a name its manifest lists in `former_names`
//! (docs/plugins/calling/commands.md, Renamed plugins).

use board_model::{CapabilityName, Code, PluginName};

use crate::call::CallError;
use crate::choose::Chosen;
use crate::installed::Installed;

/// The install `plugin` names: the one of that name, or else the one whose
/// manifest lists `plugin` among its former names, and whether it was the
/// latter. A name that both a plugin and another's former names hold, or
/// that two plugins' former names hold, is refused, listing the plugins
/// that hold it as a former name first.
pub(crate) fn named(
    installed: Vec<Installed>,
    plugin: &PluginName,
    capability: &CapabilityName,
) -> Result<(Installed, bool), CallError> {
    let (mut by_name, mut by_former) = (None, Vec::new());
    for one in installed {
        if one.name == plugin.as_str() {
            by_name = Some(one);
        } else if let Ok(manifest) = &one.manifest
            && manifest.former_names.contains(plugin)
        {
            let name = manifest.name.clone();
            by_former.push((one, name));
        }
    }
    match (by_name, by_former.len()) {
        (Some(found), 0) => Ok((found, false)),
        (None, 0) => Err(CallError::NotInstalled {
            plugin: plugin.clone(),
        }),
        (None, 1) => Ok((by_former.remove(0).0, true)),
        (by_name, _) => Err(CallError::FormerNameTaken {
            name: plugin.clone(),
            capability: capability.clone(),
            plugins: (by_former.into_iter().map(|(_, name)| name))
                .chain(by_name.map(|_| plugin.clone()))
                .collect(),
        }),
    }
}

/// The warnings that `called`, a former name the call gave, reached
/// `chosen`, and that `entry`, the board's `plugins` entry enabling it, is
/// a former name; none for the plugin's own name.
pub(crate) fn warnings(
    chosen: &Chosen,
    called: Option<&PluginName>,
    entry: &PluginName,
) -> Vec<String> {
    let (name, capability) = (&chosen.manifest.name, &chosen.capability().name);
    let code = Code::PluginFormerName.as_str();
    let called = called.map(|former| {
        format!(
            "{code}: {former} is the former name of the {name} plugin; call it as `board call {name}.{capability}`"
        )
    });
    let enabled = (entry != name).then(|| {
        format!(
            "{code}: the board's .board/board.json enables {entry}, the former name of the {name} plugin; `board plugin enable {name}` adds its name"
        )
    });
    called.into_iter().chain(enabled).collect()
}
