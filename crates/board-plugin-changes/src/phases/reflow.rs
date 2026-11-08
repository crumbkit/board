//! The `reflow` capability: moving a change to another flow.

use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{final_phase, refuse_v1};
use crate::board::{self, rows};
use crate::change::{OWN, line, read, touch};
use crate::flow::{
    FlowSummary, asked_for, blob_at, refuse_invalid, refuse_without_repository, version_line,
};
use crate::records::{BOARD_ITSELF, BUILD, PAUSED, RunRecord, is_final};
use crate::{PLUGIN, refusal, to_value};

/// A change and the flow to move it to.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReflowInput {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(
        regex(pattern = "^[a-z0-9]+(-[a-z0-9]+)*$"),
        description = "The flow to follow from now on: the board's own at /flows/<name>, or a \
                       shipped one, development or small."
    )]
    flow: String,
}

/// The change, the flow it follows now and the phase it stands in.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Reflowed {
    path: String,
    flow: FlowSummary,
    phase: String,
}

/// Moves the change to the current version of the flow named, which must
/// hold the phase the change stands in, writing that version when the board
/// lacks it; a paused change stands in build. A change not yet migrated
/// loses its own copy at `<path>/flow`, unless a run names that copy.
pub(crate) fn reflow(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let ReflowInput { path, flow } = PluginError::read_input(PLUGIN, "reflow", input)?;
    let (mut change, blob) = read(host, &path)?;
    refuse_v1(&path, &change, "flow version")?;
    if is_final(&change.phase) {
        return Err(final_phase(
            &path,
            &change.phase,
            format!(
                "the change at {path} is {}, so it follows no flow any more; list the open changes \
                 with `board changes open`",
                change.phase
            ),
        ));
    }
    let chosen = asked_for(host, &flow)?;
    refuse_invalid(&chosen, "reflow")?;
    if change.mount == BOARD_ITSELF {
        refuse_without_repository(&chosen, &change.mount)?;
    }
    let standing = if change.phase == PAUSED {
        BUILD
    } else {
        change.phase.as_str()
    };
    if chosen.phase(standing).is_none() {
        return Err(refusal(
            "changes::reflow::no_phase",
            format!(
                "the flow {flow} has no phase {standing}, where the change at {path} stands, so \
                 the change would have nowhere to be; choose a flow with a {standing} phase, \
                 whose phases `board changes next` lists"
            ),
            [
                ("path", json!(path)),
                ("flow", json!(flow)),
                ("phase", json!(standing)),
                ("phases", json!(chosen.names())),
            ],
        ));
    }
    let (version, written) = version_line(host, &chosen)?;
    let mut lines: Vec<Value> = written.into_iter().collect();
    let copy = format!("{path}/flow");
    if change.flow.as_deref() == Some(copy.as_str()) {
        let held = blob_at(host, &copy)?;
        if held != "none" && !run_names(host, &copy)? {
            lines.push(json!({"path": copy, "expect": held, "remove": true}));
        }
    }
    change.flow = Some(version);
    let change = touch(host, change);
    lines.push(line(&path, &blob, &change));
    board::put(host, &lines)?;
    Ok(to_value(Reflowed {
        path,
        flow: FlowSummary::from(&chosen),
        phase: change.phase,
    }))
}

/// Whether a run of the board's own changes names the flow at `path`,
/// whether or not Board reads that run as valid.
fn run_names(host: &dyn Host, path: &str) -> Result<bool, PluginError> {
    let sql = format!(
        "SELECT 1 FROM entries AS e JOIN bodies AS b ON b.blob = e.blob WHERE e.type = {} AND \
         substr(e.logical, 1, {}) = {} AND json_extract(b.body, '$.flow') = {} LIMIT 1",
        literal(RunRecord::TYPE),
        OWN.len(),
        literal(OWN),
        literal(path)
    );
    Ok(!rows(host, &sql)?.is_empty())
}
