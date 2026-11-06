//! The controls that bear on a change, read through the controls plugin by
//! a nested call and from the board's index: the obligations on a plan's
//! paths or a mount, and the files of a diff that a named control lists.

use board_ops::{Host, PluginError, literal};
use serde_json::{Value, json};

use crate::board::{body, call_or_refusal, readable, refused, rows, text};

/// The record type of a control.
const CONTROL_TYPE: &str = "controls/control/v1";
/// The codes Board gives a call to a plugin it does not offer.
pub(crate) const CONTROLS_UNAVAILABLE: [&str; 2] =
    ["board::plugin::not_installed", "board::plugin::not_enabled"];

/// The controls `controls.for-paths` finds on `paths`, with their summary,
/// or why the controls plugin could not be asked.
fn for_paths(
    host: &dyn Host,
    paths: &[String],
) -> Result<Result<Vec<(String, String)>, String>, PluginError> {
    if paths.is_empty() {
        return Ok(Ok(Vec::new()));
    }
    let output = match call_or_refusal(host, "controls.for-paths", &json!({"paths": paths})) {
        Ok(output) => output,
        Err(refusal) if CONTROLS_UNAVAILABLE.contains(&refusal.code.as_str()) => {
            return Ok(Err(format!(
                "the obligations on the plan's paths are read with controls.for-paths, which this \
                 Board refused: {}; enable it with `board plugin enable controls`",
                refused("call", &refusal).message
            )));
        }
        Err(refusal) => return Err(refused("call", &refusal)),
    };
    Ok(Ok((output
        .get("controls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|control| {
        Some((
            control.get("control")?.as_str()?.to_owned(),
            control.get("summary")?.as_str()?.to_owned(),
        ))
    })
    .collect()))
}

/// Which of `controls` are obligations, by the `kind` their records hold.
fn obligations_among(host: &dyn Host, controls: &[String]) -> Result<Vec<String>, PluginError> {
    if controls.is_empty() {
        return Ok(Vec::new());
    }
    let listed: Vec<String> = controls.iter().map(|each| literal(each)).collect();
    let sql = format!(
        "SELECT logical FROM records WHERE type = {} AND logical IN ({}) AND \
         json_extract(body, '$.kind') = 'obligation' ORDER BY logical",
        literal(CONTROL_TYPE),
        listed.join(", ")
    );
    Ok((rows(host, &sql)?.iter())
        .filter_map(|row| text(row.first()).map(str::to_owned))
        .collect())
}

/// The obligation controls on `paths`, a plan's files and patterns, by
/// path, or why they could not be read.
pub(crate) fn obligations(
    host: &dyn Host,
    paths: &[String],
) -> Result<Result<Vec<String>, String>, PluginError> {
    let found = match for_paths(host, paths)? {
        Ok(found) => found,
        Err(why) => return Ok(Err(why)),
    };
    let controls: Vec<String> = found.into_iter().map(|(control, _)| control).collect();
    Ok(Ok(obligations_among(host, &controls)?))
}

/// The obligation controls on anything under `mount`, by path, with each
/// one's summary; none when the controls plugin is not enabled.
pub(crate) fn on_mount(host: &dyn Host, mount: &str) -> Result<Vec<(String, String)>, PluginError> {
    let pattern = format!("{}/**", mount.trim_end_matches('/'));
    let Ok(found) = for_paths(host, &[pattern])? else {
        return Ok(Vec::new());
    };
    let controls: Vec<String> = found.iter().map(|(control, _)| control.clone()).collect();
    let owed = obligations_among(host, &controls)?;
    Ok(found
        .into_iter()
        .filter(|(control, _)| owed.contains(control))
        .collect())
}

/// The files of `edited`, by logical path, that the control at `control`
/// lists in its `paths` or `applies_to`, or none when the board holds no
/// control there that Board reads.
pub(crate) fn listed_by(
    host: &dyn Host,
    control: &str,
    edited: &[String],
) -> Result<Option<Vec<String>>, PluginError> {
    let sql = format!(
        "SELECT code, body FROM records WHERE logical = {} AND type = {}",
        literal(control),
        literal(CONTROL_TYPE)
    );
    let found = rows(host, &sql)?;
    let Some(row) = found.first().filter(|row| readable(row.first())) else {
        return Ok(None);
    };
    let record = body(row.get(1));
    let list = |key: &str| -> Vec<String> {
        (record
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten())
        .filter_map(|each| each.as_str().map(str::to_owned))
        .collect()
    };
    let (patterns, named) = (list("paths"), list("applies_to"));
    if edited.is_empty() {
        return Ok(Some(Vec::new()));
    }
    // A control's paths are GLOB patterns on the path without its leading
    // slash, as the controls plugin matches them.
    let bare: Vec<&str> = edited
        .iter()
        .map(|each| each.trim_start_matches('/'))
        .collect();
    let listed = |each: &Value| literal(&each.to_string());
    let sql = format!(
        "SELECT '/' || f.value FROM json_each({}) AS f WHERE EXISTS (SELECT 1 FROM json_each({}) \
         AS p WHERE f.value GLOB p.value) ORDER BY f.key",
        listed(&json!(bare)),
        listed(&json!(patterns))
    );
    let mut touched: Vec<String> = (rows(host, &sql)?.iter())
        .filter_map(|row| text(row.first()).map(str::to_owned))
        .collect();
    for each in edited {
        if named.contains(each) && !touched.contains(each) {
            touched.push(each.clone());
        }
    }
    Ok(Some(touched))
}
