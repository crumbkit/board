//! The record types the plugin ships. Each schema is generated from its
//! struct; the files under `types/` are those schemas, committed.

use board_plugin::shape::{
    Accepts, Actor, CommitName, Date, LogicalGlob, Prose, Record, Ref, Slug, Summary, Tag,
    Timestamp, Url,
};
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

/// The mount a change on the board itself names: the board's root.
pub(crate) const BOARD_ITSELF: &str = "/";

fn board_itself() -> String {
    BOARD_ITSELF.to_owned()
}

fn is_board_itself(mount: &str) -> bool {
    mount == BOARD_ITSELF
}

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

/// The phases a change is in once it has left its flow: `done` after its
/// flow's last phase, `dropped` when it ended early, and `paused` when it
/// was taken out of build to keep the work in progress small.
pub(crate) const DONE: &str = "done";
pub(crate) const DROPPED: &str = "dropped";
pub(crate) const PAUSED: &str = "paused";
/// The phase the WIP limit counts, which `pause` and `resume` leave and
/// enter.
pub(crate) const BUILD: &str = "build";

/// Whether a change in `phase` has ended: done or dropped.
pub(crate) fn is_final(phase: &str) -> bool {
    phase == DONE || phase == DROPPED
}

/// Where a `changes/change/v1` stands: `plan`, `build`, `review`, `land`
/// and `done` in order; `dropped` ends it early, and `paused` holds a change
/// taken out of build to keep the work in progress small.
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
    /// The phase called `name`, or none when no phase of v1 is.
    pub(crate) fn named(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::from(name)).ok()
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Build => "build",
            Self::Review => "review",
            Self::Land => "land",
            Self::Done => DONE,
            Self::Dropped => DROPPED,
            Self::Paused => PAUSED,
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

/// `changes/change/v2`: one change to a repository, the flow it follows
/// and where it stands in it.
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
    /// The mount of the repository the change edits, such as `/repos/board`;
    /// left out for a change that edits only the board's own records, such
    /// as one on the flow-authoring flow, which `start` takes as `--mount /`.
    #[serde(default = "board_itself", skip_serializing_if = "is_board_itself")]
    #[schemars(with = "Option<Ref<Mount>>")]
    pub(crate) mount: String,
    /// The version of the flow the change follows, at
    /// `/flows/<name>/<version>`, taken when it started or last reflowed. A
    /// prototype follows no flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref<crate::flow::Flow>>")]
    pub(crate) flow: Option<String>,
    /// Where the change stands: a phase of its flow, or `done`, `dropped`
    /// or `paused`.
    #[schemars(with = "Slug")]
    pub(crate) phase: String,
    /// The check suite the change's runs belong to, which `start` resolved
    /// from the repository's contributing docs or was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref<Suite>>")]
    pub(crate) suite: Option<String>,
    /// The open tasks, decisions and evals on the mount that `start`
    /// reported, which the plan cites or dismisses.
    #[serde(default)]
    #[schemars(with = "Vec<Ref>")]
    pub(crate) surfaced: Vec<String>,
    /// Where the active run of the change stops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stop: Option<crate::flow::Stop>,
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
    #[schemars(with = "Option<CommitName>")]
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
    /// Whether Board read the change as a `changes/change/v1`, which it
    /// writes back as one until `migrate` rewrites it.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) v1: bool,
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
            flow: self.flow.clone(),
            phase: self.phase.clone(),
            suite: self.suite.clone(),
            surfaced: self.surfaced.clone(),
            stop: self.stop.clone(),
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
            v1: self.v1,
        }
    }
}

impl ChangeRecord {
    /// A new change in `phase`, with nothing else set.
    pub(crate) fn new(title: String, mount: String, phase: &str) -> Self {
        Self {
            tag: Tag::default(),
            title,
            mount,
            flow: None,
            phase: phase.to_owned(),
            suite: None,
            surfaced: Vec::new(),
            stop: None,
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
            v1: false,
        }
    }

