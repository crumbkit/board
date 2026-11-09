//! Reading and writing the board through the host, with Board's refusals
//! as `changes::board::refused`, as the Python plugin's helper gave them.

use board_model::{Code, Fields, LogicalPath, Object, PathError, Refusal, parse_json};
use board_ops::{Absent, Bulk, Host, Limit, PluginError, RefsError, RefsReport, literal};
use board_plugin::Target;
use board_plugin::shape::TREE_CODES;
use serde_json::{Map, Value};

use crate::{PLUGIN, refusal};

/// `changes::board::refused`, for Board's refusal of `board <command>`.
pub(crate) fn refused(command: &str, refusal: &Refusal) -> PluginError {
    PluginError::board_refused(PLUGIN, command, refusal)
}

/// `path` as a logical path, refused as `board <command>` would refuse it.
pub(crate) fn logical(path: &str, command: &str) -> Result<LogicalPath, PluginError> {
    path.parse().map_err(|error: PathError| {
        refused(
            command,
            &Refusal::new(error.code(), &error, &Fields::default()),
        )
    })
}

/// Every row `sql` selects. Rows Board leaves out at its output budget are
/// `changes::board::over_budget`.
pub(crate) fn rows(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    let report =
        (host.query(sql, Limit::ALL)).map_err(|error| refused("query", &error.refusal()))?;
    if report.truncated {
        return Err(refusal(
            "changes::board::over_budget",
            "a query's rows passed the output budget of one board run \
             (board::output::over_budget); select fewer or smaller columns, or page by key"
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

/// The JSON object a record's `body` column holds, or an empty one when it
/// holds none.
pub(crate) fn body(value: Option<&Value>) -> Map<String, Value> {
    match text(value).map(|body| parse_json(body.as_bytes())) {
        Some(Ok(Value::Object(object))) => object,
        _ => Map::new(),
    }
}

/// Whether a record whose `code` column is `code` is still read: whole, or
/// degraded only by what the tree around it holds.
pub(crate) fn readable(code: Option<&Value>) -> bool {
    text(code).is_none_or(|code| TREE_CODES.contains(&code))
}

/// The SQL list of the tree codes, for `code IN (...)`.
pub(crate) fn tree_codes() -> String {
    let codes: Vec<String> = TREE_CODES.iter().map(|code| literal(code)).collect();
    codes.join(", ")
}

/// Board's answer for each of `paths`, in order, read as `board refs` reads
/// them.
fn answers(
    host: &dyn Host,
    paths: &[String],
) -> Result<Vec<Result<RefsReport, RefsError>>, PluginError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let asked = (paths.iter())
        .map(|path| logical(path, "refs"))
        .collect::<Result<Vec<_>, _>>()?;
    (host.refs(&asked, Absent::Refuse, &[])).map_err(|error| refused("refs", &error.refusal()))
}

/// `answer`, with a path that names nothing as none.
fn found(answer: Result<RefsReport, RefsError>) -> Result<Option<RefsReport>, RefsError> {
    match answer {
        Ok(report) => Ok(Some(report)),
        Err(error) if error.code() == Code::PathNotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Where each path resolves, in order, or none when nothing is there.
pub(crate) type Resolved = Result<Vec<Option<RefsReport>>, PluginError>;

/// Where each of `paths` resolves, in order, or none when nothing is there.
/// Any other refusal of a path is `changes::board::refused`, because what
/// one change depends on must not read as absent.
pub(crate) fn refs(host: &dyn Host, paths: &[String]) -> Resolved {
    (answers(host, paths)?.into_iter())
        .map(|answer| found(answer).map_err(|error| refused("refs", &error.refusal())))
        .collect()
}

/// [`refs`] for a walk over many changes: a path that one record stops, such
/// as a mount that cannot be read or a dangling link, is none as well and
/// warned of, so that it stops only the changes that depend on it.
pub(crate) fn refs_in_walk(host: &dyn Host, paths: &[String]) -> Resolved {
    (answers(host, paths)?.into_iter())
        .map(|answer| match found(answer) {
            Err(error) if error.stops_only_this_path() => {
                host.warn(&error.refusal());
                Ok(None)
            }
            answer => answer.map_err(|error| refused("refs", &error.refusal())),
        })
        .collect()
}

/// Where `path` resolves, or none when nothing is there.
pub(crate) fn resolve(host: &dyn Host, path: &str) -> Result<Option<RefsReport>, PluginError> {
    Ok(refs(host, &[path.to_owned()])?.into_iter().next().flatten())
}

/// The stored bytes of the file at each of `paths`, in order, or none for a
/// path Board refuses or that names a directory.
pub(crate) fn get(host: &dyn Host, paths: &[String]) -> Result<Vec<Option<Vec<u8>>>, PluginError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut asked = Vec::new();
    for path in paths {
        // A path Board cannot parse holds nothing it could read.
        asked.push(path.parse::<LogicalPath>().ok());
    }
    let parsed: Vec<LogicalPath> = asked.iter().flatten().cloned().collect();
    let answers =
        (host.get_each(&parsed)).map_err(|error| refused("get", &error.refusal("get")))?;
    let mut answers = answers.into_iter();
    Ok((asked.iter())
        .map(|path| {
            path.as_ref()?;
            match answers.next()? {
                Ok(report) if matches!(report.location.object, Object::Blob(_)) => {
                    Some(report.bytes)
                }
                _ => None,
            }
        })
        .collect())
}

/// The text of the file at `path`, or none when Board reads nothing there.
pub(crate) fn get_text(host: &dyn Host, path: &str) -> Result<Option<String>, PluginError> {
    let found = get(host, &[path.to_owned()])?.into_iter().next().flatten();
    Ok(found.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
}

/// Writes each of `lines`, one line of `board put --batch`, in one commit,
/// or nothing.
pub(crate) fn put(host: &dyn Host, lines: &[Value]) -> Result<(), PluginError> {
    (host.put(lines))
        .map(|_| ())
        .map_err(|error| refused("put --batch", &error.refusal()))
}

/// What `<capability>` of another plugin outputs for `input`, or Board's
/// refusal of the call as `changes::board::refused`.
pub(crate) fn call(host: &dyn Host, capability: &str, input: &Value) -> Result<Value, PluginError> {
    call_or_refusal(host, capability, input).map_err(|refusal| refused("call", &refusal))
}

/// What `<capability>` outputs for `input`, or the refusal itself.
pub(crate) fn call_or_refusal(
    host: &dyn Host,
    capability: &str,
    input: &Value,
) -> Result<Value, Box<Refusal>> {
    let target: Target = capability
        .parse()
        .expect("each capability called is <plugin>.<capability>");
    host.call(&target, input).map(|report| report.output)
}

/// Writes each of `lines` as [`put`] does, marking the commit as a bulk
/// one with the trailer `Board-Bulk: <reason>`.
pub(crate) fn put_bulk(host: &dyn Host, lines: &[Value], reason: &str) -> Result<(), PluginError> {
    let bulk =
        Bulk::new(reason).map_err(|error| refused("put --batch --bulk", &error.refusal()))?;
    (host.put_bulk(lines, &bulk))
        .map(|_| ())
        .map_err(|error| refused("put --batch --bulk", &error.refusal()))
}
