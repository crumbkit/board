//! Reading and writing the board through the host, with Board's refusals as
//! `keeper::board::refused`.

use board_model::{LogicalPath, Object, Refusal};
use board_ops::{Host, Limit, PluginError, literal};
use board_plugin::Target;
use serde_json::Value;

use crate::{PLUGIN, refusal};

/// `keeper::board::refused`, for Board's refusal of `board <command>`.
pub(crate) fn refused(command: &str, refusal: &Refusal) -> PluginError {
    PluginError::board_refused(PLUGIN, command, refusal)
}

/// What `<capability>` of another plugin outputs for `input`, or the
/// refusal as `keeper::gather::source_failed` naming the capability and its
/// code.
pub(crate) fn call(host: &dyn Host, capability: &str, input: &Value) -> Result<Value, PluginError> {
    let target: Target = capability
        .parse()
        .expect("each capability called is <plugin>.<capability>");
    host.call(&target, input)
        .map(|report| report.output)
        .map_err(|failed| {
            refusal(
                "keeper::gather::source_failed",
                format!(
                    "{capability} refused with {}: {}; fix that capability's input or the board it \
                     reads, then run again",
                    failed.code.as_str(),
                    failed.message
                ),
                [("capability", Value::from(capability))],
            )
        })
}

/// Every row `sql` selects.
pub(crate) fn rows(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    let report =
        (host.query(sql, Limit::ALL)).map_err(|error| refused("query", &error.refusal()))?;
    if report.truncated {
        return Err(refusal(
            "keeper::board::over_budget",
            "a query's rows passed the output budget of one board run \
             (board::output::over_budget); select fewer or smaller columns"
                .to_owned(),
            [],
        ));
    }
    Ok(report.rows)
}

/// The text of a column, or none for SQL null or any other value.
pub(crate) fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// The stored text of the file at `path`, or none when Board reads nothing
/// there.
pub(crate) fn get_text(host: &dyn Host, path: &str) -> Result<Option<String>, PluginError> {
    let Ok(path) = path.parse::<LogicalPath>() else {
        return Ok(None);
    };
    let answers =
        (host.get_each(&[path])).map_err(|error| refused("get", &error.refusal("get")))?;
    Ok(answers.into_iter().next().and_then(|answer| match answer {
        Ok(report) if matches!(report.location.object, Object::Blob(_)) => {
            Some(String::from_utf8_lossy(&report.bytes).into_owned())
        }
        _ => None,
    }))
}

/// Writes each of `lines`, one line of `board put --batch`, in one commit,
/// or nothing.
pub(crate) fn put(host: &dyn Host, lines: &[Value]) -> Result<(), PluginError> {
    (host.put(lines))
        .map(|_| ())
        .map_err(|error| refused("put --batch", &error.refusal()))
}

/// The blob of the file at the logical `path`, or `none` when nothing is
/// there, as a `put` line expects it.
pub(crate) fn blob_at(host: &dyn Host, path: &str) -> Result<String, PluginError> {
    let sql = format!("SELECT blob FROM entries WHERE logical = {}", literal(path));
    Ok(rows(host, &sql)?
        .first()
        .and_then(|row| text(row.first()))
        .unwrap_or("none")
        .to_owned())
}
