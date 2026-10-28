//! Effects: who may write or act where, by preference records, and the
//! pending effects and answers the owner reads and writes
//! (docs/architecture/writes/effects.md).

use std::fmt;
use std::fmt::Write as _;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::attachment::Digest;
use crate::json::canonical_json;
use crate::path::escaped;

/// The `$type` of a rule saying how far a plugin may go.
pub const PREFERENCE_TYPE: &str = "board/preference/v1";
/// The `$type` of a pending effect, a request waiting for the owner.
pub const EFFECT_TYPE: &str = "board/effect/v1";
/// The `$type` of the owner's answer to a pending effect.
pub const SEEN_TYPE: &str = "board/seen/v1";
/// The effects types, which are Board's own and need no schema.
pub const EFFECT_TYPES: [&str; 3] = [PREFERENCE_TYPE, EFFECT_TYPE, SEEN_TYPE];

/// The first line of the statement an approval signs.
pub const APPROVAL_HEADER: &str = "board/approve/v1";

/// The first line of the statement the owner signs to let their own put
/// change a rule or an effect.
pub const WRITE_HEADER: &str = "board/write/v1";

/// The SSH signature namespace `statement` is signed in, which is its
/// header: an owner's write in `board/write/v1`, anything else in
/// `board/approve/v1`. A key the allowed signers file limits to approvals
/// therefore cannot change a rule. A directive has its own namespace.
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

/// How far a plugin may go, from widest to narrowest, so the narrowest of
/// several is the greatest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Authority {
    Act,
    Propose,
    Ask,
    Refuse,
}

impl fmt::Display for Authority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Act => "act",
            Self::Propose => "propose",
            Self::Ask => "ask",
            Self::Refuse => "refuse",
        })
    }
}

/// When the owner is told about an effect. Board reports it and does
/// nothing else with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tell {
    Silent,
    Summary,
    Now,
}

/// The owner's answer to a pending effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    Approve,
    Refuse,
}

impl Answer {
    /// The authority a rule saved from this answer gives.
    pub fn authority(self) -> Authority {
        match self {
            Self::Approve => Authority::Act,
            Self::Refuse => Authority::Refuse,
        }
    }
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

/// A logical path pattern: `/` first, `*` for any run of characters,
/// including `/`, and `?` for one character. `[`, `]`, `{`, `}` and `\`
/// are refused, so no pattern means more than it seems to. It matches
/// ASCII letters in either case, as a case-insensitive file system does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Pattern(String);

impl Pattern {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `path` is one this pattern names.
    pub fn matches(&self, path: &str) -> bool {
        let pattern: Vec<char> = self.0.to_ascii_lowercase().chars().collect();
        let text: Vec<char> = path.to_ascii_lowercase().chars().collect();
        let (mut p, mut t) = (0, 0);
        // The last `*` seen, and where in `text` it stopped consuming.
        let mut star: Option<(usize, usize)> = None;
        while t < text.len() {
            match pattern.get(p) {
                Some('*') => {
                    star = Some((p, t));
                    p += 1;
                }
                Some(&c) if c == '?' || c == text[t] => {
                    p += 1;
                    t += 1;
                }
                _ => match star {
                    Some((at, from)) => {
                        star = Some((at, from + 1));
                        p = at + 1;
                        t = from + 1;
                    }
                    None => return false,
                },
            }
        }
        pattern[p..].iter().all(|&c| c == '*')
    }

    /// Whether the pattern names one path only.
    pub fn is_exact(&self) -> bool {
        !self.0.contains(['*', '?'])
    }
}

impl FromStr for Pattern {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        if !text.starts_with('/') {
            return Err(format!("the pattern {text:?} does not start with /"));
        }
        let mut segments = text[1..].split('/');
        if text != "/" && segments.any(|segment| ["", ".", ".."].contains(&segment)) {
            return Err(format!(
                "the pattern {text:?} has an empty, . or .. segment, which no path has"
            ));
        }
        if let Some(refused) = text.chars().find(|c| "[]{}\\".contains(*c)) {
            return Err(format!(
                "the pattern {text:?} holds {refused:?}; a pattern may use only * and ?"
            ));
        }
        Ok(Self(text.to_owned()))
    }
}

impl TryFrom<String> for Pattern {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
    }
}

impl From<Pattern> for String {
    fn from(pattern: Pattern) -> Self {
        pattern.0
    }
}

/// Whether `op` is a write Board makes itself, `put` or `rm`: it acts
/// where no rule covers it, and holds its own pending effect.
pub fn is_write(op: &str) -> bool {
    op == PUT_OP || op == RM_OP
}

