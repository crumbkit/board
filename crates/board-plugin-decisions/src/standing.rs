//! Every decision a board and its mounted boards hold, read with one query,
//! and which of them a later decision supersedes. The evals plugin reads
//! them here too.

use std::collections::HashSet;

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError};
use board_plugin::shape::TREE_CODES;
use board_plugin_plans::{Instant, instant};
use serde::Deserialize;
use serde_json::Value;

/// Every decision, by path.
pub const DECISIONS: &str = "SELECT logical, code, body FROM records \
     WHERE type = 'decisions/decision/v1' AND logical IS NOT NULL";

/// A decision as this plugin reads it, and whether a decision supersedes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub path: String,
    pub decision: String,
    pub why: String,
    pub decided: String,
    pub when: Instant,
    pub basis: Vec<String>,
    pub urls: Vec<String>,
    pub superseded: bool,
}

/// A decision as a mounted board's own, possibly looser, schema may admit
/// it. An unknown field is ignored.
#[derive(Deserialize)]
struct Stored {
    decision: String,
    why: String,
    decided: String,
    #[serde(rename = "session")]
    _session: String,
    #[serde(default)]
    basis: Vec<String>,
    #[serde(default)]
    urls: Vec<String>,
    #[serde(default)]
    supersedes: Vec<String>,
}

/// Every readable decision, in no order. A decision Board marks degraded
/// for anything but the tree is left out, as is one this plugin cannot
/// read. `plugin` names the caller in the code of a refused query.
///
/// A decision is superseded when any readable decision lists its path in
/// `supersedes`. Paths are compared as written, so a reference through a
/// link, or one a mounted board's decision makes to its own board, matches
/// nothing; decisions live in the owner's board, where neither happens.
pub fn decisions(host: &dyn Host, plugin: &str) -> Result<Vec<Decision>, PluginError> {
    let report = (host.query(DECISIONS, Limit::ALL))
        .map_err(|error| PluginError::board_refused(plugin, "query", &error.refusal()))?;
    let mut found = Vec::new();
    let mut superseded = HashSet::new();
    for row in &report.rows {
        let [Value::String(path), code, body] = &row[..] else {
            continue;
        };
        let tree = code.is_null() || TREE_CODES.iter().any(|tree| code == *tree);
        let Some((stored, when)) = tree.then(|| readable(body)).flatten() else {
            continue;
        };
        superseded.extend(stored.supersedes);
        found.push(Decision {
            path: path.clone(),
            decision: stored.decision,
            why: stored.why,
            decided: stored.decided,
            when,
            basis: stored.basis,
            urls: stored.urls,
            superseded: false,
        });
    }
    for decision in &mut found {
        decision.superseded = superseded.contains(&decision.path);
    }
    Ok(found)
}

/// `None` when `body` is not a decision this plugin reads: one whose
/// `decided` is not a time.
fn readable(body: &Value) -> Option<(Stored, Instant)> {
    let record = parse_json(body.as_str()?.as_bytes()).ok()?;
    let stored = Stored::deserialize(&record).ok()?;
    let when = instant(&stored.decided)?;
    Some((stored, when))
}
