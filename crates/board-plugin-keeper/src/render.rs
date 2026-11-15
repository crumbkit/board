//! `render`: the status board page.

use board_ops::{Host, PluginError};
use serde_json::{Value, json};

use crate::host::put;

pub(crate) fn render(host: &dyn Host, _input: &Value) -> Result<Value, PluginError> {
    let path = "/projects/keeper/work-view/index.html";
    put(host, &[json!({"path": path, "text": "<!doctype html>\n"})])?;
    Ok(json!({"path": path}))
}
