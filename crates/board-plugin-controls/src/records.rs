//! The record types the plugin ships, and the rule its outputs repeat. The
//! schemas are generated from these structs; the files under `types/` are
//! those schemas, committed. Every property of a control has a title, as the
//! Python plugin's pydantic shapes generated it.

use std::borrow::Cow;

use board_plugin::shape::{Date, Prose, Record, Ref, Tag, Url};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// `controls/control/v1`: a requirement on files, why it exists, and
/// optionally a rule a program checks.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "ControlRecord",
    description = "`controls/control/v1`: a requirement on files, why it exists, and optionally a rule a program checks.\n\nIt applies to the entries `applies_to` references and to every entry of\nthe board whose path matches one of `paths`, SQLite GLOB patterns such as\n`docs/*.md`."
)]
pub struct ControlRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    #[schemars(title = "Requirement", with = "Prose")]
    pub requirement: String,
    #[schemars(title = "Purpose", with = "Prose")]
    pub purpose: String,
    // A field left out admits no null: `type` replaces Option's `["array", "null"]`.
    #[schemars(title = "Applies To", extend("type" = "array"))]
    pub applies_to: Option<Vec<Ref>>,
    #[schemars(title = "Paths", extend("type" = "array"))]
    pub paths: Option<Vec<String>>,
    pub rule: Option<Rule>,
    /// `standing`, the default, for a rule that stays until it is changed,
    /// or `obligation` for work a landed change left owed, which the work
    /// that discharges it deletes.
    #[schemars(title = "Kind")]
    pub kind: Option<Kind>,
    /// When to show the control: `start` at session start, `edit` before
    /// the first edit of a file it covers, or `failing` only when `check`
    /// reports it failing. Left out, an obligation shows at `start`, a
    /// control with a budget when `failing`, and any other at `edit`.
    #[schemars(title = "Show")]
    pub show: Option<ShowAt>,
    /// What justifies the control, such as a source note
    /// (`controls/source/v1`) or an eval.
    #[schemars(title = "Sources")]
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
pub enum ShowAt {
    Start,
    Edit,
    Failing,
}

impl ShowAt {
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

/// What a program checks, told apart by the fields it holds.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Rule {
    Budget(Budget),
    Pattern(Pattern),
    Judged(Judged),
}

/// A limit on the words or bytes of the files the control applies to, or
/// of the text `evaluate` is given.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub budget: Measure,
    pub max: u64,
    // Left out when the record leaves it out, so an output repeats the rule
    // as the record holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per: Option<Per>,
}

/// A file of regular expressions that text must not match, and what a
/// finding shows of them.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    pub pattern: String,
    pub show: Show,
}

/// A rubric that a clean-context model reads text against.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Judged {
    pub judged: String,
}

/// The fields of every kind of rule, as one object, so that Board checks
/// `pattern` and `judged` as references: it searches no reference under
/// `anyOf` or `oneOf`. [`rule_schema`] adds the `oneOf` that picks the kind.
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[expect(dead_code, reason = "only its schema is used")]
#[schemars(
    rename = "Rule",
    title = "Rule",
    description = "What a program checks: a budget, a pattern file or a judged rubric, told apart by their fields.\n\nA budget, `budget` and `max`, limits the words or bytes of the files or\ntext; with `per` `total`, the default, the files' usage is summed, and\nwith `file` each file on its own must be within `max`. Words are\nseparated by any whitespace. A pattern, `pattern` and `show`, names a file\nof regular expressions, one per line, that text must not match; with\n`show` `match-only`, a finding quotes what matched and never the\nexpression. A judged rule, `judged`, names a rubric that a clean-context\nmodel reads text against."
)]
pub(crate) struct RuleFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Budget")]
    budget: Option<Measure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Max")]
    max: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Per", extend("default" = "total"))]
    per: Option<Per>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Pattern")]
    pattern: Option<Ref>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Show")]
    show: Option<Show>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Judged")]
    judged: Option<Ref>,
}

/// The fields of each kind of rule, in the order `oneOf` lists them.
const KINDS: [&[&str]; 3] = [&["budget", "max", "per"], &["pattern", "show"], &["judged"]];

/// The fields each kind of rule must hold.
const REQUIRED: [&[&str]; 3] = [&["budget", "max"], &["pattern", "show"], &["judged"]];

/// A rule's schema is `RuleFields`, and a `oneOf` whose branches each
/// require one kind's fields and refuse every other kind's. No branch holds
/// a reference, so Board's check for hidden references passes.
impl JsonSchema for Rule {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "RuleOfOneKind".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        rule_schema(generator)
    }
}

fn rule_schema(generator: &mut SchemaGenerator) -> Schema {
    let kinds: Vec<Value> = (REQUIRED.iter().enumerate())
        .map(|(kind, required)| {
            let others: Vec<Value> = (KINDS.iter().enumerate())
                .filter(|(other, _)| *other != kind)
                .flat_map(|(_, fields)| fields.iter())
                .map(|field| json!({"required": [field]}))
                .collect();
            json!({"required": required, "not": {"anyOf": others}})
        })
        .collect();
    let mut schema = generator.subschema_for::<RuleFields>();
    schema.insert("oneOf".to_owned(), Value::from(kinds));
    schema
}

/// A rule as an output repeats it: [`RuleFields`] without the `oneOf`, or
/// null for a control without one.
pub(crate) fn rule_output_schema(generator: &mut SchemaGenerator) -> Schema {
    // Null widens the object's type. A `oneOf` with null would hide the
    // board-ref fields inside it, and Board checks no reference there.
    let mut rule = <RuleFields as JsonSchema>::json_schema(generator);
    rule.insert("type".to_owned(), json!(["object", "null"]));
    rule
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

// What a pattern finding shows: the match only, or the expression too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum Show {
    MatchOnly,
    Pattern,
}

// Whether a budget sums its files or limits each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Per {
    Total,
    File,
}
