//! The stages of a workflow and the kinds of agent that work in them, two
//! closed lists a rule can register skills and context against.

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// A stage of a workflow, where an agent receives what is registered for
// it. No doc comment, which would describe the inlined enum in a committed
// schema.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Stage {
    Orient,
    Plan,
    Mockup,
    Build,
    Review,
    Land,
    Handoff,
}

// A kind of agent, which receives only what is registered for its kind.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum AgentKind {
    Lead,
    Ux,
    Builder,
    Reviewer,
}

impl Stage {
    /// Every stage, in workflow order.
    pub const ALL: [Self; 7] = [
        Self::Orient,
        Self::Plan,
        Self::Mockup,
        Self::Build,
        Self::Review,
        Self::Land,
        Self::Handoff,
    ];

    /// The stage's name, as a record and a flag write it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Orient => "orient",
            Self::Plan => "plan",
            Self::Mockup => "mockup",
            Self::Build => "build",
            Self::Review => "review",
            Self::Land => "land",
            Self::Handoff => "handoff",
        }
    }
}

impl AgentKind {
    /// Every kind of agent.
    pub const ALL: [Self; 4] = [Self::Lead, Self::Ux, Self::Builder, Self::Reviewer];

    /// The kind's name, as a record and a flag write it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Ux => "ux",
            Self::Builder => "builder",
            Self::Reviewer => "reviewer",
        }
    }
}

/// A name that is not one of a closed list's values, with the list it
/// was read against.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{kind} {given:?} is not one of {}", .known.join(", "))]
pub struct UnknownName {
    /// What the list names, such as `stage`.
    pub kind: &'static str,
    /// The name as given.
    pub given: String,
    /// Every name the list holds, in order.
    pub known: Vec<&'static str>,
}

/// The value of `all` whose name is `text`, or which names were known.
fn named<T: Copy>(
    kind: &'static str,
    text: &str,
    all: &[T],
    name: fn(T) -> &'static str,
) -> Result<T, UnknownName> {
    (all.iter().copied().find(|value| name(*value) == text)).ok_or_else(|| UnknownName {
        kind,
        given: text.to_owned(),
        known: all.iter().copied().map(name).collect(),
    })
}

impl FromStr for Stage {
    type Err = UnknownName;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        named("stage", text, &Self::ALL, Self::as_str)
    }
}

impl FromStr for AgentKind {
    type Err = UnknownName;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        named("agent kind", text, &Self::ALL, Self::as_str)
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn each_stage_reads_back_from_its_name_and_serializes_as_it() {
        for stage in Stage::ALL {
            assert_eq!(stage.as_str().parse::<Stage>(), Ok(stage));
            assert_eq!(serde_json::to_value(stage).unwrap(), json!(stage.as_str()));
            assert_eq!(
                serde_json::from_value::<Stage>(json!(stage.as_str())).unwrap(),
                stage
            );
        }
        let names: Vec<&str> = Stage::ALL.iter().map(|stage| stage.as_str()).collect();
        assert_eq!(
            names,
            [
                "orient", "plan", "mockup", "build", "review", "land", "handoff"
            ]
        );
    }

    #[test]
    fn each_agent_kind_reads_back_from_its_name_and_serializes_as_it() {
        for kind in AgentKind::ALL {
            assert_eq!(kind.as_str().parse::<AgentKind>(), Ok(kind));
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        }
        let names: Vec<&str> = AgentKind::ALL.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(names, ["lead", "ux", "builder", "reviewer"]);
    }

    #[test]
    fn an_unknown_name_is_refused_with_every_known_one_and_a_known_one_is_read() {
        let refused = "deploy".parse::<Stage>().unwrap_err();
        assert_eq!(
            refused.to_string(),
            "stage \"deploy\" is not one of orient, plan, mockup, build, review, land, handoff"
        );
        assert_eq!("land".parse::<Stage>(), Ok(Stage::Land));
        let refused = "designer".parse::<AgentKind>().unwrap_err();
        assert_eq!(
            refused.to_string(),
            "agent kind \"designer\" is not one of lead, ux, builder, reviewer"
        );
        assert_eq!("ux".parse::<AgentKind>(), Ok(AgentKind::Ux));
        assert!(serde_json::from_value::<Stage>(json!("deploy")).is_err());
    }
}
