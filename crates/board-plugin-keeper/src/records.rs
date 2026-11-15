//! The record type the plugin ships. Its schema is generated from these
//! structs; the file under `types/` is that schema, committed.
//!
//! The record is `keeper/map/v1`, which board top reads and the personal
//! board already holds. The paths a map names are as they were on the day of
//! the pass, and a change or task may be gone later, so they are plain
//! strings and not references that Board resolves.

use std::collections::BTreeMap;

use board_plugin::shape::{Date, Prose, Record, Ref, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `keeper/map/v1`: a consolidation pass's project tree, its findings and,
/// when the pass surveyed them, the systems the work touches. Phase, touched
/// date and pull request come from `changes.open` and the change records,
/// not from this record.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeeperMap {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The UTC day of the pass.
    #[schemars(with = "Date")]
    pub date: String,
    /// The curation record this map was built from.
    pub source: Ref,
    /// The groups that projects belong to, such as one per board or team.
    pub realms: Vec<Realm>,
    /// The project tree: every open change is in exactly one project.
    pub projects: Vec<Project>,
    /// What the pass found that needs the owner or an agent to act.
    pub findings: Vec<Finding>,
    /// Orderings between changes of different projects.
    pub links: Option<Vec<Link>>,
    /// How many questions wait on the owner, by realm id.
    pub questions_waiting: Option<BTreeMap<String, u64>>,
    /// Each path through a doubly mounted repository, with the path it
    /// duplicates.
    pub dedupe: Option<BTreeMap<String, String>>,
    /// The repositories and services the work touches, from the agent's
    /// survey of each.
    pub systems: Option<Vec<System>>,
    /// How the status board places an open question on a project.
    pub question_rules: Option<QuestionRules>,
}

impl Record for KeeperMap {
    const TYPE: &'static str = "keeper/map/v1";
}

/// A group of projects.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Realm {
    /// The realm's id, a short word such as `board`.
    pub id: String,
    /// The realm's name as a page shows it.
    #[schemars(with = "Prose")]
    pub title: String,
}

/// Where a project stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum State {
    Active,
    Queued,
    Held,
    WaitingOnOwner,
    Stale,
    Done,
}

/// A project: a goal with the changes and tasks that serve it.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// The project's id, a slug.
    pub id: String,
    /// The id of the realm the project belongs to.
    pub realm: String,
    /// What the project is for, in a few words.
    #[schemars(with = "Prose")]
    pub title: String,
    /// Where the project stands.
    pub state: State,
    /// Where the work stands and what comes next.
    #[schemars(with = "Option<Prose>")]
    pub summary: Option<String>,
    /// The task that tracks the project, by path, or null when none does.
    /// The field is always present, so that null can say there is none.
    #[schemars(required, extend("type" = ["string", "null"]))]
    pub task: Option<String>,
    /// The open changes of the project, by path.
    pub changes: Vec<String>,
    /// The open tasks of the project that no change of it does, by path.
    pub tasks: Option<Vec<String>>,
    /// Pairs of changes of this project, the first landing before the
    /// second.
    #[schemars(with = "Option<Vec<[String; 2]>>")]
    pub order: Option<Vec<(String, String)>>,
    /// The open questions of the project, by path.
    pub questions: Option<Vec<String>>,
}

/// What kind of problem a finding names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum Kind {
    Landed,
    Superseded,
    Merge,
    StalePlan,
    EmptyPlan,
    Stale,
    LooseLink,
    DuplicateMount,
    AnsweredOpen,
}

/// A problem the pass found, with the capability that fixes it and the
/// question filed for the owner.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// What kind of problem it is.
    pub kind: Kind,
    /// The changes, tasks or records the finding concerns, by path.
    pub paths: Vec<String>,
    /// What shows the problem.
    #[schemars(with = "Prose")]
    pub evidence: String,
    /// The capability that fixes it, and how.
    #[schemars(with = "Prose")]
    pub fix: String,
    /// The question filed for the owner, by path.
    pub question: Option<String>,
    /// Stable id: the kind, a dash and the first 8 hex digits of the sha256
    /// of the paths joined by newlines.
    #[schemars(regex(pattern = r"^[a-z]+(-[a-z]+)*-[0-9a-f]{8}$"))]
    pub id: String,
}

/// A change that must land before another.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// The change that lands first, by path.
    pub before: String,
    /// The change that lands after it, by path.
    pub after: String,
    /// The id of the project that holds both.
    pub project: String,
}

/// What a link between systems carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum LinkKind {
    ApiCall,
    EventStream,
    LogsAndRecords,
}

/// A repository or service, as the agent surveyed it.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct System {
    /// The system's id. A repository's id is its mount's name.
    pub id: String,
    /// The id of the realm it belongs to.
    pub realm: String,
    /// The system's name as a page shows it.
    #[schemars(with = "Prose")]
    pub title: String,
    /// The language it is written in.
    pub language: Option<String>,
    /// Whether the system is outside the repositories, such as a cloud
    /// service that only links reach.
    pub external: Option<bool>,
    /// The mount whose Cargo workspace `keeper.render` reads for crates.
    pub crates_from: Option<Ref>,
    /// The parts of the system that the survey found.
    pub modules: Option<Vec<Module>>,
    /// The links that start at this system.
    pub links: Option<Vec<SystemLink>>,
}

/// A part of a system.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Module {
    /// The module's id, such as `server`.
    pub id: String,
    /// How many lines of code it holds.
    pub lines: u64,
    /// What the module does.
    #[schemars(with = "Prose")]
    pub purpose: String,
}

/// Which end of a link a control sits at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum End {
    From,
    To,
}

/// A gate or a limit on a link.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Control {
    /// The control's name.
    #[schemars(with = "Prose")]
    pub caption: String,
    /// What it does.
    #[schemars(with = "Prose")]
    pub detail: String,
    /// The end of the link where it sits.
    pub at: End,
}

/// A link from a system, or one of its modules, to another system or
/// module.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemLink {
    /// The link's id.
    pub id: String,
    /// What the link carries, in a few words.
    #[schemars(with = "Prose")]
    pub name: String,
    /// The module of this system where the link starts.
    pub module: Option<String>,
    /// The other end: a system id, or `<system>.<module>`.
    pub to: String,
    /// What the link carries.
    pub kind: LinkKind,
    /// How busy the link is, from 1 to 10.
    #[schemars(range(min = 1, max = 10))]
    pub traffic: Option<u8>,
    /// Whether the link runs in production.
    pub production: Option<bool>,
    /// Gates and limits on the link.
    pub controls: Option<Vec<Control>>,
}

/// A rule that ties a question to a project by a task path.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskRule {
    /// Text that a question's `blocked_work` contains.
    pub contains: String,
    /// The id of the project the question belongs to.
    pub to: String,
}

/// How an open question is placed on a project, and which realm counts a
/// question that no rule places. The first rule that matches wins: a change
/// the question names, then a task, then the project name in its path.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionRules {
    /// The realm id of each mounted board's questions, by mount name.
    pub mount_realms: BTreeMap<String, String>,
    /// The realm id of the questions of each project folder of the board
    /// itself, by folder name.
    #[serde(rename = "localRealm")]
    pub local_realm: BTreeMap<String, String>,
    /// The realm id of every other question of the board itself.
    #[serde(rename = "localDefault")]
    pub local_default: String,
    /// Rules by task.
    pub tasks: Vec<TaskRule>,
    /// The project id for each project folder, as `<folder>` or
    /// `<mount>/<folder>`.
    pub names: BTreeMap<String, String>,
}
