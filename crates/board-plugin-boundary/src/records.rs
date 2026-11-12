//! The record types the plugin ships, and the verdicts a crossing repeats.
//! Their schemas are generated from these structs; the files under `types/`
//! are those schemas, committed.

use board_plugin::shape::{Accepts, Digest, Glob, Open, Prose, Record, Ref, Tag, or_former_names};
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

/// `boundary/boundary/v1`: the rules content must pass to leave a checkout
/// for a destination by an action.
///
/// Boundaries belong under `/boundaries/` in the personal board. Globs are
/// `SQLite` GLOB patterns: `*` matches any characters, `/` and `.` included.
/// Destinations match without case; action and part names match with case.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundaryRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The checkouts it applies to, by their paths, usually mounts; left out
    /// for any checkout.
    #[schemars(with = "Option<Vec<Ref>>")]
    pub from: Option<Vec<String>>,
    /// A glob of the destinations it applies to, such as github:octo/*.
    #[schemars(with = "Glob")]
    pub to: String,
    /// Globs of the actions it applies to, such as github.*.
    #[schemars(with = "Vec<Glob>", length(min = 1))]
    pub actions: Vec<String>,
    /// The rules to check and the parts each applies to.
    #[schemars(length(min = 1))]
    pub checks: Vec<Check>,
}

impl Record for BoundaryRecord {
    const TYPE: &'static str = "boundary/boundary/v1";
}

/// A rule, and the names of the parts it applies to. A boundary written
/// before the rename names the rule `control`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = check_or_former)]
pub struct Check {
    /// The rule to check the parts against, in the personal board's own
    /// tree.
    #[serde(alias = "control")]
    #[schemars(with = "Ref<Rule>")]
    pub rule: String,
    /// Globs of the part names it applies to, such as pr.* or *.
    #[schemars(with = "Vec<Glob>", length(min = 1))]
    pub parts: Vec<String>,
}

fn check_or_former(schema: &mut Schema) {
    or_former_names(schema, &[("rule", "control")]);
}

/// The rules plugin's record, which a check and a verdict name, of either
/// type the plugin reads.
#[derive(Debug)]
pub enum Rule {}

impl Accepts for Rule {
    const TYPES: &'static [&'static str] = &["rules/rule/v1", "controls/control/v1"];
}

/// `boundary/crossing/v1`: the verdicts on one content, at
/// `/crossings/<hex>` in the personal board, where `<hex>` is its digest
/// without `sha256:`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrossingRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The digest of the content's action, destination and parts.
    #[schemars(with = "Digest")]
    pub digest: String,
    /// The capability that sends the parts, such as github.pr-create.
    pub action: String,
    /// Where the parts go, such as github:octo/board.
    pub to: String,
    /// The checkout the parts leave, by its path in the personal board; left
    /// out when the check named none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref>")]
    pub from: Option<String>,
    /// The boundaries that matched, by their paths.
    #[schemars(with = "Vec<Ref<BoundaryRecord>>")]
    pub boundaries: Vec<String>,
    /// Each check's verdict on each part it applies to.
    #[serde(deserialize_with = "verdicts")]
    pub verdicts: Vec<Verdict>,
    /// What the destination returned, such as a URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    pub result: Option<String>,
    /// The id of the owner's approved effect that let this content through
    /// its failed checks.
    #[serde(rename = "override", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Digest>")]
    pub overridden: Option<String>,
}

impl Record for CrossingRecord {
    const TYPE: &'static str = "boundary/crossing/v1";
}

impl CrossingRecord {
    /// Whether the content may cross: no verdict failed, or the owner
    /// approved an override.
    pub fn passed(&self) -> bool {
        self.overridden.is_some() || !self.verdicts.iter().any(Verdict::failed)
    }
}

/// One rule's verdict on one part, as `rules.evaluate` gave it, with each
/// null left out. A crossing written before the rename, and the result of
/// `rules.evaluate`, name the rule's path `control` and its checker `rule`;
/// [`verdicts`] reads both.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = verdict_or_former)]
pub struct Verdict {
    /// The rule's path.
    #[schemars(with = "Ref<Rule>")]
    pub rule: String,
    /// The part's name.
    pub part: String,
    /// The kind of the rule's checker; left out for a written requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checker: Option<Open<Kind>>,
    /// passed, failed, skipped after a free rule failed, or stated.
    pub status: Open<Status>,
    /// What the rule found wrong, at most 20.
    pub findings: Vec<Finding>,
    /// What a budget counted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<u64>,
    /// A budget's limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u64>,
    /// How many findings were left out past the first 20.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<u64>,
}

impl Verdict {
    /// Whether the part failed the rule. A status this Board does not know
    /// is not known to pass, so it fails.
    pub fn failed(&self) -> bool {
        !matches!(self.status, Open::Known(Status::Passed | Status::Skipped))
    }
}

/// A verdict's schema that also admits the former names: `control` for the
/// rule's path and `rule` for its checker. `rule` holds a path or a
/// checker, so it is a plain string here, and only `control` is a reference
/// Board follows, until the former names are dropped.
fn verdict_or_former(schema: &mut Schema) {
    let Some(properties) = (schema.get_mut("properties")).and_then(Value::as_object_mut) else {
        return;
    };
    let path = properties.get("rule").cloned().unwrap_or_default();
    let checker = properties.get("checker").cloned().unwrap_or_default();
    properties.insert("control".to_owned(), path);
    properties.insert(
        "rule".to_owned(),
        json!({"description": "The rule's path; before the rename, its checker.", "type": "string"}),
    );
    if let Some(Value::Array(required)) = schema.get_mut("required") {
        required.retain(|each| each != "rule");
    }
    schema.insert(
        "oneOf".to_owned(),
        json!([
            {"required": ["rule"], "properties": {"rule": {"pattern": "^/"}}, "not": {"required": ["control"]}},
            {"required": ["control"], "properties": {"rule": checker}, "not": {"required": ["checker"]}},
        ]),
    );
}

/// Verdicts read from JSON that names each one's fields either way: a
/// verdict holding `control` has its former names, so its `rule` is the
/// checker and its `control` the rule.
pub fn verdicts<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Verdict>, D::Error> {
    (Vec::<Map<String, Value>>::deserialize(deserializer)?.into_iter())
        .map(|mut fields| {
            if let Some(rule) = fields.remove("control") {
                if let Some(checker) = fields.remove("rule") {
                    fields.insert("checker".to_owned(), checker);
                }
                fields.insert("rule".to_owned(), rule);
            }
            Verdict::deserialize(Value::Object(fields)).map_err(serde::de::Error::custom)
        })
        .collect()
}

/// The kind of checker a verdict comes from, as a rule's checker names it
/// in its `check`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Kind {
    Budget,
    Pattern,
    Judged,
    Version,
}

/// How a part fared against one rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Status {
    Passed,
    Failed,
    Skipped,
    Stated,
}

/// What a rule found wrong in a part: the text at fault, and its line
/// when it has one.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// The text at fault: a match, a budget's count, or a quoted sentence.
    #[schemars(with = "Prose")]
    pub text: String,
    /// The line of the part the text starts on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// The expression that matched, when its rule shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    pub pattern: Option<String>,
    /// How to rewrite the text, from a judged rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    pub rewrite: Option<String>,
}
