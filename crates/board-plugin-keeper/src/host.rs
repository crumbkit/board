//! Reading and writing the board through the host, with Board's refusals as
//! `keeper::board::refused`.

use board_ops::{Host, PluginError};
use board_plugin::Target;
use serde_json::Value;

use crate::PLUGIN;

/// What `<capability>` of another plugin outputs for `input`, or Board's
/// refusal of the call as `keeper::board::refused`.
pub(crate) fn call(host: &dyn Host, capability: &str, input: &Value) -> Result<Value, PluginError> {
    let target: Target = capability
        .parse()
        .expect("each capability called is <plugin>.<capability>");
    host.call(&target, input)
        .map(|report| report.output)
        .map_err(|refusal| PluginError::board_refused(PLUGIN, "call", &refusal))
}
