//! Effects: the pending effects and answers the owner reads and writes
//! (docs/architecture/writes/effects.md).

use std::fmt::Write as _;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::attachment::Digest;
use crate::json::canonical_json;
use crate::path::escaped;

/// The `$type` of a pending effect, a request waiting for the owner.
pub const EFFECT_TYPE: &str = "board/effect/v1";
/// The `$type` of the owner's answer to a pending effect.
pub const SEEN_TYPE: &str = "board/seen/v1";
/// The effects types, which are Board's own and need no schema.
pub const EFFECT_TYPES: [&str; 2] = [EFFECT_TYPE, SEEN_TYPE];

/// The first line of the statement an approval signs.
pub const APPROVAL_HEADER: &str = "board/approve/v1";

/// The first line of the statement the owner signs to let their own put
/// change an effect.
pub const WRITE_HEADER: &str = "board/write/v1";

/// The SSH signature namespace `statement` is signed in, which is its
/// header: an owner's write in `board/write/v1`, anything else in
/// `board/approve/v1`. A key the allowed signers file limits to approvals
/// therefore cannot change an answer. A directive has its own namespace.
pub fn signature_namespace(statement: &str) -> &'static str {
    match statement.split_once('\n') {
        Some((WRITE_HEADER, _)) => WRITE_HEADER,
        _ => APPROVAL_HEADER,
    }
}

/// The op of a put, which Board performs itself.
pub const PUT_OP: &str = "put";
/// The op of a removal, which Board performs itself.
pub const RM_OP: &str = "rm";

/// The owner's answer to a pending effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    Approve,
    Refuse,
}

impl FromStr for Answer {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "approve" => Ok(Self::Approve),
            "refuse" => Ok(Self::Refuse),
            _ => Err(format!("{text:?} is not an answer; give approve or refuse")),
        }
    }
}

/// Whether `op` is a plugin's own op, `<plugin>.<name>`.
pub fn is_op(op: &str) -> bool {
    op.split_once('.')
        .is_some_and(|(plugin, name)| !plugin.is_empty() && !name.is_empty())
}

/// Whether `text` is spelled as an effect id: 64 lowercase hex digits.
pub fn is_effect_id(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A `board/effect/v1` record: a request waiting for the owner, with the
/// record or text a put would write and the blob it would replace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    #[serde(rename = "$type")]
    record_type: String,
    /// The root of the board the effect targets.
    pub board: String,
    pub plugin: String,
    pub op: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<Digest>,
    /// The blob a put replaces, in hex, or none for a new file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Effect {
    /// A request of `op` at `path` by `plugin` in `board`, with no content.
    pub fn new(board: String, plugin: String, op: String, path: String) -> Self {
        Self {
            record_type: EFFECT_TYPE.to_owned(),
            board,
            plugin,
            op,
            path,
            digest: None,
            prior: None,
            record: None,
            text: None,
        }
    }

    pub fn from_value(value: &Value) -> Result<Self, serde_json::Error> {
        Self::deserialize(value)
    }

    /// The SHA-256 of the canonical JSON of the request, in hex: its board,
    /// plugin, op and path, and for a put the digest of what it writes and
    /// the blob it replaces. The same request has the same id, and an
    /// answer to it does not admit a put over another blob.
    pub fn id(&self) -> String {
        let request = json!({
            "board": self.board,
            "plugin": self.plugin,
            "op": self.op,
            "path": self.path,
            "digest": self.digest.as_ref().map(ToString::to_string),
            "prior": self.prior,
        });
        Digest::of(canonical_json(&request).as_bytes()).hex()
    }

    /// The statement the owner signs to approve this effect, which the
    /// signer shows them. Its first line keeps it from reading as any other
    /// signed statement, and its id binds the content and the blob a put
    /// replaces. The other lines are for the owner to read, with hidden
    /// characters escaped: the request, the content's size, then the record,
    /// pretty-printed with its keys sorted, or the text a put writes, split
    /// at each `\n`.
    ///
    /// Every stored signature is checked against this text, so its layout
    /// is frozen: a change to it needs a new first line.
    pub fn approval(&self) -> String {
        let mut statement = format!(
            "{APPROVAL_HEADER}\neffect: {}\nplugin: {}\nop: {}\npath: {}\nboard: {}\n",
            self.id(),
            escaped(&self.plugin),
            escaped(&self.op),
            escaped(&self.path),
            escaped(&self.board),
        );
        self.show_content(&mut statement);
        statement
    }

    /// The statement the owner signs to let their own put of this effect's
    /// content write: its path and board, then the content as
    /// [`Effect::approval`] shows it. Board asks for it as the put runs and
    /// keeps no signature of it, so its layout is not frozen.
    pub fn owner_write(&self) -> String {
        let mut statement = format!(
            "{WRITE_HEADER}\nop: {}\npath: {}\nboard: {}\n",
            escaped(&self.op),
            escaped(&self.path),
            escaped(&self.board),
        );
        self.show_content(&mut statement);
        statement
    }

    /// Appends the content's size, then the record, pretty-printed with its
    /// keys sorted, or the text, split at each `\n`, with hidden characters
    /// escaped.
    fn show_content(&self, statement: &mut String) {
        let sorted = |record: &Value| {
            serde_json::from_slice::<Value>(canonical_json(record).as_bytes()).ok()
        };
        let content = match (&self.record, &self.text) {
            (Some(record), _) => {
                sorted(record).and_then(|record| serde_json::to_string_pretty(&record).ok())
            }
            (None, Some(text)) => Some(text.clone()),
            (None, None) => None,
        };
        if let Some(content) = content {
            let lines: Vec<&str> = content.split('\n').collect();
            let _ = writeln!(
                statement,
                "content: {} bytes in {} lines",
                content.len(),
                lines.len()
            );
            for line in lines {
                statement.push_str(&escaped(line));
                statement.push('\n');
            }
        }
    }
}

/// A `board/seen/v1` record: the owner's answer to the pending effect at
/// `effect`. An approval carries `signature`, the owner's SSH signature
/// over [`Effect::approval`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seen {
    #[serde(rename = "$type")]
    record_type: String,
    pub effect: String,
    pub answer: Answer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl Seen {
    pub fn new(effect: String, answer: Answer, reason: Option<String>) -> Self {
        Self {
            record_type: SEEN_TYPE.to_owned(),
            effect,
            answer,
            reason,
            signature: None,
        }
    }

    pub fn from_value(value: &Value) -> Result<Self, serde_json::Error> {
        Self::deserialize(value)
    }
}

/// Whether a parsed record of one of [`EFFECT_TYPES`] reads as its type;
/// `None` for any other type.
pub fn effect_record(record_type: &str, value: &Value) -> Option<Result<(), String>> {
    let shown = |error: serde_json::Error| error.to_string();
    Some(match record_type {
        EFFECT_TYPE => Effect::from_value(value).map(|_| ()).map_err(shown),
        SEEN_TYPE => Seen::from_value(value).map(|_| ()).map_err(shown),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
