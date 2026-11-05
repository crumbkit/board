//! The record types the plugin ships. Each schema is generated from its
//! struct; the files under `types/` are those schemas, committed.

use board_plugin::shape::{Accepts, Date, LogicalGlob, Prose, Record, Ref, Tag, Url};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The record types a change's `tickets` may reference; the jira and github
/// plugins own them.
pub(crate) const TICKET_TYPES: [&str; 2] = ["jira/issue/v1", "github/issue/v1"];
/// The record types a change's `pulls` may reference.
pub(crate) const PULL_TYPES: [&str; 1] = ["github/pull/v1"];
/// The record type a change's `task` references; the tasks plugin owns it.
pub(crate) const TASK_TYPE: &str = "tasks/task/v2";
/// The record type of a repository's mount.
const MOUNT_TYPE: &str = "board/mount/v1";

/// A repository's mount.
#[derive(Debug)]
pub(crate) enum Mount {}

impl Accepts for Mount {
    const TYPES: &'static [&'static str] = &[MOUNT_TYPE];
}

/// A task of the tasks plugin.
#[derive(Debug)]
pub(crate) enum Task {}

impl Accepts for Task {
    const TYPES: &'static [&'static str] = &[TASK_TYPE];
}

/// A ticket of the jira or github plugin.
#[derive(Debug)]
pub(crate) enum Ticket {}

impl Accepts for Ticket {
    const TYPES: &'static [&'static str] = &TICKET_TYPES;
}

/// A pull request of the github plugin.
#[derive(Debug)]
pub(crate) enum Pull {}

impl Accepts for Pull {
    const TYPES: &'static [&'static str] = &PULL_TYPES;
}

/// Where a change stands: `plan`, `build`, `review`, `land` and `done` in
/// order; `dropped` ends it early, and `paused` holds a change taken out of
/// build to keep the work in progress small.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
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

/// Who must agree to the change before it is built: `owner` when no one
/// else owns its files, `ask-first` when someone else does, and `unowned`
/// when the repository names no owners.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub(crate) enum Route {
    Owner,
    AskFirst,
    Unowned,
}

/// What a change is for: `build` lands on main, `prototype` answers a
/// question on its own branch and never lands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Kind {
    #[default]
    Build,
    Prototype,
}

/// Where a prototype stands: `exploring` until its verdict, then `settled`
/// when it answered its question or `abandoned`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum State {
    Exploring,
    Settled,
    Abandoned,
}

/// `changes/change/v1`: one change to a repository and its phase.
///
/// A change lives at `/changes/<name>`, and the records it owns sit under
/// `/changes/<name>/`: its plans, a prototype's record and its
/// understanding checks. Git history holds its earlier phases.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/"))]
pub(crate) struct ChangeRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// What the change does, in a few words.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    pub(crate) title: String,
    /// The mount of the repository the change edits, such as `/repos/board`.
    #[schemars(with = "Ref<Mount>")]
    pub(crate) mount: String,
    /// Where the change stands.
    pub(crate) phase: Phase,
    /// Who must agree to the change before it is built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) route: Option<Route>,
    /// The task the change does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref<Task>>")]
    pub(crate) task: Option<String>,
    /// The change's current plan, a `changes/plan/v1` under
    /// `/changes/<name>/plans/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref<PlanRecord>>")]
    pub(crate) plan: Option<String>,
    /// The commit under test and review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = "^[0-9a-f]{7,40}$"))]
    pub(crate) head: Option<String>,
    /// The Jira or GitHub issues the change works on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<Ref<Ticket>>>")]
    pub(crate) tickets: Option<Vec<String>>,
    /// The change's GitHub pull requests; `sync` reads the newest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<Ref<Pull>>>")]
    pub(crate) pulls: Option<Vec<String>>,
    /// The UTC day a changes capability last touched the change, which keeps
    /// it active under the WIP limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Date>")]
    pub(crate) touched: Option<String>,
    /// The reason given for the latest move into build past the WIP limit,
    /// which `pr` puts in the pull request body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>", extend("minLength" = 1))]
    pub(crate) over_limit: Option<String>,
    /// The merge commit's GitHub page, which `sync` records when the pull
    /// request merged and `next` shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Url>")]
    pub(crate) merge_commit_url: Option<String>,
    /// Why the change was dropped, which `sync` and `drop` record and `next`
    /// shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>", extend("minLength" = 1))]
    pub(crate) dropped_reason: Option<String>,
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
            task: self.task.clone(),
            plan: self.plan.clone(),
            head: self.head.clone(),
            tickets: self.tickets.clone(),
            pulls: self.pulls.clone(),
            touched: self.touched.clone(),
            over_limit: self.over_limit.clone(),
            merge_commit_url: self.merge_commit_url.clone(),
            dropped_reason: self.dropped_reason.clone(),
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
            task: None,
            plan: None,
            head: None,
            tickets: None,
            pulls: None,
            touched: None,
            over_limit: None,
            merge_commit_url: None,
            dropped_reason: None,
        }
    }

    /// The pull requests it references, none when it names none.
    pub(crate) fn pulls(&self) -> &[String] {
        self.pulls.as_deref().unwrap_or_default()
    }
}

