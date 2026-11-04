//! `for-paths`: the controls that apply to any of several files and
//! patterns, such as the paths a change's plan declares.

use std::collections::{BTreeMap, BTreeSet};

use board_ops::{Host, PluginError, literal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::for_path;
use crate::index::{self, FILES};
use crate::standards;

/// The files and patterns to list the controls of.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    /// Files and `SQLite` GLOB patterns by logical path, such as `/repos/inbox/crates/inbox-store/**`, where `*` also matches `/`.
    #[schemars(length(min = 1), inner(regex(pattern = "^/")))]
    paths: Vec<String>,
}

/// Each control that applies to one of the paths, once, by the control's
/// path.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ForPathsOutput")]
pub(crate) struct Output {
    controls: Vec<Applying>,
}

/// A control, its summary, or else its requirement's first sentence cut to
/// 25 words, and the asked paths that reached it, in the order they were
/// asked.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Applying {
    control: String,
    summary: String,
    matched: Vec<String>,
}

/// Each readable control that applies to one of the paths.
///
/// A path with no `*`, `?` or `[` is a file, matched as `for-path` matches
/// it. Any other path is a pattern. A pattern reaches a control whose
/// `applies_to` names a path the pattern matches, and a control one of
/// whose `paths` matches a file the pattern matches, by the file's display
/// path. A pattern that matches no file yet is matched as if it were a
/// path, so `/crates/new/*.rs` reaches a control on `crates/*.rs`.
pub(crate) fn for_paths(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { paths } = PluginError::read_input("controls", "for-paths", input)?;
    let mut reached: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for path in paths.iter().filter(|path| seen.insert(*path)) {
        let applying = if path.contains(['*', '?', '[']) {
            globbed(host, path)?
        } else {
            for_path::applying(host, path)?
        };
        for control in applying {
            reached.entry(control).or_default().push(path.clone());
        }
    }
    let mut controls = index::controls(host)?;
    let controls = (reached.into_iter())
        .filter_map(|(logical, matched)| {
            let control = controls.remove(&logical)?;
            Some(Applying {
                control: logical,
                summary: (control.summary)
                    .unwrap_or_else(|| standards::summary(&control.requirement)),
                matched,
            })
        })
        .collect();
    let output = Output { controls };
    Ok(serde_json::to_value(output).expect("Output holds only strings and lists"))
}

/// The records in the board's own tree that the pattern `path` reaches.
fn globbed(host: &dyn Host, path: &str) -> Result<BTreeSet<String>, PluginError> {
    let pattern = literal(path.strip_prefix('/').unwrap_or(path));
    let sql = format!(
        "WITH planned AS MATERIALIZED (SELECT full FROM {FILES} WHERE full GLOB {pattern}) \
         SELECT r.logical FROM records r, json_each(r.body, '$.paths') p \
         WHERE r.type = 'controls/control/v1' AND r.source = 'worktree' \
         AND ({pattern} GLOB p.value OR EXISTS (SELECT 1 FROM planned WHERE full GLOB p.value)) \
         UNION SELECT r.logical FROM records r, json_each(r.body, '$.applies_to') a \
         WHERE r.type = 'controls/control/v1' AND r.source = 'worktree' \
         AND substr(a.value, 2) GLOB {pattern}"
    );
    let report = index::rows(host, &sql)?;
    Ok((report.rows.into_iter())
        .filter_map(|row| match row.into_iter().next() {
            Some(Value::String(logical)) => Some(logical),
            _ => None,
        })
        .collect())
}
