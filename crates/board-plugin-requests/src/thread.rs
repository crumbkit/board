//! `requests.thread`: every version of a plugin's request to the owner, with
//! the owner's answers and replies, oldest first.

use std::collections::BTreeMap;

use board_model::{Answer, Effect, Seen, is_effect_id, parse_json};
use board_ops::{Host, Limit, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::ReplyRecord;
use board_plugin::shape::Record;

/// Every pending, answered and seen record in the board's own effects
/// directories. Each file's name is checked against its content in Rust.
const EFFECTS: &str = "SELECT logical, body FROM records WHERE code IS NULL AND (\
     (type = 'board/effect/v1' AND (logical LIKE '/effects/pending/%' \
     OR logical LIKE '/effects/answered/%')) \
     OR (type = 'board/seen/v1' AND logical LIKE '/effects/seen/%'))";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ThreadInput")]
pub(crate) struct Input {
    /// The id of any version of the request, pending or answered: 64
    /// lowercase hex digits.
    effect: String,
}

/// A request's versions and the owner's replies. Every version shares the
/// board, plugin, op and path named here.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ThreadOutput")]
pub(crate) struct Output {
    board: String,
    plugin: String,
    op: String,
    path: String,
    versions: Vec<Version>,
    replies: Vec<Reply>,
}

/// One version of the request. `asked` is null for an effect written before
/// Board recorded it. `pending` is true while it waits for an answer; the
/// newest pending version is `latest`, and replaces each older pending one.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ThreadVersion")]
struct Version {
    id: String,
    asked: Option<String>,
    record: Option<Value>,
    text: Option<String>,
    pending: bool,
    latest: bool,
    answer: Option<Answered>,
}

/// The owner's answer to a version.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ThreadAnswer")]
struct Answered {
    #[schemars(schema_with = "answer_schema")]
    answer: Answer,
    reason: Option<String>,
}

fn answer_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type": "string", "enum": ["approve", "refuse"]})
}

/// A reply of the owner's, by path, with the version it answers.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ThreadReply")]
struct Reply {
    path: String,
    effect: String,
    text: String,
}

pub(crate) fn thread(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { effect: id } = Input::deserialize(input).map_err(|error| PluginError {
        code: "requests::call::bad_input".to_owned(),
        message: format!(
            "thread's input {input} passed its schema but does not read: {error}; \
             the input struct and its schema disagree, so report this to Board's developers"
        ),
        fields: Map::new(),
    })?;
    if !is_effect_id(&id) {
        return Err(not_found(&id, "is not 64 lowercase hex digits"));
    }
    let (effects, answers) = effects(host)?;
    let Some((given, _)) = effects.get(&id) else {
        return Err(not_found(
            &id,
            "names no effect under /effects/pending/ or /effects/answered/",
        ));
    };
    let same = |effect: &Effect| {
        (&effect.board, &effect.plugin, &effect.op, &effect.path)
            == (&given.board, &given.plugin, &given.op, &given.path)
    };
    let mut found: Vec<(&String, &Effect, bool)> = (effects.iter())
        .filter(|(_, (effect, _))| same(effect))
        .map(|(id, (effect, pending))| (id, effect, *pending && !answers.contains_key(id)))
        .collect();
    found.sort_by(|(a, first, _), (b, second, _)| {
        let key = |id, effect: &Effect| (effect.asked.is_some(), effect.asked.clone(), id);
        key(*a, first).cmp(&key(*b, second))
    });
    let latest = (found.iter().rev())
        .find(|(_, _, pending)| *pending)
        .map(|(id, _, _)| (*id).clone());
    let versions: Vec<Version> = (found.iter())
        .map(|(id, effect, pending)| Version {
            id: (*id).clone(),
            asked: effect.asked.clone(),
            record: effect.record.clone(),
            text: effect.text.clone(),
            pending: *pending,
            latest: latest.as_ref() == Some(*id),
            answer: answers.get(*id).map(|seen| Answered {
                answer: seen.answer,
                reason: seen.reason.clone(),
            }),
        })
        .collect();
    let replies = replies(host, &versions)?;
    let output = Output {
        board: given.board.clone(),
        plugin: given.plugin.to_string(),
        op: given.op.clone(),
        path: given.path.clone(),
        versions,
        replies,
    };
    Ok(serde_json::to_value(output).expect("Output holds only JSON values"))
}

fn not_found(id: &str, why: &str) -> PluginError {
    let mut fields = Map::new();
    fields.insert("effect".to_owned(), json!(id));
    PluginError {
        code: "requests::effect::not_found".to_owned(),
        message: format!(
            "{id:?} {why}; give the id of a pending or answered effect, as \
             `board --personal ls /effects/pending` lists them"
        ),
        fields,
    }
}

fn rows(host: &dyn Host, sql: &str) -> Result<Vec<(String, Value)>, PluginError> {
    let report = (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("requests", "query", &error.refusal()))?;
    Ok((report.rows.into_iter())
        .filter_map(|row| match &row[..] {
            [Value::String(logical), Value::String(body)] => {
                let body = parse_json(body.as_bytes()).ok()?;
                Some((logical.clone(), body))
            }
            _ => None,
        })
        .collect())
}

/// Every effect by id, each with whether it is under `/effects/pending/`,
/// and every answer by the id of its effect. A file whose name is not its
/// content's id is left out, as `board answer` refuses it. A pending effect
/// wins over an answered copy of the same id, which an effect asked again
/// leaves.
type Effects = BTreeMap<String, (Effect, bool)>;

fn effects(host: &dyn Host) -> Result<(Effects, BTreeMap<String, Seen>), PluginError> {
    let (mut effects, mut answers) = (Effects::new(), BTreeMap::new());
    for (logical, body) in rows(host, EFFECTS)? {
        let Some((directory, name)) = logical.rsplit_once('/') else {
            continue;
        };
        let name = name.to_owned();
        if directory == "/effects/seen" {
            if let Ok(seen) = Seen::from_value(&body) {
                answers.insert(name, seen);
            }
            continue;
        }
        let pending = directory == "/effects/pending";
        let Ok(effect) = Effect::from_value(&body) else {
            continue;
        };
        if effect.id() == name && (pending || !effects.contains_key(&name)) {
            effects.insert(name, (effect, pending));
        }
    }
    Ok((effects, answers))
}

/// The replies to `versions`, ordered by file name, then by path. A reply
/// counts only under `/requests/replies/<its effect>/` in this board.
fn replies(host: &dyn Host, versions: &[Version]) -> Result<Vec<Reply>, PluginError> {
    // Each id was computed as 64 hex digits, so it is safe in SQL.
    let ids: Vec<String> = (versions.iter())
        .map(|version| format!("'{}'", version.id))
        .collect();
    let sql = format!(
        "SELECT logical, body FROM records WHERE code IS NULL AND type = '{}' \
         AND json_extract(body, '$.effect') IN ({}) \
         AND logical LIKE '/requests/replies/' || json_extract(body, '$.effect') || '/%'",
        ReplyRecord::TYPE,
        ids.join(", ")
    );
    let mut replies: Vec<Reply> = (rows(host, &sql)?.into_iter())
        .filter_map(|(path, body)| {
            let text = |key: &str| Some(body.get(key)?.as_str()?.to_owned());
            let (effect, text) = (text("effect")?, text("text")?);
            Some(Reply { path, effect, text })
        })
        .collect();
    let name = |reply: &Reply| reply.path.rsplit('/').next().unwrap_or_default().to_owned();
    replies.sort_by_cached_key(|reply| (name(reply), reply.path.clone()));
    Ok(replies)
}

#[cfg(test)]
mod tests;
