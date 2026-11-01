//! Crossings in the personal board: finding one, writing one, and the
//! `verify` and `record` capabilities that read and complete them.

use std::collections::BTreeMap;

use board_model::parse_json;
use board_ops::{Host, PluginError, literal};
use board_plugin::shape::{Prose, Record, Sha256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::boundaries::{self, Globs, rows, valid};
use crate::content::Content;
use crate::records::CrossingRecord;
use crate::{personal, refused};

/// Where crossings live in the personal board, each named by its digest.
pub(crate) fn path(digest: &str) -> String {
    format!("/crossings/{digest}")
}

/// The valid crossing of `digest` in the personal board, and its blob.
pub(crate) fn find(
    host: &dyn Host,
    digest: &str,
) -> Result<Option<(CrossingRecord, String)>, PluginError> {
    let sql = format!(
        "SELECT body, blob FROM records WHERE type = {} AND source = 'worktree' AND {} \
         AND logical = {}",
        literal(CrossingRecord::TYPE),
        valid(),
        literal(&path(digest)),
    );
    Ok(rows(host, &sql)?.into_iter().find_map(|row| {
        let [Value::String(body), Value::String(blob)] = &row[..] else {
            return None;
        };
        let record = parse_json(body.as_bytes()).ok()?;
        Some((CrossingRecord::deserialize(&record).ok()?, blob.clone()))
    }))
}

/// Writes `crossing` at its path, replacing the crossing `prior` names only
/// if it is still the one there.
pub(crate) fn write(
    host: &dyn Host,
    crossing: &CrossingRecord,
    prior: Option<&str>,
) -> Result<(), PluginError> {
    let mut line = json!({"path": path(&crossing.digest), "record": crossing});
    if let Some(blob) = prior {
        line["expect"] = json!(blob);
    }
    host.put(&[line])
        .map(|_| ())
        .map_err(|error| refused("put --batch", &error.refusal()))
}

/// Find the crossing of a digest, or of the content that `action`, `to` and
/// `parts` give.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "VerifyInput")]
pub(crate) struct VerifyInput {
    /// The crossing's digest, given alone.
    #[schemars(with = "Option<Sha256>")]
    digest: Option<String>,
    /// The capability that sends the parts, such as github.pr-create.
    action: Option<String>,
    /// Where the parts go, such as github:octo/board.
    to: Option<String>,
    /// Each part of the content, by its name, such as pr.body.
    #[schemars(extend("minProperties" = 1))]
    parts: Option<BTreeMap<String, String>>,
}

/// Whether a passing crossing exists for the digest.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "VerifyOutput")]
pub(crate) struct VerifyOutput {
    /// The SHA-256 of the crossing's content.
    #[schemars(with = "Sha256")]
    digest: String,
    /// The crossing's path in the personal board, or null when there is none.
    crossing: Option<String>,
    /// Whether the crossing exists, every check passed, and the same
    /// boundaries match it now.
    passed: bool,
    /// What the destination returned, or null.
    #[schemars(with = "Option<Prose>")]
    result: Option<String>,
}

/// Whether a passing crossing exists for the digest given, or for the
/// content given.
pub(crate) fn verify(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: VerifyInput = PluginError::read_input("boundary", "verify", input)?;
    let digest = match (&given.digest, &given.action, &given.to, &given.parts) {
        (Some(digest), None, None, None) => digest.clone(),
        (None, Some(action), Some(to), Some(parts)) => Content { action, to, parts }.digest(),
        _ => return Err(bad_input(&given)),
    };
    let personal = personal(host)?;
    let personal = &*personal;
    let Some((record, _)) = find(personal, &digest)? else {
        return Ok(output(&VerifyOutput {
            digest,
            crossing: None,
            passed: false,
            result: None,
        }));
    };
    // The crossing passes only while the same boundaries match. This misses
    // an edit to a matched boundary's checks or to a control's rule, until
    // the crossing records the blob of each record it was checked against.
    let boundaries = boundaries::read(personal)?;
    let (action, to) = (record.action.as_str(), record.to.as_str());
    let asked = boundaries
        .iter()
        .flat_map(|boundary| boundary.asked(action, to, &[]));
    let globs = Globs::answer(personal, asked)?;
    let matching: Vec<&str> = (boundaries.iter())
        .filter(|boundary| boundary.covers(&globs, action, to, record.from.as_deref()))
        .map(|boundary| boundary.path.as_str())
        .collect();
    Ok(output(&VerifyOutput {
        crossing: Some(path(&digest)),
        passed: record.passed && matching == record.boundaries,
        result: record.result,
        digest,
    }))
}

/// `boundary::verify::bad_input`: a digest given with content, or content
/// given in part.
fn bad_input(given: &VerifyInput) -> PluginError {
    let named = [
        ("action", given.action.is_some()),
        ("digest", given.digest.is_some()),
        ("parts", given.parts.is_some()),
        ("to", given.to.is_some()),
    ];
    let names: Vec<&str> = (named.iter())
        .filter(|(_, present)| *present)
        .map(|(name, _)| *name)
        .collect();
    let hint = given.digest.as_ref().map_or_else(
        || "pass all of action, to and parts on stdin, or only --digest".to_owned(),
        |digest| format!("run `board --personal call boundary.verify --digest {digest}`"),
    );
    let listed = if names.is_empty() {
        "nothing".to_owned()
    } else {
        names.join(", ")
    };
    PluginError {
        code: "boundary::verify::bad_input".to_owned(),
        message: format!(
            "verify takes a digest, or the action, to and parts of the content, and not both; \
             it was given {listed}"
        ),
        fields: Map::from_iter([("given".to_owned(), json!(names))]),
        hint: Some(hint),
    }
}

/// Add what the destination returned to the passing crossing of `digest`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RecordInput")]
pub(crate) struct RecordInput {
    /// The crossing's digest.
    #[schemars(with = "Sha256")]
    digest: String,
    /// What the destination returned, such as the pull request's URL.
    #[schemars(length(min = 1))]
    result: String,
}

/// The crossing and the result it now holds.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RecordOutput")]
pub(crate) struct RecordOutput {
    /// The crossing's path in the personal board.
    crossing: String,
    /// What the destination returned.
    #[schemars(with = "Prose")]
    result: String,
}

/// Set the result of the passing crossing of the digest, or refuse naming
/// why there is none.
pub(crate) fn record(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let RecordInput { digest, result } = PluginError::read_input("boundary", "record", input)?;
    let personal = personal(host)?;
    let crossing = path(&digest);
    let refusal = |reason: &str, message: String, hint: &str| PluginError {
        code: format!("boundary::crossing::{reason}"),
        message,
        fields: Map::from_iter([("crossing".to_owned(), json!(crossing))]),
        hint: Some(hint.to_owned()),
    };
    let Some((mut record, blob)) = find(&*personal, &digest)? else {
        return Err(refusal(
            "not_found",
            format!("no crossing is at {crossing} in the personal board"),
            "run `board --personal call boundary.check` on the content first",
        ));
    };
    if !record.passed {
        return Err(refusal(
            "failed",
            format!("the crossing at {crossing} failed its checks, so nothing was sent to record"),
            "change the content and run `board --personal call boundary.check` again",
        ));
    }
    record.result = Some(result.clone());
    write(&*personal, &record, Some(&blob))?;
    Ok(output(&RecordOutput { crossing, result }))
}

pub(crate) fn output(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("an output holds only strings, numbers, lists and maps")
}
