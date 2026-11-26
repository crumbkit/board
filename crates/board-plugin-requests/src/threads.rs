//! `requests.threads`: the thread of every request with a pending version,
//! each as `requests.thread` returns it, from one read of the effects and
//! one of the replies.

use std::collections::BTreeMap;

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::thread::{self, assemble, effects, replies, waits};

/// `requests.threads` takes no input.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ThreadsInput")]
pub(crate) struct Input {}

/// Every request with a pending version, ordered by board, plugin, op and
/// path. Each thread is what `requests.thread` returns for any of its
/// versions.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ThreadsOutput")]
pub(crate) struct Output {
    threads: Vec<thread::Output>,
}

pub(crate) fn threads(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {} = PluginError::read_input("requests", "threads", input)?;
    let (effects, answers) = effects(host)?;
    let mut listed: BTreeMap<_, thread::Output> = BTreeMap::new();
    for (id, (effect, _)) in &effects {
        if !waits(&effects, &answers, id) {
            continue;
        }
        let key = (
            &effect.board,
            effect.plugin.to_string(),
            &effect.op,
            &effect.path,
        );
        listed
            .entry(key)
            .or_insert_with(|| assemble(&effects, &answers, effect));
    }
    let mut threads: Vec<thread::Output> = listed.into_values().collect();
    if !threads.is_empty() {
        let all: Vec<&thread::Output> = threads.iter().collect();
        for reply in replies(host, &all)? {
            if let Some(thread) = threads.iter_mut().find(|thread| thread.answers(&reply)) {
                thread.replies.push(reply);
            }
        }
    }
    Ok(serde_json::to_value(Output { threads }).expect("Output holds only JSON values"))
}

#[cfg(test)]
mod tests;
