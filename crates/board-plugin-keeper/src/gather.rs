//! `gather`: the live facts a consolidation pass needs.

use board_ops::{Host, PluginError};
use serde_json::{Value, json};

use crate::host::call;

pub(crate) fn gather(host: &dyn Host, _input: &Value) -> Result<Value, PluginError> {
    let changes = call(host, "changes.open", &json!({}))?;
    let tasks = call(host, "tasks.tree", &json!({"root": true, "repo": "/"}))?;
    Ok(json!({"changes": changes, "tasks": tasks}))
}
