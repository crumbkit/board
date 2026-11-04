//! The boundary in the personal board: checking content before it is sent,
//! recording what GitHub returned, naming the boundaries that cover a
//! write, and the checkout's mount, which a boundary's `from` names.

use std::path::Path;

use board_model::Refusal;
use board_ops::{Elsewhere, Host, PluginError};
use board_plugin::Target;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::parts::Parts;

/// The fields Board adds to a plugin's refusal, which it adds again to the
/// refusal this plugin passes on.
const ADDED: [&str; 2] = ["plugin", "stderr"];

/// The personal board, which holds the boundaries and the crossings, or
/// `github::board::refused` when none is named or it cannot be opened.
pub(crate) fn personal(host: &dyn Host) -> Result<Box<dyn Host + '_>, PluginError> {
    host.board(&Elsewhere::Personal)
        .map_err(|refusal| PluginError::board_refused("github", "--personal", &refusal))
}

/// What a passing check returned that every action reports.
#[derive(Debug, Deserialize)]
pub(crate) struct Passed {
    pub(crate) digest: String,
    pub(crate) crossing: String,
    pub(crate) note: Option<String>,
}

/// Runs `boundary.<capability>` in the personal board with `input`.
fn call(personal: &dyn Host, capability: &str, input: &Value) -> Result<Value, Box<Refusal>> {
    let target: Target = format!("boundary.{capability}")
        .parse()
        .expect("boundary.<capability> is <plugin>.<capability>");
    personal.call(&target, input).map(|report| report.output)
}

/// The boundary's check of `parts` of `action` to `to` from the checkout
/// `from`, or its refusal passed on as `github::boundary::refused`.
///
/// Board requires this plugin's codes to start with `github::`, so the
/// boundary's message, fields and hint are kept, with the boundary's code
/// in `code` and any code the boundary carried in `cause`. Any other
/// refusal is `github::board::refused`.
pub(crate) fn check(
    personal: &dyn Host,
    action: &str,
    to: &str,
    from: Option<&str>,
    parts: &Parts,
) -> Result<Passed, PluginError> {
    let mut input = json!({"action": action, "to": to, "parts": parts});
    if let Some(from) = from {
        input["from"] = json!(from);
    }
    let output = call(personal, "check", &input).map_err(|refusal| passed_on(&refusal))?;
    Passed::deserialize(&output).map_err(|error| unreadable("check", &error))
}

/// The boundary's refusal as this plugin's own.
fn passed_on(refusal: &Refusal) -> PluginError {
    let code = refusal.code.as_str();
    if !code.starts_with("boundary::") {
        return PluginError::board_refused("github", "--personal call", refusal);
    }
    let mut fields = refusal.fields.clone();
    for added in ADDED {
        fields.remove(added);
    }
    if let Some(cause) = fields.remove("code") {
        fields.insert("cause".to_owned(), cause);
    }
    fields.insert("code".to_owned(), Value::from(code));
    PluginError {
        code: "github::boundary::refused".to_owned(),
        message: refusal.message.clone(),
        fields,
        hint: refusal.hint.clone(),
    }
}

/// `github::boundary::unreadable`: `boundary.<capability>` answered in a
/// shape this plugin does not read, which happens only when the two come
/// from different Board builds.
fn unreadable(capability: &str, error: &serde_json::Error) -> PluginError {
    PluginError {
        code: "github::boundary::unreadable".to_owned(),
        message: format!("boundary.{capability} gave output this plugin cannot read: {error}"),
        fields: serde_json::Map::new(),
        hint: Some("update Board so its github and boundary plugins agree".to_owned()),
    }
}

/// Records `result`, what GitHub returned, on the crossing of `digest`.
pub(crate) fn record(personal: &dyn Host, digest: &str, result: &str) -> Result<(), PluginError> {
    let input = json!({"digest": digest, "result": result});
    call(personal, "record", &input)
        .map(|_| ())
        .map_err(|refusal| PluginError::board_refused("github", "--personal call", &refusal))
}

/// The boundaries `boundary.covers` names for `action` to `to` from `from`.
pub(crate) fn covers(
    personal: &dyn Host,
    action: &str,
    to: &str,
    from: Option<&str>,
) -> Result<Vec<String>, PluginError> {
    let mut input = json!({"action": action, "to": to});
    if let Some(from) = from {
        input["from"] = json!(from);
    }
    let output = call(personal, "covers", &input).map_err(|refusal| passed_on(&refusal))?;
    Vec::<String>::deserialize(&output["boundaries"]).map_err(|error| unreadable("covers", &error))
}

/// The checkout's path in the personal board, or none when the personal
/// board mounts it no place or more than one.
pub(crate) fn mount(personal: &dyn Host, top: &Path) -> Result<Option<String>, PluginError> {
    let places = (personal.places(top))
        .map_err(|refusal| PluginError::board_refused("github", "--personal refs", &refusal))?;
    Ok(match places.as_slice() {
        [place] => Some(place.to_string()),
        _ => None,
    })
}