/// `changes/prototype/v1`: what makes a change a prototype, the question it
/// answers and its verdict.
///
/// It lives at `/changes/<name>/prototype`, beside its change, and its path
/// names the change. A prototype works on branch `proto/<name>` and never
/// lands. The change's phase says where it stands: exploring until
/// `verdict` ends it in done when settled or in dropped when abandoned.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrototypeRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// The one question the prototype answers.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    pub(crate) question: String,
    /// What it found, in one sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>", extend("minLength" = 1))]
    pub(crate) verdict: Option<String>,
    /// The page of the artifact version the verdict means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Url>")]
    pub(crate) verdict_artifact: Option<String>,
}

impl Record for PrototypeRecord {
    const TYPE: &'static str = "changes/prototype/v1";
}

impl Clone for PrototypeRecord {
    fn clone(&self) -> Self {
        Self {
            tag: Tag::default(),
            question: self.question.clone(),
            verdict: self.verdict.clone(),
            verdict_artifact: self.verdict_artifact.clone(),
        }
    }
}

/// `changes/plan/v1`: one plan for a change, the records it uses, the paths
/// it will edit and the records attached while it was the change's plan.
///
/// A plan lives at `/changes/<name>/plans/<stamp>`, beside its text at
/// `<stamp>.md`; the change's `plan` names its current one. A plan for a build
/// change that `changes.plan` wrote always has `paths`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/{change}/plans/"))]
pub(crate) struct PlanRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// The plan's Markdown text.
    #[schemars(with = "Ref")]
    pub(crate) text: String,
    /// The standards documents, controls, decisions, evals and other changes
    /// the plan relies on.
    #[schemars(with = "Vec<Ref>")]
    pub(crate) uses: Vec<String>,
    /// The files and SQLite GLOB patterns, by logical path, the change will
    /// edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<LogicalGlob>>")]
    pub(crate) paths: Option<Vec<String>>,
    /// The records the change produced while this plan was its current one, which
    /// `changes.attach --ref` adds: tasks, runs, evals, decisions,
    /// understanding checks and review reports. The gates read those of
    /// every plan of the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<Ref>>")]
    pub(crate) records: Option<Vec<String>>,
}

impl Record for PlanRecord {
    const TYPE: &'static str = "changes/plan/v1";
}

/// `changes/understanding/v1`: the owner's answers to a check that he
/// understands a change's data or UX.
///
/// It lives at `/changes/<name>/understanding/<subject>`. The owner's
/// sign-off decision references it in its `refs`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/{change}/understanding/"))]
pub(crate) struct UnderstandingRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    tag: Tag<Self>,
    /// The change whose design was checked.
    #[schemars(with = "Ref<ChangeRecord>")]
    change: String,
    /// What was checked: the design's data or its UX.
    subject: Subject,
    /// Each question asked, in order.
    #[schemars(length(min = 1))]
    questions: Vec<Asked>,
    /// Predict or edge questions asked again, worded differently, before
    /// review; the build gate wants two that match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rechecks: Option<Vec<Asked>>,
}

impl Record for UnderstandingRecord {
    const TYPE: &'static str = "changes/understanding/v1";
}

/// What an understanding check covers.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
enum Subject {
    Data,
    Ux,
}

/// One question of an understanding check, the owner's answer and the
/// design's.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Asked")]
struct Asked {
    /// The question, as asked.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    question: String,
    /// The owner's answer, in his own words.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    answer: String,
    /// What the design says the answer is.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    design: String,
    /// The kind of question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<Question>,
    /// Whether the owner's answer matches the design.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    matches: Option<bool>,
}

/// The kinds of question an understanding check asks.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
enum Question {
    Predict,
    Choose,
    Explain,
    Edge,
}
