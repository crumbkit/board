//! `changes/flow/v1`, a flow definition: a change's phases in order, the
//! instruction `next` gives in each, and the checks that gate leaving it.
//! The gates in `gates.rs` interpret a flow's checks by their kind.
//!
//! The plugin ships one flow, `development`, compiled in from
//! `flows/development.json`. It reproduces the gates Board had before flows
//! existed. A board's own flows, flow selection and placeholders in the
//! text are not read yet.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use board_plugin::shape::{Open, PathTemplate, Pattern, Prose, Record, Slug, Tag, TypeName};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::records::{Route, Subject};

/// The development flow, as the plugin ships it.
static DEVELOPMENT: LazyLock<Flow> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../flows/development.json"))
        .expect("flows/development.json is a changes/flow/v1; a test parses it")
});

/// The flow every change follows.
pub(crate) fn development() -> &'static Flow {
    &DEVELOPMENT
}

/// `changes/flow/v1`: a process a change follows from its first phase to
/// done.
///
/// A change moves through `phases` in order and then to `done`. `stop` is
/// where an agent that runs the flow stops by default.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/flows/"))]
pub(crate) struct Flow {
    /// The record's type.
    #[serde(rename = "$type")]
    tag: Tag<Self>,
    /// The flow's name, such as `development`.
    #[schemars(with = "Slug")]
    pub(crate) name: String,
    /// What the flow is for, in a sentence.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    pub(crate) summary: String,
    /// Where an agent that runs the flow stops unless told otherwise.
    pub(crate) stop: Stop,
    /// The phases in order; `done` follows the last.
    #[schemars(length(min = 1))]
    pub(crate) phases: Vec<Phase>,
}

impl Record for Flow {
    const TYPE: &'static str = "changes/flow/v1";
}

impl Flow {
    /// The phase named `name`, or none when the flow has no such phase.
    pub(crate) fn phase(&self, name: &str) -> Option<&Phase> {
        self.phases.iter().find(|phase| phase.name == name)
    }

    /// The name of the phase after `name`: the next in the list, `done`
    /// after the last, or none when the flow has no phase `name`.
    pub(crate) fn after(&self, name: &str) -> Option<&str> {
        let at = self.phases.iter().position(|phase| phase.name == name)?;
        Some(self.phases.get(at + 1).map_or("done", |next| &next.name))
    }
}

/// Where a run stops: `through` a phase does its work and stops before
/// leaving it; `until` a phase moves into it and stops before its work.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Stop {
    Through(
        /// The phase to stop in, after its work.
        #[schemars(with = "Slug")]
        String,
    ),
    Until(
        /// The phase to stop at, before its work.
        #[schemars(with = "Slug")]
        String,
    ),
}

/// One phase of a flow.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Phase {
    /// The phase's name, which a change's `phase` holds while it is here.
    #[schemars(with = "Slug")]
    pub(crate) name: String,
    /// What to do in this phase, which `next` returns.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    pub(crate) instruction: String,
    /// The checks that must pass before the change leaves the phase, in the
    /// order `next` lists them.
    pub(crate) checks: Vec<Check>,
    /// Who does the phase's work.
    pub(crate) agent: Agent,
    /// The skills the instruction relies on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(with = "Vec<Slug>")]
    pub(crate) skills: Vec<String>,
}

/// Who does a phase's work: `lead`, the agent running the flow, or
/// `builder`, a fresh agent started from `changes.brief`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Agent {
    Lead,
    Builder,
}

/// One check of a phase: its kind and the kind's parameters, who can make
/// it pass, and when it applies.
///
/// Each parameter after `waivable` belongs to the kinds its description
/// names; a kind ignores the parameters of other kinds.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Check {
    /// The check's name, unique in the flow, which `next` reports.
    #[schemars(with = "Slug")]
    pub(crate) id: String,
    /// What the check tests.
    pub(crate) kind: Open<Kind>,
    /// Who can make the check pass; `agent` when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) actor: Option<Actor>,
    /// Whether a missing check refuses `advance`; true when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) blocks: Option<bool>,
    /// When the check applies; always when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) when: Option<When>,
    /// What to do when the check is missing, in place of its kind's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>", extend("minLength" = 1))]
    pub(crate) remedy: Option<String>,
    /// Whether a decision the owner signed may waive the check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) waivable: Option<bool>,
    /// `sections`: the plan's headings that must each have text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<Prose>>")]
    pub(crate) sections: Option<Vec<String>>,
    /// `field`: the field of the change that must be set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) field: Option<Field>,
    /// `field`: the record types the field's references must have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<TypeName>>")]
    pub(crate) types: Option<Vec<String>>,
    /// `field` and `records`: the fewest values or records that pass; one
    /// when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub(crate) min: Option<u32>,
    /// `records`: the type of the attached records the check counts.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    #[schemars(rename = "type", with = "Option<TypeName>")]
    pub(crate) record_type: Option<String>,
    /// `records`: the part of each record's `by` that must differ between
    /// the records counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) distinct: Option<Distinct>,
    /// `records`: what each record counted must meet.
    #[serde(rename = "where", default, skip_serializing_if = "Option::is_none")]
    #[schemars(rename = "where")]
    pub(crate) conditions: Option<Where>,
    /// `run`: the scope the run must have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) scope: Option<Scope>,
    /// `understood`: what the understanding check covers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) subject: Option<Subject>,
    /// `understood`: the fewest rechecks matching the plan that each
    /// attached understanding check holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub(crate) rechecks: Option<u32>,
    /// `mockups`: the part of the UX section the check reads; both when
    /// left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) part: Option<Part>,
    /// `obligations`: the plan's heading under which obligations left owed
    /// are listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>", extend("minLength" = 1))]
    pub(crate) escape_section: Option<String>,
    /// `controlled-paths`: the controls whose paths the diff must not touch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<PathTemplate>>")]
    pub(crate) controls: Option<Vec<String>>,
    /// `exists`: where a record must exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<PathTemplate>")]
    pub(crate) path: Option<String>,
}

