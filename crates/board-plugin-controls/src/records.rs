//! The record types the plugin ships, and the budget its outputs repeat.
//! Each schema is generated from these structs; the files under `types/`
//! are those schemas, committed.

use board_plugin::shape::{Date, Prose, Record, Ref, Tag, Url};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `controls/control/v1`: a requirement on files, why it exists, and
/// optionally a budget a program checks. It applies to the entries
/// `applies_to` references and to every entry of the board whose path
/// matches one of `paths`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// What the files must do, in a sentence or two.
    #[schemars(with = "Prose")]
    pub requirement: String,
    /// Why the requirement exists.
    #[schemars(with = "Prose")]
    pub purpose: String,
    /// The entries the control applies to.
    pub applies_to: Option<Vec<Ref>>,
    /// `SQLite` GLOB patterns, such as `docs/*.md`, matched against each
    /// entry's path from the board's root.
    pub paths: Option<Vec<String>>,
    /// The budget a program checks; a control without one is stated.
    pub rule: Option<Budget>,
    /// `standing`, the default, for a rule that stays until it is changed,
    /// or `obligation` for work a landed change left owed, which the work
    /// that discharges it deletes.
    pub kind: Option<Kind>,
    /// When to show the control: `start` at session start, `edit` before
    /// the first edit of a file it covers, or `failing` only when `check`
    /// reports it failing. Left out, an obligation shows at `start`, a
    /// control with a budget when `failing`, and any other at `edit`.
    pub show: Option<Show>,
    /// What justifies the control, such as a source note
    /// (`controls/source/v1`) or an eval.
    pub sources: Option<Vec<Ref>>,
}

impl Record for ControlRecord {
    const TYPE: &'static str = "controls/control/v1";
}

/// `controls/source/v1`: an outside source a control cites through its
/// `sources`, so one note serves every control that cites it.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// Where the source is published.
    #[schemars(with = "Url")]
    pub url: String,
    /// The source's title.
    #[schemars(with = "Prose")]
    pub title: String,
    /// The day the source was read.
    #[schemars(with = "Date")]
    pub read_on: String,
    /// Why the source matters to the controls that cite it.
    #[schemars(with = "Prose")]
    pub why: String,
}

impl Record for SourceRecord {
    const TYPE: &'static str = "controls/source/v1";
}

// Whether a control stays until changed or records owed work. No doc
// comment, which would describe the inlined enum in the committed schema.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Kind {
    #[default]
    Standing,
    Obligation,
}

// When the mod shows a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Show {
    Start,
    Edit,
    Failing,
}

impl Show {
    /// `show` as the record sets it, or, left out, `start` for an
    /// obligation, `failing` for a control with a budget, and `edit` for
    /// any other.
    pub fn effective(show: Option<Self>, kind: Kind, budget: bool) -> Self {
        match (show, kind, budget) {
            (Some(show), _, _) => show,
            (None, Kind::Obligation, _) => Self::Start,
            (None, Kind::Standing, true) => Self::Failing,
            (None, Kind::Standing, false) => Self::Edit,
        }
    }
}

/// A limit on the words or bytes of the files the control applies to.
///
/// With `per` `total`, the default, the files' usage is summed; with `file`,
/// each file on its own must be within `max`. Words are separated by any
/// whitespace.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// What the budget counts: `words` or `bytes`.
    pub budget: Measure,
    /// The most the files may use.
    pub max: u64,
    /// `total`, the default, to sum the files, or `file` to limit each one.
    // Left out when the record leaves it out, so an output repeats the rule
    // as the record holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("default" = "total"))]
    pub per: Option<Per>,
}

// What a budget counts. No doc comment, which would describe the inlined
// enum in the committed schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Measure {
    Words,
    Bytes,
}

// Whether a budget sums its files or limits each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Per {
    Total,
    File,
}
