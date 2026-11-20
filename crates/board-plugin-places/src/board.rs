//! Reading and writing the board for every capability: one entry by its
//! path, a batch of puts, and the plugin's own refusals.

use board_model::{Code, Fields, LogicalPath, Object, PathError, Refusal};
use board_ops::{GetReport, Host, PluginError, PutReport};
use serde_json::{Map, Value};

/// `places::board::refused`, for Board's refusal of `board <command>`.
pub(crate) fn refused(command: &str, refusal: &Refusal) -> PluginError {
    PluginError::board_refused("places", command, refusal)
}

/// A refusal of the plugin's own, with `fields`.
pub(crate) fn refusal(code: &str, message: String, fields: &[(&str, &str)]) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields: (fields.iter())
            .map(|(name, value)| ((*name).to_owned(), Value::from(*value)))
            .collect::<Map<_, _>>(),
        hint: None,
    }
}

/// `path` as a logical path, refused as `board get` would refuse it.
pub(crate) fn logical(path: &str) -> Result<LogicalPath, PluginError> {
    path.parse().map_err(|error: PathError| {
        refused(
            "get",
            &Refusal::new(error.code(), &error, &Fields::default()),
        )
    })
}

/// What one path holds.
pub(crate) enum Held {
    /// Nothing.
    Nothing,
    /// A directory or a file that is not a record.
    Other,
    /// A record and its blob.
    Record(Value, String),
}

/// What `path` holds.
pub(crate) fn get(host: &dyn Host, path: &LogicalPath) -> Result<Held, PluginError> {
    let answers = (host.get_each(std::slice::from_ref(path)))
        .map_err(|error| refused("get", &error.refusal("get")))?;
    match answers.into_iter().next() {
        Some(Ok(report)) => held(report),
        Some(Err(error)) if error.code() == Code::PathNotFound => Ok(Held::Nothing),
        Some(Err(error)) if error.code() == Code::RecordNotARecord => Ok(Held::Other),
        Some(Err(error)) => Err(refused("get", &error.refusal("get"))),
        // Board answers a get of one path once; this stands in for an
        // answer it never leaves out.
        None => Err(refusal(
            "places::board::refused",
            format!("board get gave no answer for {path}"),
            &[("path", path.as_str())],
        )),
    }
}

/// What one answer of `get_each` holds.
pub(crate) fn held(report: GetReport) -> Result<Held, PluginError> {
    let blob = match &report.location.object {
        Object::Blob(id) => id.to_string(),
        Object::Directory => return Ok(Held::Other),
    };
    let record = report.record.map_err(|refusal| refused("get", &refusal))?;
    Ok(Held::Record(record, blob))
}

/// Writes `lines` in one commit.
pub(crate) fn put(host: &dyn Host, lines: &[Value]) -> Result<PutReport, PluginError> {
    host.put(lines)
        .map_err(|error| refused("put --batch", &error.refusal()))
}
