//! Which of the owner's acts need a Touch ID signature, and which take an
//! answer in chat (docs/architecture/writes/signing.md, Optional signing).
//!
//! Signing is on when Board's allowed signers file, or the attention one,
//! has an `owner` line, and stays on once the personal board holds a signed
//! approval or order, unless the owner's signed edit sets
//! `signing.configured` false in its `.board/board.json`. With
//! signing on, an act the setting's `require` lists needs Touch ID and any
//! other act takes an answer. With signing off, every act takes an answer.

use serde::{Deserialize, Serialize};

/// An act the owner approves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Act {
    /// Signing an order, which binds agents until it expires.
    Order,
    /// Approving a plugin's pending effect.
    Effect,
    /// The owner's put under `effects/`, or removal of a board's config.
    OwnerWrite,
    /// Approving a change's plan.
    Plan,
    /// Signing off what a change stores.
    Data,
    /// Agreeing a change's mockups.
    Mockup,
}

impl Act {
    /// Every act, in the order the state line names them.
    pub const ALL: [Self; 6] = [
        Self::Order,
        Self::Effect,
        Self::OwnerWrite,
        Self::Plan,
        Self::Data,
        Self::Mockup,
    ];

    /// The acts a setting with no `require` lists: each changes how agents or
    /// plugins behave.
    pub const DEFAULT: [Self; 3] = [Self::Order, Self::Effect, Self::OwnerWrite];

    /// The act as the state line names it.
    const fn words(self) -> &'static str {
        match self {
            Self::Order => "orders",
            Self::Effect => "effects",
            Self::OwnerWrite => "owner's writes",
            Self::Plan => "plans",
            Self::Data => "data sign-offs",
            Self::Mockup => "mockups",
        }
    }
}

/// `signing` in `.board/board.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningSetting {
    /// The acts that need Touch ID while signing is on; [`Act::DEFAULT`]
    /// when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require: Option<Vec<Act>>,
    /// Whether signing is configured, so that a missing allowed signers
    /// file refuses the listed acts. Left out, the personal board decides:
    /// it is configured once it holds a signed approval or order. Only the
    /// owner's signed edit sets it false, which turns signing off again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured: Option<bool>,
}

impl SigningSetting {
    /// Whether `act` is listed.
    pub fn requires(&self, act: Act) -> bool {
        self.require
            .as_deref()
            .map_or(Act::DEFAULT.contains(&act), |listed| listed.contains(&act))
    }

    /// The listed acts, in [`Act::ALL`]'s order.
    fn listed(&self) -> Vec<Act> {
        (Act::ALL.into_iter())
            .filter(|act| self.requires(*act))
            .collect()
    }
}

/// What Board's allowed signers file holds, as far as signing's state
/// needs to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signers {
    /// No file is there.
    Missing,
    /// The file is there and no line names `owner`.
    NoOwner,
    /// A line names `owner`.
    Owner,
    /// Whether a file is there cannot be told, such as for a link to
    /// nothing or a directory Board cannot search.
    Unreadable,
}

/// What an edit of the `signing` setting changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// `require` alone, or a signed record removed or unsigned.
    Require,
    /// `configured`, which says whether a missing allowed signers file
    /// refuses the listed acts.
    Configured,
}

/// What a question names to approve, in chat, an edit that leaves `signing`
/// as `setting`: `sha256:` and the hex SHA-256 of its canonical JSON, or of
/// `null` for an edit that removes it, so an answer approves that setting
/// alone.
pub fn setting_digest(setting: Option<&serde_json::Value>) -> String {
    let null = serde_json::Value::Null;
    let canonical = crate::canonical_json(setting.unwrap_or(&null));
    format!("sha256:{}", sha256_hex(canonical.as_str().as_bytes()))
}

/// Whether signing is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigningState {
    /// No `owner` line, and no signature verified yet.
    Off,
    /// An `owner` line, and no signature verified yet.
    On,
    /// A signature verified, and the allowed signers file is there.
    Configured,
    /// A signature verified, and the allowed signers file is gone.
    FileMissing,
}

/// What an act needs before Board takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// An answer in chat, or a signature.
    Answer,
    /// The owner's Touch ID signature.
    Signature,
    /// Nothing will do: the allowed signers file Board verified against is
    /// gone, and the owner restores it.
    FileMissing,
}

