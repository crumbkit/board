//! The record types the plugin ships. Their schemas are generated from these
//! structs; the files under `types/` are those schemas, committed.

use schemars::JsonSchema;
use serde::Serialize;

use crate::shape::{Record, Ref, Tag};

/// `plans/question/v1`: a question an agent session needs the owner to answer.
///
/// It lives at `/projects/<project>/questions/<id>`, so its path names its
/// project. `<id>` is a sortable UTC time and a short random suffix, such as
/// `20261024T1830Z-k3`. Its answer is a `plans/answer/v1` at
/// `/projects/<project>/answers/<id>`, with the same id. Never put a secret
/// value in a question.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// What the owner is asked.
    #[schemars(length(min = 1))]
    pub question: String,
    /// The asking session's `local_` id, so the answer can be sent back to it.
    #[schemars(regex(pattern = r"^local_[0-9a-f-]+$"))]
    pub session: String,
    /// When the question was asked.
    #[schemars(extend("format" = "date-time"))]
    pub asked: String,
    /// `open` until an answer is recorded, then `answered`; `withdrawn` when
    /// the question no longer matters.
    pub status: Status,
    /// The mount of the repository the question concerns, such as
    /// `/repos/board`, when there is one.
    pub mount: Option<Ref>,
    /// The answers the owner may choose from.
    #[serde(default)]
    pub options: Vec<Choice>,
    /// The `id` of the option the asker recommends.
    pub recommendation: Option<String>,
    /// Why the asker recommends it.
    pub why: Option<String>,
    /// What cannot proceed until the question is answered.
    pub blocks: Option<String>,
    /// A Markdown file in the board that holds long background.
    pub context_path: Option<Ref>,
    /// What the asker did not know, such as details nobody has written down
    /// yet.
    pub note: Option<String>,
}

impl Record for QuestionRecord {
    const TYPE: &'static str = "plans/question/v1";
}

/// Where a question stands.
#[derive(Debug, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Status {
    Open,
    Answered,
    Withdrawn,
}

/// One answer the owner may give. An answer's `choice` and the question's
/// `recommendation` name it by `id`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Option")]
pub struct Choice {
    /// The option's name, unique among the question's options.
    #[schemars(length(min = 1))]
    pub id: String,
    /// What choosing the option means, in words the owner reads.
    #[schemars(length(min = 1))]
    pub text: String,
}

/// `plans/answer/v1`: the owner's answer to a `plans/question/v1`.
///
/// It lives at `/projects/<project>/answers/<id>`, with its question's id, so
/// a question has one answer. Never put a secret value in an answer.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnswerRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The question this answers.
    pub question: Ref<QuestionRecord>,
    /// The `id` of the option the owner chose, or `none` when the owner's
    /// words choose no option.
    #[schemars(length(min = 1))]
    pub choice: String,
    /// The owner's words, verbatim.
    #[schemars(length(min = 1))]
    pub words: String,
    /// When the owner gave them.
    #[schemars(extend("format" = "date-time"))]
    pub answered: String,
    /// The session that wrote the record, which may differ from the session
    /// that asked.
    pub by: Option<String>,
    /// Where the owner gave the answer.
    pub evidence: Option<Evidence>,
    /// The plugin and hook that wrote this record, such as
    /// `plans/pane-answer`, when the owner answered through a mod rather than
    /// to an agent; absent when an agent or a person wrote it.
    #[schemars(regex(pattern = r"^[a-z0-9-]+/[a-z0-9-]+$"))]
    pub written_by: Option<String>,
}

impl Record for AnswerRecord {
    const TYPE: &'static str = "plans/answer/v1";
}

/// Where the owner gave an answer. Nothing checks it yet; it is written so
/// that a later check can.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// The Claude Code session id of the transcript the owner typed in.
    pub session: String,
    /// The uuid of the turn the owner typed.
    pub entry: String,
    /// The owner's text exactly as it appears in the entry.
    pub quote: String,
}

/// `plans/decision/v1`: a decision a session made, and why.
///
/// Only a stub a hook wrote, which names itself in `written_by`, may leave
/// `why` empty, for the agent to fill in.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend(
    "if" = {"not": {"required": ["written_by"]}},
    "then" = {"properties": {"why": {"minLength": 1}}},
))]
pub struct DecisionRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The decision.
    pub decision: String,
    /// The reason for it.
    pub why: String,
    /// The session that decided.
    pub session: String,
    /// When it was decided.
    #[schemars(extend("format" = "date-time"))]
    pub decided: String,
    /// What was not chosen.
    #[serde(default)]
    pub alternatives: Vec<String>,
    /// Entries in the board the decision concerns, such as an answer.
    #[serde(default)]
    pub refs: Vec<Ref>,
    /// Web pages the decision concerns, such as a pull request.
    #[serde(default)]
    #[schemars(inner(regex(pattern = r"^https?://")))]
    pub urls: Vec<String>,
    /// `wanted` when the owner should look at the decision.
    pub review: Option<Review>,
    /// The plugin and hook that wrote this stub without an agent, such as
    /// `runs/pr-create`; absent when an agent or a person wrote it.
    #[schemars(regex(pattern = r"^[a-z0-9-]+/[a-z0-9-]+$"))]
    pub written_by: Option<String>,
}

impl Record for DecisionRecord {
    const TYPE: &'static str = "plans/decision/v1";
}

/// Whether the owner should look at a decision.
#[derive(Debug, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Review {
    Wanted,
    None,
}