    /// The pull requests it references, none when it names none.
    pub(crate) fn pulls(&self) -> &[String] {
        self.pulls.as_deref().unwrap_or_default()
    }

    /// The change as a `changes/change/v1` once more, or none when its
    /// phase is no phase of v1.
    pub(crate) fn as_v1(&self) -> Option<ChangeV1> {
        Some(ChangeV1 {
            tag: Tag::default(),
            title: self.title.clone(),
            mount: self.mount.clone(),
            phase: Phase::named(&self.phase)?,
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
        })
    }
}

impl From<ChangeV1> for ChangeRecord {
    /// A v1 change as Board reads it: on the shipped development flow, with
    /// no suite, surfaced records or stop.
    fn from(old: ChangeV1) -> Self {
        Self {
            route: old.route,
            task: old.task,
            plan: old.plan,
            head: old.head,
            tickets: old.tickets,
            pulls: old.pulls,
            touched: old.touched,
            over_limit: old.over_limit,
            merge_commit_url: old.merge_commit_url,
            dropped_reason: old.dropped_reason,
            v1: true,
            ..Self::new(old.title, old.mount, old.phase.as_str())
        }
    }
}

/// A change of either version, which a record about a change names until
/// every change is migrated.
#[derive(Debug)]
pub(crate) enum AnyChange {}

impl Accepts for AnyChange {
    const TYPES: &'static [&'static str] = &crate::CHANGE_TYPES;
}

/// A check suite of the runs plugin.
#[derive(Debug)]
pub(crate) enum Suite {}

impl Accepts for Suite {
    const TYPES: &'static [&'static str] = &[SUITE_TYPE];
}

/// The record type of a check suite; the runs plugin owns it.
pub(crate) const SUITE_TYPE: &str = "runs/suite/v1";

/// `changes/change/v1`: one change to a repository and its phase.
///
/// A change lives at `/changes/<name>`, and the records it owns sit under
/// `/changes/<name>/`: its plans, a prototype's record and its
/// understanding checks. Git history holds its earlier phases.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ChangeRecord", extend("x-board-folder" = "/changes/"))]
pub(crate) struct ChangeV1 {
    /// The record's type.
    #[serde(rename = "$type")]
    tag: Tag<Self>,
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

impl Record for ChangeV1 {
    const TYPE: &'static str = crate::CHANGE_V1;
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
    /// Records the change surfaced that the plan does not build on, in
    /// groups that share one reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) considered: Option<Vec<Considered>>,
    /// Obligations on the plan's paths that the change leaves owed, each
    /// with why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) owed: Option<Vec<Owed>>,
}

/// Records a plan does not build on, and why they do not apply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Considered {
    /// The records the reason covers.
    #[schemars(with = "Vec<Ref>", length(min = 1))]
    pub(crate) records: Vec<String>,
    /// Why they do not apply.
    #[schemars(with = "Prose", length(min = 1))]
    pub(crate) why: String,
}

/// An obligation a change leaves owed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Owed {
    /// The obligation left owed.
    #[schemars(with = "Ref<Control>")]
    pub(crate) control: String,
    /// Why this change does not discharge it, and what will.
    #[schemars(with = "Prose", length(min = 1))]
    pub(crate) why: String,
}

/// A control of the controls plugin.
#[derive(Debug)]
pub(crate) enum Control {}

impl Accepts for Control {
    const TYPES: &'static [&'static str] = &[CONTROL_TYPE];
}

/// The record type of a control; the controls plugin owns it.
pub(crate) const CONTROL_TYPE: &str = "controls/control/v1";

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
    #[schemars(with = "Ref<AnyChange>")]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Subject {
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

/// A question for the owner, which the plans plugin owns.
#[derive(Debug)]
pub(crate) enum OwnerQuestion {}

impl Accepts for OwnerQuestion {
    const TYPES: &'static [&'static str] = &["plans/question/v1"];
}

