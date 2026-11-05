//! The record types the plugin ships. Each schema is generated from its
//! struct; the files under `types/` are those schemas, committed. They keep
//! the bytes the Python plugin shipped, with a title on every property of a
//! change as pydantic wrote it, so a board that enabled changes before the
//! port needs no `plugin enable`.

use board_plugin::shape::{Record, Tag};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// The record types a change's `tickets` may reference; the jira and github
/// plugins own them.
pub(crate) const TICKET_TYPES: [&str; 2] = ["jira/issue/v1", "github/issue/v1"];
/// The record types a change's `pulls` may reference.
pub(crate) const PULL_TYPES: [&str; 1] = ["github/pull/v1"];
/// The record type a change's `task` references; the tasks plugin owns it.
pub(crate) const TASK_TYPE: &str = "tasks/task/v2";
/// The shared URL pattern of `board_plugin::shape::Url`.
const URL: &str = "^https?://([^/?#@]*@)?[^/?#@:][^/?#@]*([/?#]|$)";

/// Where a change stands. `dropped` ends a change outside the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Phase {
    Plan,
    Build,
    Review,
    Land,
    Done,
    Dropped,
    Paused,
}

impl Phase {
    /// The phase after this one, in the order plan, build, review, land,
    /// done; none for a phase outside it.
    pub(crate) fn next(self) -> Option<Self> {
        match self {
            Self::Plan => Some(Self::Build),
            Self::Build => Some(Self::Review),
            Self::Review => Some(Self::Land),
            Self::Land => Some(Self::Done),
            Self::Done | Self::Dropped | Self::Paused => None,
        }
    }

    /// Whether the change has ended: done or dropped.
    pub(crate) fn is_final(self) -> bool {
        matches!(self, Self::Done | Self::Dropped)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Build => "build",
            Self::Review => "review",
            Self::Land => "land",
            Self::Done => "done",
            Self::Dropped => "dropped",
            Self::Paused => "paused",
        }
    }
}

/// Who must agree to the change before it is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Route {
    Owner,
    AskFirst,
    Unowned,
}

/// What a change is for: `build` lands on main, `prototype` answers a
/// question on its own branch and never lands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    #[default]
    Build,
    Prototype,
}

impl Kind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Prototype => "prototype",
        }
    }
}

/// Where a prototype stands: exploring until its verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum State {
    Exploring,
    Settled,
    Abandoned,
}

const CHANGE: &str = "`changes/change/v1`: one change to a repository, its phase, and every record it produced.\n\nA change lives at `/changes/<name>.json`; its plan and any files it owns\nsit in `/changes/<name>/`. Git history holds its earlier phases. `mount`\nreferences the repository's mount, such as `/repos/board`. `phase` moves\nfrom `plan` through `build`, `review` and `land` to `done`; `dropped`\nends a change early. `route` is `owner` when no one else owns the files,\n`ask-first` when someone else does, and `unowned` when the repository\nnames no owners. `plan` references the Markdown plan.\n`issue` links the spec issue, when there is one. `head` is the commit\nunder test and review. `pull_request` is the pull request's URL.\n`tickets` references the Jira or GitHub issues the change works on, and\n`pulls` its GitHub pull requests, each as a record of that type.\nA keeper moves a change in build to `paused` to keep the work in progress\nsmall, and back to `build` when the cap allows.\n`records` references every record the change produced: tasks, runs,\nevals, questions, decisions and review reports.\n`task` references the task the change does. `kind` is `build`, as when\nit is left out, for work that lands on main, or `prototype` for work that\nanswers `question` on branch `proto/<name>` and never lands. A\nprototype's `state` is `exploring` until `verdict`, one sentence, settles\nor abandons it; `verdict_artifact` is the URL of the artifact version\nthe verdict means.\n`sync` records `merge_commit_url`, the merge commit's GitHub URL, when it ends a\nchange whose pull request merged; `sync` and `drop` record\n`dropped_reason` when they drop one. `over_limit` is the reason given\nfor the latest move into build past the WIP limit. `touched` is the UTC\nday a changes capability last touched the change, which keeps it active\nunder the limit.";

