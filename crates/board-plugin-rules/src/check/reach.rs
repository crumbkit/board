//! The files a set of rules reaches under a path, which `check` and
//! `report` measure.

use std::collections::{BTreeMap, BTreeSet};

use board_model::LogicalPath;
use board_ops::{Host, PluginError};

use super::{Unreadable, referenced, under};
use crate::index::{self, Control, File, Health};

/// What `controls` reach under `scope`: the files each reaches there by
/// display path, how many files with no display path each reaches, which
/// is counted only under `/`, the files the index marks degraded by
/// control, and the controls that apply, which under `/` is every one and
/// under another path those reaching a file there.
pub(crate) struct Reached {
    pub(crate) files: BTreeMap<String, BTreeSet<LogicalPath>>,
    pub(crate) unnamed: BTreeMap<String, usize>,
    pub(crate) unreadable: BTreeMap<String, Vec<Unreadable>>,
    pub(crate) applying: Vec<String>,
}

/// The files `controls` reach under `scope`, through their references and
/// their patterns, as [`super::check`] measures them.
pub(crate) fn reach(
    host: &dyn Host,
    scope: &LogicalPath,
    controls: &BTreeMap<String, Control>,
) -> Result<Reached, PluginError> {
    let root = *scope == LogicalPath::root();
    let named: Vec<&str> = (controls.iter())
        .filter(|(_, control)| control.applies_to.as_ref().is_some_and(|to| !to.is_empty()))
        .map(|(logical, _)| logical.as_str())
        .collect();
    let mut files = referenced(host, &named)?;
    for logical in controls.keys() {
        files.entry(logical.clone()).or_default();
    }
    // The files Board's index marks degraded, by control, under the path.
    let mut unreadable: BTreeMap<String, Vec<Unreadable>> = BTreeMap::new();
    // Files with no display path, by control. They lie under no path but `/`.
    let mut unnamed: BTreeMap<String, usize> = BTreeMap::new();
    for found in index::matched(host, scope, controls)? {
        let Some(reached) = files.get_mut(&found.control) else {
            continue;
        };
        let (shown, inside) = match found.file {
            File::Named(path) => {
                let inside = under(&path, scope);
                let shown = path.to_string();
                reached.insert(path);
                (shown, inside)
            }
            File::Unnamed(path) => {
                *unnamed.entry(found.control.clone()).or_default() += 1;
                (path, root)
            }
        };
        if let (Health::Degraded(code), true) = (found.health, inside) {
            (unreadable.entry(found.control).or_default()).push(Unreadable { file: shown, code });
        }
    }
    for reached in files.values_mut() {
        reached.retain(|path| under(path, scope));
    }
    // Under the root every control is listed, so one whose pattern reaches nothing shows with no files.
    let applying = (controls.keys())
        .filter(|logical| root || !files[*logical].is_empty())
        .cloned()
        .collect();
    Ok(Reached {
        files,
        unnamed,
        unreadable,
        applying,
    })
}