/// Signing's state and setting, which together say what each act needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signing {
    pub state: SigningState,
    pub setting: SigningSetting,
}

impl Signing {
    /// The state `signers` and `setting` give, where `held` says whether
    /// the personal board holds a signed approval or order.
    pub fn new(signers: Signers, setting: SigningSetting, held: bool) -> Self {
        let configured = setting.configured.unwrap_or(held);
        let state = match (configured, signers) {
            // Board cannot tell whether a file is there, so it refuses as
            // if one it verified against were gone.
            (_, Signers::Unreadable) | (true, Signers::Missing) => SigningState::FileMissing,
            (true, _) => SigningState::Configured,
            (false, Signers::Owner) => SigningState::On,
            (false, _) => SigningState::Off,
        };
        Self { state, setting }
    }

    /// What `act` needs.
    pub fn need(&self, act: Act) -> Need {
        let listed = self.setting.requires(act);
        match self.state {
            SigningState::Off => Need::Answer,
            _ if !listed => Need::Answer,
            SigningState::FileMissing => Need::FileMissing,
            SigningState::On | SigningState::Configured => Need::Signature,
        }
    }

    /// What `edit` of the `signing` setting needs: a signature whenever
    /// signing is on, whatever `require` lists, so no unsigned edit turns
    /// signing off. With signing off, an edit of `configured` still needs a
    /// signature, so it is refused, since an unsigned `configured` false
    /// would keep signing off after the owner signs; an edit of `require`
    /// takes an answer.
    pub fn need_to_edit(&self, edit: Edit) -> Need {
        match (self.state, edit) {
            (SigningState::Off, Edit::Require) => Need::Answer,
            (SigningState::Off | SigningState::On | SigningState::Configured, _) => Need::Signature,
            (SigningState::FileMissing, _) => Need::FileMissing,
        }
    }

    /// The line that says whether signing is on, which `board orders pinned`
    /// and `board orders signers` start with. `file` is the allowed signers
    /// file as the line names it.
    pub fn line(&self, file: &str) -> String {
        let listed = self.setting.listed();
        let acts = joined(&listed);
        match self.state {
            SigningState::Off => "Signing is off: every act takes an answer in chat.".to_owned(),
            _ if listed.is_empty() => "Signing is on: no act needs Touch ID.".to_owned(),
            SigningState::On | SigningState::Configured => {
                format!("Signing is on: {acts} need Touch ID.")
            }
            SigningState::FileMissing => format!(
                "Signing is on, but {file} is missing: {acts} are refused until it is restored."
            ),
        }
    }

    /// [`Signing::line`], except that with signing off it names `file` as
    /// the file with no `owner` line.
    pub fn signers_line(&self, file: &str) -> String {
        match self.state {
            SigningState::Off => format!("Signing is off: {file} has no owner line."),
            _ => self.line(file),
        }
    }
}

/// The id of the option an answer chooses to approve an act.
pub const APPROVE_OPTION: &str = "approve";

/// What Board read of an answer that approves an act. None of it decides
/// yet: the check that an agent's answer carries words the owner typed,
/// [`Approving::heard`], refuses nothing while it runs silent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approving {
    /// The owner's surface that wrote the answer, such as
    /// `plans/pane-answer`, when one did.
    pub surface: Option<String>,
    /// The session the answer names in `by.session`, whose prompt log
    /// says whether the owner typed its words.
    pub session: Option<String>,
    /// The answer's words.
    pub words: String,
}

impl Approving {
    /// Whether the answer's words are a prompt the owner typed, as `log`,
    /// the prompt log of [`Approving::session`] read at the check, says.
    /// The answer's own `heard` field is never read: whoever wrote the
    /// answer chose it.
    pub fn heard(&self, log: Option<&str>, now: std::time::SystemTime) -> bool {
        log.is_some_and(|log| heard_in(log, &self.words, now))
    }
}

