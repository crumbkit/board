//! `for-path`: the controls that apply to one path.

use std::collections::BTreeSet;

use board_model::Code;
use board_ops::{Absent, Host, PluginError, RefsReport, source_name};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::index::{self, FILES, literal};
use crate::logical;
use crate::records::Budget;

/// The controls that apply to this logical path.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    /// The file to list the controls of, by its logical path, such as /docs/README.md; ask about a new file by the path it will have.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: String,
}

/// The controls that apply to the path, by the control's path.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ForPathOutput")]
pub(crate) struct Output {
    controls: Vec<Applying>,
}

/// A control that applies to the path asked about.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Applying {
    control: String,
    requirement: String,
    purpose: String,
    rule: Option<Budget>,
}

/// Each readable control whose `applies_to` reaches the path or whose
/// `paths` match it, by the control's path.
///
/// A pattern matches the file the path resolves to, through any link or
/// mount, as `check` does: by the display path Board gives it, whichever
/// path was asked. A path with nothing there yet, such as a file about to be
/// written, is matched as asked, where Board would place a file there. A
/// path beneath a file, where nothing can be, matches no pattern.
pub(crate) fn for_path(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path } = PluginError::read_input("controls", "for-path", input)?;
    let asked = [logical(&path)?];
    let mut controls = index::controls(host)?;
    let refused = |error: &board_ops::RefsError| {
        PluginError::board_refused("controls", "refs", &error.refusal())
    };
    let answers = host.refs(&asked, Absent::Locate).map_err(|e| refused(&e))?;
    let [answer] = <[_; 1]>::try_from(answers).expect("refs answers each path it is asked once");
    let mut applying = BTreeSet::new();
    match answer {
        Ok(report) => {
            applying.extend(referencing(&report));
            applying.extend(matching(host, &report)?);
        }
        // Only a path beneath a file names nothing, and nothing can be written there.
        Err(error) if error.code() == Code::PathNotFound => {}
        Err(error) => return Err(refused(&error)),
    }
    let controls = (applying.into_iter())
        .filter_map(|logical| {
            let control = controls.remove(&logical)?;
            Some(Applying {
                control: logical,
                requirement: control.requirement,
                purpose: control.purpose,
                rule: control.rule,
            })
        })
        .collect();
    let output = Output { controls };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and lists"))
}

/// The records whose `applies_to` reaches the path `report` answers.
fn referencing(report: &RefsReport) -> impl Iterator<Item = String> + '_ {
    (report.to.iter())
        .filter(|incoming| {
            (incoming.pointers.iter()).any(|pointer| pointer.starts_with("/applies_to/"))
        })
        .filter_map(|incoming| Some(incoming.logical.as_ref()?.to_string()))
}

/// The controls whose patterns match the file `report` resolves to, or,
/// when nothing is there yet, the path where a file would be. Only an
/// existing file has a row in `files`.
fn matching(host: &dyn Host, report: &RefsReport) -> Result<Vec<String>, PluginError> {
    let source = source_name(&report.location.board.source);
    let (place, display) = match &report.missing {
        Some(missing) => (missing.path.as_str(), missing.display.to_string()),
        None => (report.location.path.as_str(), report.display.to_string()),
    };
    let sql = format!(
        "WITH files AS {FILES} SELECT DISTINCT r.logical FROM records r, json_each(r.body, '$.paths') p \
         WHERE r.type = 'controls/control/v1' AND r.source = 'worktree' AND COALESCE(\
         (SELECT full FROM files WHERE source = {} AND path = {}), {}) GLOB p.value",
        literal(&source),
        literal(place),
        literal(display.strip_prefix('/').unwrap_or(&display)),
    );
    let report = index::rows(host, &sql)?;
    Ok((report.rows.into_iter())
        .filter_map(|row| match row.into_iter().next() {
            Some(Value::String(logical)) => Some(logical),
            _ => None,
        })
        .collect())
}
