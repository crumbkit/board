//! Reading and writing the board for every capability: one record by its
//! path, every artifact with its versions, and a batch of puts.

use std::collections::BTreeMap;
use std::time::SystemTime;

use board_model::{Code, Fields, LogicalPath, Object, PathError, Refusal, parse_json};
use board_ops::{Host, Limit, PluginError, literal};
use board_plugin::shape::{Record, TREE_CODES};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::records::{ArtifactRecord, VersionRecord};

/// Where a version record sits under its artifact's path.
pub(crate) const VERSIONS: &str = "/versions/";

/// `artifacts::board::refused`, for Board's refusal of `board <command>`.
pub(crate) fn refused(command: &str, refusal: &Refusal) -> PluginError {
    PluginError::board_refused("artifacts", command, refusal)
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

/// The record at `path` and its blob, or none when nothing is there.
pub(crate) fn get(host: &dyn Host, path: &str) -> Result<Option<(Value, String)>, PluginError> {
    let answers = (host.get_each(&[logical(path)?]))
        .map_err(|error| refused("get", &error.refusal("get")))?;
    match answers.into_iter().next() {
        Some(Ok(report)) => {
            let blob = match &report.location.object {
                Object::Blob(id) => id.to_string(),
                Object::Directory => return Ok(None),
            };
            let record = report.record.map_err(|refusal| refused("get", &refusal))?;
            Ok(Some((record, blob)))
        }
        Some(Err(error)) if error.code() == Code::PathNotFound => Ok(None),
        Some(Err(error)) => Err(refused("get", &error.refusal("get"))),
        // Board answers a get of one path once; this stands in for an
        // answer it never leaves out.
        None => Err(refusal(
            "artifacts::board::refused",
            format!("board get gave no answer for {path}"),
            &[("path", path)],
        )),
    }
}

/// The artifact record at `path` and its blob. Anything else there, or
/// nothing, is `artifacts::artifact::not_found`.
pub(crate) fn artifact(
    host: &dyn Host,
    path: &str,
) -> Result<(ArtifactRecord, String), PluginError> {
    held(host, path)?.ok_or_else(|| not_found(path, "holds no record"))
}

/// The artifact record at `path` and its blob, or none when nothing is
/// there. Anything else there is `artifacts::artifact::not_found`.
pub(crate) fn held(
    host: &dyn Host,
    path: &str,
) -> Result<Option<(ArtifactRecord, String)>, PluginError> {
    let Some((record, blob)) = get(host, path)? else {
        return Ok(None);
    };
    if record["$type"] != ArtifactRecord::TYPE {
        return Err(not_found(path, "is not an artifacts/artifact/v1 record"));
    }
    let record = ArtifactRecord::deserialize(&record)
        .map_err(|_| not_found(path, "holds an artifact record this plugin cannot read"))?;
    Ok(Some((record, blob)))
}

fn not_found(path: &str, what: &str) -> PluginError {
    refusal(
        "artifacts::artifact::not_found",
        format!(
            "{path} {what}; run `board --personal call artifacts.list` for the artifacts' paths"
        ),
        &[("path", path)],
    )
}

/// Writes `lines` in one commit.
pub(crate) fn put(host: &dyn Host, lines: &[Value]) -> Result<(), PluginError> {
    (host.put(lines))
        .map(|_| ())
        .map_err(|error| refused("put --batch", &error.refusal()))
}

/// The time `now` names, in UTC to the second, such as 2026-11-01T09:00:00Z.
pub(crate) fn timestamp(now: SystemTime) -> String {
    humantime::format_rfc3339_seconds(now).to_string()
}

/// One artifact record, by its path.
#[derive(Debug)]
pub(crate) struct Held {
    pub(crate) path: String,
    pub(crate) record: ArtifactRecord,
}

/// One version record, by its path.
#[derive(Debug)]
pub(crate) struct Seen {
    pub(crate) path: String,
    pub(crate) record: VersionRecord,
}

/// Every artifact in the board with its versions, newest first, and how
/// many records were left out because Board marks them degraded or this
/// plugin cannot read them.
#[derive(Debug, Default)]
pub(crate) struct Artifacts {
    pub(crate) held: Vec<Held>,
    pub(crate) versions: BTreeMap<String, Vec<Seen>>,
    pub(crate) unreadable: usize,
}

/// Reads every artifact and version record. A version belongs to the
/// artifact whose path is its own up to `/versions/`, so an artifact in a
/// mounted board reads the same as one here.
pub(crate) fn all(host: &dyn Host) -> Result<Artifacts, PluginError> {
    let codes: Vec<String> = TREE_CODES.iter().map(|code| literal(code)).collect();
    let sql = format!(
        "SELECT logical, type, (status = 'ok' OR code IN ({})), body FROM records \
         WHERE type IN ({}, {})",
        codes.join(", "),
        literal(ArtifactRecord::TYPE),
        literal(VersionRecord::TYPE),
    );
    let rows = (host.query(&sql, Limit::ALL))
        .map(|report| report.rows)
        .map_err(|error| refused("query", &error.refusal()))?;
    let mut found = Artifacts::default();
    for row in rows {
        let [Value::String(path), Value::String(kind), readable, body] = &row[..] else {
            found.unreadable += 1;
            continue;
        };
        let body = body
            .as_str()
            .and_then(|body| parse_json(body.as_bytes()).ok());
        let (Some(body), Some(1)) = (body, readable.as_i64()) else {
            found.unreadable += 1;
            continue;
        };
        if kind == ArtifactRecord::TYPE {
            match ArtifactRecord::deserialize(&body) {
                Ok(record) => found.held.push(Held {
                    path: path.clone(),
                    record,
                }),
                Err(_) => found.unreadable += 1,
            }
            continue;
        }
        let (Ok(record), Some((owner, _))) = (
            VersionRecord::deserialize(&body),
            path.rsplit_once(VERSIONS),
        ) else {
            found.unreadable += 1;
            continue;
        };
        (found.versions.entry(owner.to_owned()).or_default()).push(Seen {
            path: path.clone(),
            record,
        });
    }
    for versions in found.versions.values_mut() {
        // Newest first, a tie going to the path that sorts last.
        versions.sort_by(|a, b| (&b.record.at, &b.path).cmp(&(&a.record.at, &a.path)));
    }
    Ok(found)
}
