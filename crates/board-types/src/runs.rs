//! The records `runs/run/v1` and `runs/suite/v1`, which a program writes
//! into a board to record a run of a suite.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// A record type, named by its `$type`.
pub trait Typed {
    /// The record's `$type`, such as `runs/run/v1`.
    const TYPE: &'static str;
}

/// A record's `$type`: it writes `R::TYPE` and reads only that string.
pub struct Tag<R>(PhantomData<fn() -> R>);

/// `runs/run/v1`: one run of a suite, by whom, when, on what, and what it
/// gave. Times are RFC 3339 text, as Board stores them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The suite's logical path, such as `/suites/bench`.
    pub suite: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<Actor>,
    pub started: String,
    pub scope: Scope,
    pub results: Vec<CaseResult>,
    /// Whether the working tree under test had uncommitted changes; a run
    /// that does not say is never counted as clean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<String>,
    /// What was used, such as a commit, a model or a library, and its version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versions: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Host>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measures: Option<BTreeMap<String, MeasureValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The plugin and hook that wrote the run, such as `runs/subagent-stop`;
    /// absent when an agent or a person wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written_by: Option<String>,
}

impl Run {
    /// The record's `$type`.
    pub const TYPE: &'static str = "runs/run/v1";
}

impl Typed for Run {
    const TYPE: &'static str = Self::TYPE;
}

/// `runs/suite/v1`: what gets run, how, and what a run of it measures.
///
/// It leaves out `cases`, whose `input` and `expected` are any JSON: a
/// suite that lists its cases inline reads without them, and writing one
/// needs another type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suite {
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// What is tested: a repository as `owner/name`, or a board path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subjects: Option<Vec<String>>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// A board path to the file that lists the cases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cases_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measures: Option<Vec<Measure>>,
}

impl Suite {
    /// The record's `$type`.
    pub const TYPE: &'static str = "runs/suite/v1";
}

impl Typed for Suite {
    const TYPE: &'static str = Self::TYPE;
}

/// A quantity a run takes, in `unit`. A result or a run gives its value
/// under `name`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measure {
    pub name: String,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Who acted: the session, the program, the person and the agent's name,
/// each when known.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    /// The Claude Code session's `local_` id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// The program, such as `claude-code` or `codex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// The person, such as `ana`, or their email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    /// The name the agent was given, as free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// Whether a run ran every case. A scope a later Board adds reads as
/// `Other`, holding it as written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Full,
    Targeted,
    #[serde(untagged)]
    Other(String),
}

/// The machine a run ran on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub os: String,
    pub arch: String,
    /// Read from a whole number, such as `8` or `8.0`.
    #[serde(deserialize_with = "whole")]
    pub logical_cpus: u32,
}

/// What one case gave in a run. A case that did not pass has a `note`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaseResult {
    pub case: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measures: Option<BTreeMap<String, MeasureValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The number of a repeated trial of the case, from 1, read from a
    /// whole number.
    #[serde(
        default,
        deserialize_with = "whole_or_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub trial: Option<u32>,
    /// Board paths of files that back the result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Vec<String>>,
}

/// What a case gave. An outcome a later Board adds reads as `Other`,
/// holding it as written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Pass,
    Fail,
    Error,
    Skipped,
    #[serde(untagged)]
    Other(String),
}

/// What a case or a run measures: a whole number, a number, a label or a
/// flag.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MeasureValue {
    Integer(i64),
    Number(f64),
    Label(String),
    Flag(bool),
}

impl MeasureValue {
    /// The value as a number, when it is one. A whole number past 2^53 is
    /// the nearest `f64`, as a JSON reader takes it.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            // Parsing rounds to the nearest f64, as `as` would, without a cast.
            Self::Integer(integer) => integer.to_string().parse().ok(),
            Self::Number(number) => Some(*number),
            Self::Label(_) | Self::Flag(_) => None,
        }
    }
}

impl<R> Tag<R> {
    /// The tag of `R`.
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<R> Default for Tag<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> Clone for Tag<R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R> Copy for Tag<R> {}

impl<R> PartialEq for Tag<R> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<R> Eq for Tag<R> {}

impl<R: Typed> fmt::Debug for Tag<R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(R::TYPE)
    }
}

impl<R: Typed> Serialize for Tag<R> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(R::TYPE)
    }
}

impl<'de, R: Typed> Deserialize<'de> for Tag<R> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let found = String::deserialize(deserializer)?;
        if found == R::TYPE {
            Ok(Self::new())
        } else {
            Err(de::Error::custom(format!(
                "the $type is {found}, not {}",
                R::TYPE
            )))
        }
    }
}

/// A whole number from 0 to `u32::MAX`, written as an integer or as a
/// number with no fraction, as JSON Schema's `integer` admits.
fn whole<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    deserializer.deserialize_any(Whole)
}

fn whole_or_null<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    deserializer.deserialize_option(OptionalWhole)
}

struct Whole;

impl Visitor<'_> for Whole {
    type Value = u32;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a whole number from 0 to 4294967295")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<u32, E> {
        u32::try_from(value).map_err(|_| E::invalid_value(de::Unexpected::Unsigned(value), &self))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<u32, E> {
        u32::try_from(value).map_err(|_| E::invalid_value(de::Unexpected::Signed(value), &self))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<u32, E> {
        // A whole f64 prints with no fraction, so the text parses only when
        // the number is whole and within u32, without a cast.
        (value.to_string().parse())
            .map_err(|_| E::invalid_value(de::Unexpected::Float(value), &self))
    }
}

struct OptionalWhole;

impl<'de> Visitor<'de> for OptionalWhole {
    type Value = Option<u32>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a whole number from 0 to 4294967295, or null")
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        whole(deserializer).map(Some)
    }
}

#[cfg(test)]
mod tests;