/// Why a run stopped: `reached` its stop, `waiting` on the owner or a
/// reviewer, `refused` by a gate it could not pass, over its `budget`, or
/// an `error`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Reason {
    Reached,
    Waiting,
    Refused,
    Budget,
    Error,
}

/// `changes/reopen/v1`: one move of a change back to build for a fix
/// round, and why.
///
/// It lives at `/changes/<name>/reopens/<stamp>`. `changes.reopen` writes
/// it with the change, and `next` shows the newest one's reason while the
/// change is in build.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/{change}/reopens/"))]
pub(crate) struct ReopenRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// Why the change went back to build.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    pub(crate) reason: String,
    /// The phase the change left.
    #[schemars(with = "Slug")]
    pub(crate) from: String,
    /// Who reopened it.
    pub(crate) by: Actor,
}

impl Record for ReopenRecord {
    const TYPE: &'static str = "changes/reopen/v1";
}

/// `changes/stop/v1`: why a run of a change stopped, and where.
///
/// It lives at `/changes/<name>/stops/<stamp>`. The agent writes it with
/// `changes.stop` before it exits, or `changes.run` writes it when the
/// agent exits without one.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/{change}/stops/"))]
pub(crate) struct StopRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// Why the run stopped.
    pub(crate) reason: Reason,
    /// The phase the change stood in.
    #[schemars(with = "Slug")]
    pub(crate) phase: String,
    /// The ids of the checks of that phase that were missing and block.
    #[schemars(with = "Vec<Slug>")]
    pub(crate) missing: Vec<String>,
    /// The question the run left for the owner, whose `blocks` names the
    /// change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref<OwnerQuestion>>")]
    pub(crate) question: Option<String>,
    /// What happened, in a line.
    #[schemars(with = "Summary")]
    pub(crate) summary: String,
}

impl Record for StopRecord {
    const TYPE: &'static str = "changes/stop/v1";
}

/// `changes/run/v1`: one run of an agent on a change that `changes.run`
/// started, which lets the cost of each flow be measured.
///
/// It lives at `/changes/<name>/runs/<stamp>`, stamped when the run
/// started.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/{change}/runs/"))]
pub(crate) struct RunRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// The version of the flow the change followed during the run, at
    /// `/flows/<name>/<version>`.
    #[schemars(with = "Ref<crate::flow::Flow>")]
    pub(crate) flow: String,
    /// Where the run was told to stop.
    pub(crate) stop: crate::flow::Stop,
    /// The file name of the program the run started, its `argv[0]` without
    /// its path, when that name is a slug; never its arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Slug>")]
    pub(crate) program: Option<String>,
    /// When the agent started.
    #[schemars(with = "Timestamp")]
    pub(crate) started: String,
    /// When the agent exited or was stopped.
    #[schemars(with = "Timestamp")]
    pub(crate) ended: String,
    /// The agent's exit status; left out when it was stopped or ended on a
    /// signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) exit: Option<i32>,
    /// What the agent reported it cost, in US dollars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0))]
    pub(crate) cost_usd: Option<f64>,
    /// How long the agent reported it ran, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) duration_ms: Option<u64>,
    /// The budget the run's brief gave the agent, in US dollars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0))]
    pub(crate) budget_usd: Option<f64>,
    /// Whether the agent ran in a sandbox: a throwaway copy of the board
    /// and the repository, whose run and stop records were copied here.
    pub(crate) sandbox: bool,
    /// For a sandboxed run, the name of the change it started in the
    /// throwaway board, where it lived at `/changes/<change>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Slug>")]
    pub(crate) change: Option<String>,
    /// The phase the change stood in when the run ended.
    #[schemars(with = "Slug")]
    pub(crate) phase: String,
    /// Why the run stopped, as its stop record says.
    pub(crate) reason: Reason,
    /// The stop record that says why the run stopped.
    #[schemars(with = "Ref<StopRecord>")]
    pub(crate) stopped: String,
}

impl Record for RunRecord {
    const TYPE: &'static str = "changes/run/v1";
}