/// What a check tests, which Board implements in Rust.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub(crate) enum Kind {
    Sections,
    Field,
    Records,
    Run,
    Paths,
    Tasks,
    Understood,
    Mockups,
    Considered,
    Obligations,
    ControlledPaths,
    FlowValid,
    Exists,
}

impl Kind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Sections => "sections",
            Self::Field => "field",
            Self::Records => "records",
            Self::Run => "run",
            Self::Paths => "paths",
            Self::Tasks => "tasks",
            Self::Understood => "understood",
            Self::Mockups => "mockups",
            Self::Considered => "considered",
            Self::Obligations => "obligations",
            Self::ControlledPaths => "controlled-paths",
            Self::FlowValid => "flow-valid",
            Self::Exists => "exists",
        }
    }
}

/// Who can make a check pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Actor {
    Agent,
    Owner,
    Reviewer,
}

/// When a check applies: every condition given holds.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct When {
    /// The change's route is one of these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) route: Option<Vec<Route>>,
    /// The text under each of these plan headings matches its pattern,
    /// without case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<BTreeMap<String, Pattern>>")]
    pub(crate) section: Option<BTreeMap<String, String>>,
    /// The text under each of these plan headings does not match its
    /// pattern, without case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<BTreeMap<String, Pattern>>")]
    pub(crate) section_not: Option<BTreeMap<String, String>>,
}

/// A field of the change that a `field` check reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Field {
    Task,
    Route,
    Head,
    Plan,
    Tickets,
    Pulls,
}

/// The part of each record's `by` actor that must differ between the
/// records a `records` check counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(inline)]
pub(crate) enum Distinct {
    #[serde(rename = "by.person")]
    Person,
    #[serde(rename = "by.program")]
    Program,
    #[serde(rename = "by.session")]
    Session,
    #[serde(rename = "by.agent")]
    Agent,
}

impl Distinct {
    pub(crate) fn part(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Program => "program",
            Self::Session => "session",
            Self::Agent => "agent",
        }
    }
}

/// What each record a `records` check counts must meet.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Where {
    /// The field whose commit must be the change's head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) at_head: Option<AtHead>,
    /// For each field, the values it may hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) values: Option<BTreeMap<String, Vec<Allowed>>>,
    /// The severities none of the record's findings may have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) no_findings: Option<Vec<Severity>>,
    /// Whether the owner wrote the record himself, so its `written_by` is
    /// empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) owner_signed: Option<bool>,
    /// What the record's `refs` must each name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) refs: Option<Vec<Target>>,
}

/// The field that names the commit a record covers: `range`, whose right
/// side is the commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum AtHead {
    Range,
}

/// A value a field may hold: a word or a flag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum Allowed {
    Flag(bool),
    Word(#[schemars(with = "Slug")] String),
}

impl Allowed {
    pub(crate) fn json(&self) -> serde_json::Value {
        match self {
            Self::Flag(flag) => serde_json::Value::Bool(*flag),
            Self::Word(word) => serde_json::Value::from(word.as_str()),
        }
    }
}

/// A finding's severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl Severity {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// What a record's `refs` must name: the change's `plan` (its plan record,
/// the plan's text or the change), the `change`, or an attached record of
/// a type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum Target {
    Named(Named),
    Type(#[schemars(with = "TypeName")] String),
}

/// A target named by a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Named {
    Plan,
    Change,
}

/// The scope a run must have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Scope {
    Full,
    Targeted,
}

impl Scope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Targeted => "targeted",
        }
    }
}

/// The part of the UX section a `mockups` check reads: `agreed`, that an
/// owner decision names each mockup, or `neighbours`, that UX lists its
/// neighbours under the mockups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Part {
    Agreed,
    Neighbours,
}

#[cfg(test)]
mod tests;
