//! `render`: the status board page.

use board_ops::{Host, PluginError};
use serde_json::{Value, json};

pub(crate) fn render(host: &dyn Host, _input: &Value) -> Result<Value, PluginError> {
    let line =
        json!({"path": "/projects/keeper/work-view/index.html", "text": "<!doctype html>\n"});
    host.put(&[line])
        .map_err(|error| PluginError::board_refused(crate::PLUGIN, "put", &error.refusal()))?;
    Ok(json!({"path": "/projects/keeper/work-view/index.html"}))
}