/// Whether `op` is `put`, `rm` or `<plugin>.<name>`.
pub fn is_op(op: &str) -> bool {
    is_write(op)
        || op
            .split_once('.')
            .is_some_and(|(plugin, name)| !plugin.is_empty() && !name.is_empty())
}

/// A `board/preference/v1` record: a rule that the op at a path, by any
/// plugin or the one named, in any board or the one whose root is named,
/// has this authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preference {
    #[serde(rename = "$type")]
    record_type: String,
    pub op: String,
    pub path: Pattern,
    pub authority: Authority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tell: Option<Tell>,
}

impl Preference {
    pub fn new(op: String, path: Pattern, authority: Authority) -> Self {
        Self {
            record_type: PREFERENCE_TYPE.to_owned(),
            op,
            path,
            authority,
            plugin: None,
            board: None,
            tell: None,
        }
    }

    /// The preference a parsed record holds. Its op must be `put` or
    /// `<plugin>.<name>`.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let preference = Self::deserialize(value).map_err(|error| error.to_string())?;
        if !is_op(&preference.op) {
            return Err(format!(
                "the op {:?} is neither put nor <plugin>.<name>",
                preference.op
            ));
        }
        Ok(preference)
    }

    /// Whether this rule covers `request`.
    pub fn covers(&self, request: &Request<'_>) -> bool {
        self.op == request.op
            && self.path.matches(request.path)
            && (self.plugin.as_deref()).is_none_or(|named| Some(named) == request.plugin)
            && (self.board.as_deref()).is_none_or(|named| named == request.board)
    }
}

/// An op at a path in the board with this root, asked by a plugin or by
/// the owner.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub op: &'a str,
    pub path: &'a str,
    pub plugin: Option<&'a str>,
    pub board: &'a str,
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
    pub authority: Authority,
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
            authority: Authority::Ask,
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
    /// characters escaped: the request, whether the approval also saves a
    /// rule that acts, the content's size, then the record, pretty-printed
    /// with its keys sorted, or the text a put writes, split at each `\n`.
    ///
    /// Every stored signature is checked against this text, so its layout
    /// is frozen: a change to it needs a new first line.
    pub fn approval(&self, saves_rule: bool) -> String {
        let mut statement = format!(
            "{APPROVAL_HEADER}\neffect: {}\nplugin: {}\nop: {}\npath: {}\nboard: {}\n",
            self.id(),
            escaped(&self.plugin),
            escaped(&self.op),
            escaped(&self.path),
            escaped(&self.board),
        );
        if saves_rule {
            statement.push_str("saves rule: act for this plugin, op and path in this board\n");
        }
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
/// over [`Effect::approval`], and `rule`, the path of the rule it saved, if
/// it saved one.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

impl Seen {
    pub fn new(effect: String, answer: Answer, reason: Option<String>) -> Self {
        Self {
            record_type: SEEN_TYPE.to_owned(),
            effect,
            answer,
            reason,
            signature: None,
            rule: None,
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
        PREFERENCE_TYPE => Preference::from_value(value).map(|_| ()),
        EFFECT_TYPE => Effect::from_value(value).map(|_| ()).map_err(shown),
        SEEN_TYPE => Seen::from_value(value).map(|_| ()).map_err(shown),
        _ => return None,
    })
}

/// Where a rule was read: the owner's personal board, or the board the
/// effect targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    Owner,
    Board,
}

/// A preference and where it was read: its layer and its logical path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub layer: Layer,
    pub at: String,
    pub preference: Preference,
}

/// The authority for one request, every rule that covers it, and which of
/// them decided, by its index in `matching`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub authority: Authority,
    pub matching: Vec<Rule>,
    pub deciding: Option<usize>,
}

impl Decision {
    pub fn tell(&self) -> Option<Tell> {
        let deciding = self.matching.get(self.deciding?)?;
        deciding.preference.tell
    }
}

/// The authority `rules` give `request`. With no rule covering it, a put
/// acts and any other op asks. Otherwise the narrowest authority wins, and
/// of equals the first rule given.
pub fn decide(rules: &[Rule], request: &Request<'_>) -> Decision {
    let matching: Vec<Rule> = rules
        .iter()
        .filter(|rule| rule.preference.covers(request))
        .cloned()
        .collect();
    let deciding = matching
        .iter()
        .enumerate()
        .fold(None, |chosen: Option<usize>, (at, rule)| match chosen {
            Some(best) if matching[best].preference.authority >= rule.preference.authority => {
                Some(best)
            }
            _ => Some(at),
        });
    let authority = match deciding {
        Some(at) => matching[at].preference.authority,
        None if is_write(request.op) => Authority::Act,
        None => Authority::Ask,
    };
    Decision {
        authority,
        matching,
        deciding,
    }
}

#[cfg(test)]
mod tests;