/// `changes/change/v1`, as [`CHANGE`] says.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "ChangeRecord", description = CHANGE)]
pub(crate) struct ChangeRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub(crate) tag: Tag<Self>,
    #[schemars(title = "Title", schema_with = "nonempty")]
    pub(crate) title: String,
    #[schemars(title = "Mount", schema_with = "any_ref")]
    pub(crate) mount: String,
    #[schemars(title = "Phase", schema_with = "phase")]
    pub(crate) phase: Phase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Route", schema_with = "route")]
    pub(crate) route: Option<Route>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Plan", schema_with = "any_ref")]
    pub(crate) plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Issue", schema_with = "web")]
    pub(crate) issue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Head", schema_with = "commit")]
    pub(crate) head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Pull Request", schema_with = "web")]
    pub(crate) pull_request: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Tickets", schema_with = "tickets")]
    pub(crate) tickets: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Pulls", schema_with = "pulls")]
    pub(crate) pulls: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Records", schema_with = "any_refs")]
    pub(crate) records: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Task", schema_with = "task_ref")]
    pub(crate) task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Kind", schema_with = "kind")]
    pub(crate) kind: Option<Kind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Question", schema_with = "prose")]
    pub(crate) question: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "State", schema_with = "state")]
    pub(crate) state: Option<State>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Verdict", schema_with = "prose")]
    pub(crate) verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Verdict Artifact", schema_with = "url")]
    pub(crate) verdict_artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Merge Commit Url", schema_with = "url")]
    pub(crate) merge_commit_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Dropped Reason", schema_with = "prose")]
    pub(crate) dropped_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Over Limit", schema_with = "prose")]
    pub(crate) over_limit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Touched", schema_with = "date")]
    pub(crate) touched: Option<String>,
}

impl Record for ChangeRecord {
    const TYPE: &'static str = crate::CHANGE_TYPE;
}

impl Clone for ChangeRecord {
    fn clone(&self) -> Self {
        Self {
            tag: Tag::default(),
            title: self.title.clone(),
            mount: self.mount.clone(),
            phase: self.phase,
            route: self.route,
            plan: self.plan.clone(),
            issue: self.issue.clone(),
            head: self.head.clone(),
            pull_request: self.pull_request.clone(),
            tickets: self.tickets.clone(),
            pulls: self.pulls.clone(),
            records: self.records.clone(),
            task: self.task.clone(),
            kind: self.kind,
            question: self.question.clone(),
            state: self.state,
            verdict: self.verdict.clone(),
            verdict_artifact: self.verdict_artifact.clone(),
            merge_commit_url: self.merge_commit_url.clone(),
            dropped_reason: self.dropped_reason.clone(),
            over_limit: self.over_limit.clone(),
            touched: self.touched.clone(),
        }
    }
}

impl ChangeRecord {
    /// A new change in `phase`, with nothing else set.
    pub(crate) fn new(title: String, mount: String, phase: Phase) -> Self {
        Self {
            tag: Tag::default(),
            title,
            mount,
            phase,
            route: None,
            plan: None,
            issue: None,
            head: None,
            pull_request: None,
            tickets: None,
            pulls: None,
            records: None,
            task: None,
            kind: None,
            question: None,
            state: None,
            verdict: None,
            verdict_artifact: None,
            merge_commit_url: None,
            dropped_reason: None,
            over_limit: None,
            touched: None,
        }
    }

    /// The change's kind; `build` when it names none, as a change written
    /// before kinds does.
    pub(crate) fn kind(&self) -> Kind {
        self.kind.unwrap_or_default()
    }

    /// The records it references, none when it names none.
    pub(crate) fn records(&self) -> &[String] {
        self.records.as_deref().unwrap_or_default()
    }
}

/// `changes/sync/v1`, at `/changes/sync.json`: the UTC day of the last writing `changes.sync`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "SyncRecord")]
pub(crate) struct SyncRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// The UTC day the last writing sync ran.
    #[schemars(schema_with = "date")]
    pub(crate) synced: String,
}

impl Record for SyncRecord {
    const TYPE: &'static str = "changes/sync/v1";
}

const PLAN: &str = "`changes/plan/v1`: one plan for a change, and the records it uses.\n\nA plan lives at `/changes/<name>/plans/<stamp>.json`, beside its text at\n`<stamp>.md`; the newest stamp is the current plan. `text` references the\nMarkdown. `uses` references the standards documents, controls, decisions,\nevals and other changes the plan relies on. `paths` holds the files and\nSQLite GLOB patterns, by logical path, that the change will edit; a plan\nfor a build change always has them.";

/// `changes/plan/v1`, as [`PLAN`] says.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "PlanRecord", description = PLAN)]
pub(crate) struct PlanRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// The plan's Markdown text, beside this record.
    #[schemars(schema_with = "any_ref")]
    pub(crate) text: String,
    /// The records and documents the plan relies on.
    #[schemars(schema_with = "any_refs")]
    pub(crate) uses: Vec<String>,
    /// The files and SQLite GLOB patterns, by logical path, the change will edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "globs")]
    pub(crate) paths: Option<Vec<String>>,
}