/// Whether `answer`, a `plans/answer/v1`, approves the act on `target`,
/// such as a pending effect's id or an order's path: it names `by.person`,
/// chooses the option `approve` of `question`, the record it answers, whose
/// text names `target`, and was recorded after that question was asked.
/// `Err` says why not.
pub fn approving(
    answer: &serde_json::Value,
    question: Option<&serde_json::Value>,
    target: &str,
) -> Result<Approving, String> {
    let text = |value: &serde_json::Value, key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    if text(answer, "$type").as_deref() != Some("plans/answer/v1") {
        return Err("it is not a plans/answer/v1".to_owned());
    }
    let person = answer.get("by").and_then(|by| text(by, "person"));
    if person.is_none_or(|person| person.is_empty()) {
        return Err("it names no by.person, so it is no person's answer".to_owned());
    }
    if text(answer, "choice").as_deref() != Some(APPROVE_OPTION) {
        return Err(format!("it does not choose the option {APPROVE_OPTION}"));
    }
    let question = question
        .filter(|question| text(question, "$type").as_deref() == Some("plans/question/v1"))
        .ok_or_else(|| "the plans/question/v1 it answers is not in the board".to_owned())?;
    if !text(question, "question").is_some_and(|asked| asked.contains(target)) {
        return Err(format!("the question it answers does not name {target}"));
    }
    let at =
        |value: &serde_json::Value, key: &str| text(value, key).as_deref().and_then(crate::moment);
    match (at(question, "asked"), at(answer, "answered")) {
        (Some(asked), Some(answered)) if asked < answered => {}
        _ => {
            return Err(
                "it was recorded before its question was asked, or names no time".to_owned(),
            );
        }
    }
    Ok(Approving {
        surface: text(answer, "written_by"),
        session: answer.get("by").and_then(|by| text(by, "session")),
        words: text(answer, "words").unwrap_or_default(),
    })
}

/// Whether `answer`, a `plans/answer/v1`, was answered in the last
/// [`PROMPT_DAYS`] days up to `now`. An answer that approves an edit of
/// signing must be this recent, so one kept from long ago does not approve
/// a return to the same setting. `Err` says why not.
pub fn recent(answer: &serde_json::Value, now: std::time::SystemTime) -> Result<(), String> {
    let answered = (answer.get("answered").and_then(serde_json::Value::as_str))
        .and_then(crate::moment)
        .ok_or_else(|| "it names no time it was answered".to_owned())?;
    if answered > now {
        return Err("it was answered after now".to_owned());
    }
    let age = now.duration_since(answered).unwrap_or_default();
    if age.as_secs() > PROMPT_DAYS * 24 * 60 * 60 {
        return Err(format!(
            "it was answered more than {PROMPT_DAYS} days ago; ask the owner again"
        ));
    }
    Ok(())
}

/// How long the board-plans mod keeps a prompt's digest.
pub const PROMPT_DAYS: u64 = 30;

/// The hex SHA-256 of `text` with its leading and trailing whitespace
/// trimmed, which the board-plans mod logs for each prompt the owner types
/// and for each of its lines.
pub fn prompt_digest(text: &str) -> String {
    sha256_hex(text.trim().as_bytes())
}

/// The hex SHA-256 of `bytes`.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    use std::fmt::Write as _;
    (Sha256::digest(bytes).iter()).fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// The fewest words an answer's words hold to be heard, so that a word
/// as common as "yes" typed anywhere in the session does not count.
const HEARD_WORDS: usize = 2;

/// Whether `log`, a session's prompt log of lines `<unix seconds>
/// <digest>`, holds the digest of `words` from the last [`PROMPT_DAYS`]
/// days up to `now`. Words of fewer than `HEARD_WORDS` words are never
/// heard, and a line dated after `now` or that does not read so is passed
/// over.
pub fn heard_in(log: &str, words: &str, now: std::time::SystemTime) -> bool {
    if words.split_whitespace().count() < HEARD_WORDS {
        return false;
    }
    let digest = prompt_digest(words);
    let now = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let since = now.saturating_sub(PROMPT_DAYS * 24 * 60 * 60);
    log.lines().any(|line| {
        let mut parts = line.split_whitespace();
        let at = parts.next().and_then(|at| at.parse::<u64>().ok());
        at.is_some_and(|at| (since..=now).contains(&at)) && parts.next() == Some(digest.as_str())
    })
}

/// The acts' words joined as a reader lists them: `a`, `a and b`, `a, b
/// and c`.
fn joined(acts: &[Act]) -> String {
    let words: Vec<&str> = acts.iter().map(|act| act.words()).collect();
    match words.as_slice() {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests;
