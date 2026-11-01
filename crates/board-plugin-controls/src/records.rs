//! The record type the plugin ships, and the rule its outputs repeat. The
//! schema is generated from these structs; the file under `types/` is that
//! schema, committed. Every property has a title, as the Python plugin's
//! pydantic shapes generated it.

use std::borrow::Cow;

use board_plugin::shape::{Record, Ref, Tag};
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
    #[schemars(title = "Requirement")]
    pub requirement: String,
    #[schemars(title = "Purpose")]
    pub purpose: String,
    // A field left out admits no null: `type` replaces Option's `["array", "null"]`.
    #[schemars(title = "Applies To", extend("type" = "array"))]
    pub applies_to: Option<Vec<Ref>>,
    #[schemars(title = "Paths", extend("type" = "array"))]
    pub paths: Option<Vec<String>>,
    pub rule: Option<Rule>,
}

impl Record for ControlRecord {
    const TYPE: &'static str = "controls/control/v1";
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

/// A rule as an output repeats it: [`RuleFields`] without the `oneOf`.
pub(crate) fn rule_output_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<RuleFields>()
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