impl Record for PlanRecord {
    const TYPE: &'static str = "changes/plan/v1";
}

const UNDERSTANDING: &str = "`changes/understanding/v1`: how the owner showed he understands a change's data or UX.\n\nThe owner's sign-off decision references this record in its `refs`.";

/// `changes/understanding/v1`, as [`UNDERSTANDING`] says. The plugin only
/// ships its schema; the understanding-check skill writes it.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "UnderstandingRecord", description = UNDERSTANDING)]
pub(crate) struct UnderstandingRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    tag: Tag<Self>,
    /// The change whose plan was checked.
    #[schemars(schema_with = "change_ref")]
    change: String,
    /// What was checked: the plan's Data or its UX.
    #[schemars(schema_with = "subject")]
    subject: String,
    /// What the agent played back: what is stored, where, who shares it, what users see.
    #[schemars(schema_with = "prose")]
    playback: String,
    /// Each correction the owner made to the play-back.
    #[schemars(schema_with = "proses")]
    corrections: Vec<String>,
    /// Each question asked, in order.
    #[schemars(length(min = 1))]
    questions: Vec<Asked>,
    /// Each answer that differed from the plan, and how it was settled.
    mismatches: Vec<Mismatch>,
    /// What was named and left out of scope.
    #[schemars(schema_with = "proses")]
    parked: Vec<String>,
    /// The questions asked again before review.
    rechecks: Vec<Recheck>,
}

impl Record for UnderstandingRecord {
    const TYPE: &'static str = "changes/understanding/v1";
}

/// One question of an understanding check, the owner's answer, and whether it matches the plan.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Asked")]
struct Asked {
    /// The round it was asked in; a round holds at most three questions.
    #[schemars(range(min = 1))]
    round: u32,
    /// The kind of question.
    #[schemars(schema_with = "asked_kind")]
    kind: String,
    /// The question, as asked.
    #[schemars(schema_with = "prose")]
    question: String,
    /// The owner's answer, in his words.
    #[schemars(schema_with = "prose")]
    answer: String,
    /// Whether the answer matches the plan.
    matches: bool,
}

/// An answer that differed from the plan, and which of the two changed to settle it.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Mismatch")]
struct Mismatch {
    /// The question whose answer differed.
    #[schemars(schema_with = "prose")]
    question: String,
    /// How the answer and the plan differ, said plainly.
    #[schemars(schema_with = "prose")]
    difference: String,
    /// plan when the plan changed, expectation when the owner's expectation did.
    #[schemars(schema_with = "settled")]
    settled: String,
    /// What changed.
    #[schemars(schema_with = "prose")]
    how: String,
}

/// A predict or edge question asked again, worded differently, before review.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Recheck")]
struct Recheck {
    /// The kind of question asked again.
    #[schemars(schema_with = "recheck_kind")]
    kind: String,
    /// The question, as asked again.
    #[schemars(schema_with = "prose")]
    question: String,
    /// The owner's answer.
    #[schemars(schema_with = "prose")]
    answer: String,
    /// Whether the answer matches the plan; a wrong one reopens that part.
    matches: bool,
}

fn nonempty(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "minLength": 1})
}

fn prose(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "minLength": 1, "x-board-prose": true})
}

fn proses(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": prose(generator)})
}

fn any_ref(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "board-ref"})
}

fn any_refs(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": any_ref(generator)})
}

fn task_ref(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "board-ref", "x-board-type": TASK_TYPE})
}

fn change_ref(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "board-ref", "x-board-type": ChangeRecord::TYPE})
}

fn tickets(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"type": "string", "format": "board-ref", "x-board-type": TICKET_TYPES}})
}

fn pulls(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"type": "string", "format": "board-ref", "x-board-type": PULL_TYPES}})
}

fn globs(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"type": "string", "minLength": 1, "pattern": "^/", "x-board-glob": true}})
}

fn web(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "pattern": "^https?://"})
}

fn url(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "uri", "pattern": URL})
}

fn commit(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "pattern": "^[0-9a-f]{7,40}$"})
}

fn date(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "date"})
}

fn phase(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["plan", "build", "review", "land", "done", "dropped", "paused"]})
}

fn route(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["owner", "ask-first", "unowned"]})
}

fn kind(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["build", "prototype"]})
}

fn state(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["exploring", "settled", "abandoned"]})
}

fn subject(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["data", "ux"]})
}

fn asked_kind(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["predict", "choose", "explain", "edge"]})
}

fn recheck_kind(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["predict", "edge"]})
}

fn settled(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["plan", "expectation"]})
}
