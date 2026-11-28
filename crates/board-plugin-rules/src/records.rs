//! The record types the plugin ships, and the checker its outputs repeat.
//! The schemas of `rules/*` are generated from these structs; the files
//! under `types/rules/` are those schemas, committed. The files under
//! `types/controls/` are the schemas of the types before the rename, which
//! the plugin reads and ships as they were until every board has migrated.

use std::borrow::Cow;

use board_plugin::shape::{
    Accepts, Date, DottedVersion, Glob, LogicalGlob, Open, PathKey, Prose, Record, Ref, Summary,
    Tag, Url,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

/// `rules/rule/v1`: a requirement on files, why it exists, and optionally
/// a checker a program runs.
///
/// It applies to the entries `applies_to` references and to every entry of
/// the board whose path matches one of `paths`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// What the files must do, in sentences a person reads.
    #[schemars(with = "Prose")]
    pub requirement: String,
    /// Why the requirement exists.
    #[schemars(with = "Prose")]
    pub purpose: String,
    /// The requirement in one line of at most 25 words, which a change's
    /// brief shows in its place.
    #[schemars(with = "Option<Summary>")]
    pub summary: Option<String>,
    /// The entries the control applies to, by reference.
    // A field left out admits no null: `type` replaces Option's `["array", "null"]`.
    #[schemars(extend("type" = "array"))]
    pub applies_to: Option<Vec<Ref>>,
    /// The paths the control applies to, as `SQLite` GLOB patterns from the
    /// board's root, such as `docs/*.md`, where `*` also matches `/`.
    #[schemars(with = "Option<Vec<Glob>>", extend("type" = "array"))]
    pub paths: Option<Vec<String>>,
    /// What a program checks of the files; left out for a requirement only
    /// a reader can judge, which `check` reports as described.
    pub checker: Option<Checker>,
    /// `standing`, the default, for a rule that stays until it is changed,
    /// or `obligation` for work a landed change left owed, which the work
    /// that discharges it deletes.
    pub kind: Option<Kind>,
    /// When to show the control: `start` at session start, `edit` before
    /// the first edit of a file it covers, or `failing` only when `check`
    /// reports it failing. Left out, an obligation shows at `start`, a
    /// control `check` measures when `failing`, and any other at `edit`.
    pub show: Option<ShowAt>,
    /// What justifies the rule, such as a source note (`rules/source/v1`)
    /// or an eval.
    pub sources: Option<Vec<Ref>>,
    /// For an obligation, what left the work owed: the pull request whose
    /// merge owes it, the review report that found it, or the decision that
    /// accepted it.
    pub owed_by: Option<Ref<OwedBy>>,
    /// The suite whose audit judges the rule, such as `/suites/review`, for
    /// a rule a reader judges.
    pub judged_by: Option<Ref<JudgedBy>>,
}

/// What may judge a rule: a suite, whose runs record its audits.
#[derive(Debug)]
pub enum JudgedBy {}

impl Accepts for JudgedBy {
    const TYPES: &'static [&'static str] = &[SUITE_TYPE];
}

/// The type of a suite, which a rule's `judged_by` names.
pub(crate) const SUITE_TYPE: &str = "runs/suite/v1";

/// What may leave an obligation owed.
#[derive(Debug)]
pub enum OwedBy {}

impl Accepts for OwedBy {
    const TYPES: &'static [&'static str] = &OWED_BY_TYPES;
}

/// The record types an obligation's `owed_by` may name.
pub(crate) const OWED_BY_TYPES: [&str; 3] =
    ["github/pull/v1", "review/report/v1", "plans/decision/v1"];

impl Record for RuleRecord {
    const TYPE: &'static str = "rules/rule/v1";
}

/// The type of a rule before the rename, whose checker is named `rule`.
pub const CONTROL_TYPE: &str = "controls/control/v1";

/// Each type before the rename, and the type that replaces it.
pub const RENAMED: [(&str, &str); 2] = [
    (CONTROL_TYPE, RuleRecord::TYPE),
    ("controls/source/v1", SourceRecord::TYPE),
];

/// The schemas of the types before the rename, in the order of
/// [`RENAMED`], as the plugin shipped them.
pub(crate) const FORMER_SCHEMAS: [&str; 2] = [
    include_str!("../types/controls/control/v1.schema.json"),
    include_str!("../types/controls/source/v1.schema.json"),
];

/// The record types a query for rules matches, as an SQL list: the type
/// and the one before the rename. A macro, so `concat!` builds queries
/// with it.
macro_rules! rule_types {
    () => {
        "('rules/rule/v1', 'controls/control/v1')"
    };
}
pub(crate) use rule_types;

/// [`rule_types`], for `format!`.
pub(crate) const RULE_TYPES: &str = rule_types!();

/// `rules/source/v1`: an outside source a rule cites through its `sources`,
/// so one note serves every rule that cites it.
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
    const TYPE: &'static str = "rules/source/v1";
}

/// `rules/group/v1`: a named set of rules whose states `report` rolls up
/// into one, such as every Rust rule. A rule may be in several groups, and
/// a group holds rules only, never another group.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/rules/groups/"))]
pub struct GroupRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The group's name for people, such as `Rust`.
    #[schemars(with = "Summary")]
    pub title: String,
    /// Why the group exists.
    #[schemars(with = "Prose")]
    pub purpose: String,
    /// The rules in the group, as GLOB patterns over rule paths from the
    /// board's root, such as `/rules/rust/*` or `/rules/*-server`, where
    /// `*` also matches `/`. A rule in a mounted repository is matched by
    /// its path from the repository's root.
    #[schemars(with = "Vec<LogicalGlob>", length(min = 1))]
    pub members: Vec<String>,
}

impl Record for GroupRecord {
    const TYPE: &'static str = "rules/group/v1";
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

impl Kind {
    /// How to close the control at `control`: for an obligation, the line
    /// `check`, `for-path` and `for-paths` give it, and nothing for a
    /// standing control, which stays until it is changed.
    pub fn close(self, control: &str) -> Option<String> {
        match self {
            Self::Standing => None,
            Self::Obligation => Some(format!(
                "Once the work this obligation names is committed, delete it with `board rm \
                 {control} -m '<message naming the work>'`; merging the work does not clear it."
            )),
        }
    }
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
    /// obligation, `failing` for a control `check` measures (one with a
    /// budget, version or zero rule), and `edit` for any other.
    pub fn effective(show: Option<Self>, kind: Kind, measured: bool) -> Self {
        match (show, kind, measured) {
            (Some(show), _, _) => show,
            (None, Kind::Obligation, _) => Self::Start,
            (None, Kind::Standing, true) => Self::Failing,
            (None, Kind::Standing, false) => Self::Edit,
        }
    }
}

// The kind of a rule, which its `check` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Check {
    Budget,
    Pattern,
    Judged,
    Version,
    Zero,
}

impl Check {
    const ALL: [Self; 5] = [
        Self::Budget,
        Self::Pattern,
        Self::Judged,
        Self::Version,
        Self::Zero,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Budget => "budget",
            Self::Pattern => "pattern",
            Self::Judged => "judged",
            Self::Version => "version",
            Self::Zero => "zero",
        }
    }
}

/// What a program checks, of the kind its `check` names. A checker whose
/// `check` names a kind this Board does not know, such as one an edited
/// copy of the schema admits, is `Other`, held as the record holds it, so
/// its rule is stated and not measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checker {
    Budget(Budget),
    Pattern(Pattern),
    Judged(Judged),
    Version(Version),
    Zero(Zero),
    Other(Map<String, Value>),
}

impl Checker {
    /// Whether this Board can apply the checker: not when its `check` or
    /// one of its values is one this Board does not know, so its rule is
    /// stated and not measured.
    pub fn applies(&self) -> bool {
        match self {
            Self::Budget(budget) => budget.known().is_some(),
            Self::Pattern(pattern) => pattern.quote.known().is_some(),
            Self::Judged(_) | Self::Version(_) | Self::Zero(_) => true,
            Self::Other(_) => false,
        }
    }
}

/// A rule of a kind this Board knows, as the record writes it: its fields
/// beside `check`.
#[derive(Deserialize, Serialize)]
#[serde(tag = "check", rename_all = "lowercase")]
enum Tagged<B, P, J, V, Z> {
    Budget(B),
    Pattern(P),
    Judged(J),
    Version(V),
    Zero(Z),
}

impl<'de> Deserialize<'de> for Checker {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let fields = Map::<String, Value>::deserialize(deserializer)?;
        // A `check` that is not a string reads as serde reads any string
        // field, so its error says invalid type, not missing field.
        let check = match fields.get("check") {
            None => return Err(D::Error::missing_field("check")),
            Some(check) => String::deserialize(check).map_err(D::Error::custom)?,
        };
        if !Check::ALL
            .iter()
            .any(|known| known.name() == check.as_str())
        {
            return Ok(Self::Other(fields));
        }
        let tagged =
            Tagged::<Budget, Pattern, Judged, Version, Zero>::deserialize(Value::Object(fields))
                .map_err(D::Error::custom)?;
        Ok(match tagged {
            Tagged::Budget(rule) => Self::Budget(rule),
            Tagged::Pattern(rule) => Self::Pattern(rule),
            Tagged::Judged(rule) => Self::Judged(rule),
            Tagged::Version(rule) => Self::Version(rule),
            Tagged::Zero(rule) => Self::Zero(rule),
        })
    }
}

impl Serialize for Checker {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let tagged: Tagged<&Budget, &Pattern, &Judged, &Version, &Zero> = match self {
            Self::Budget(rule) => Tagged::Budget(rule),
            Self::Pattern(rule) => Tagged::Pattern(rule),
            Self::Judged(rule) => Tagged::Judged(rule),
            Self::Version(rule) => Tagged::Version(rule),
            Self::Zero(rule) => Tagged::Zero(rule),
            Self::Other(fields) => return fields.serialize(serializer),
        };
        tagged.serialize(serializer)
    }
}

/// A budget rule: a limit on the words or bytes of the files the control
/// applies to, or of the text `evaluate` is given.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "BudgetChecker")]
pub struct Budget {
    /// What the budget counts: `words`, separated by any whitespace, or
    /// `bytes`.
    pub unit: Open<Measure>,
    /// The most words or bytes allowed.
    pub max: u64,
    /// `total`, the default, to sum the files' usage, or `file` to hold
    /// each file on its own within `max`.
    // Left out when the record leaves it out, so an output repeats the rule
    // as the record holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("default" = "total"))]
    pub per: Option<Open<Per>>,
}

impl Budget {
    /// What the budget counts and over what, `per` defaulting to `total`,
    /// when this Board knows both.
    pub fn known(&self) -> Option<(Measure, Per)> {
        let per = match &self.per {
            Some(per) => *per.known()?,
            None => Per::Total,
        };
        Some((*self.unit.known()?, per))
    }
}

/// A pattern rule: a file of regular expressions, one per line, that text
/// must not match.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PatternChecker")]
pub struct Pattern {
    /// The file of regular expressions.
    #[schemars(with = "Ref")]
    pub file: String,
    /// What a finding quotes: `match-only` quotes what matched and never
    /// the expression, and `pattern` names the expression too.
    pub quote: Open<Quote>,
}

/// A judged rule: a rubric that a clean-context model reads text against.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "JudgedChecker")]
pub struct Judged {
    /// The rubric the model reads.
    #[schemars(with = "Ref")]
    pub rubric: String,
}

/// A version rule: every version the first group of `capture` finds is at
/// least `at_least`, compared over the numbers the version names.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "VersionChecker")]
pub struct Version {
    /// The oldest version a file may pin, as numbers separated by dots,
    /// such as `1.99.1`.
    #[schemars(with = "DottedVersion")]
    pub at_least: String,
    /// A regular expression in the syntax of Rust's regex crate, whose
    /// first group finds each version a file pins.
    #[schemars(length(min = 1))]
    pub capture: String,
}

/// A zero rule: each of `measures` is 0 in the latest full run of a clean
/// revision of `suite`, which `runs.latest` gives.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ZeroChecker")]
pub struct Zero {
    /// The suite whose runs record the measures, by its logical path, such
    /// as `/suites/old-path-users`. It is no reference, so a suite that is
    /// gone fails this rule alone.
    #[schemars(with = "PathKey")]
    pub suite: String,
    /// The run-level measures that must each be 0, by the names the suite's
    /// runs record.
    #[schemars(
        length(min = 1, max = 8),
        inner(length(min = 1, max = 64)),
        extend("uniqueItems" = true)
    )]
    pub measures: Vec<String>,
    /// The most days since the run finished; left out, a run of any age
    /// counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 365))]
    pub within_days: Option<u16>,
}

/// A rule's schema: `check` names its kind, and each kind's `$defs` entry
/// holds that kind's fields, which it requires when `check` names the kind
/// and refuses otherwise. The kinds are applied through `allOf`, where
/// Board searches the pattern file and the rubric as references; it
/// searches none under `oneOf` or `then`. `unevaluatedProperties` refuses a
/// field no kind names.
impl JsonSchema for Checker {
    fn schema_name() -> Cow<'static, str> {
        "Checker".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let kinds = vec![
            kind::<Budget>(generator, Check::Budget),
            kind::<Pattern>(generator, Check::Pattern),
            kind::<Judged>(generator, Check::Judged),
            kind::<Version>(generator, Check::Version),
            kind::<Zero>(generator, Check::Zero),
        ];
        let mut check = generator.subschema_for::<Check>();
        check.insert(
            "description".to_owned(),
            json!("The kind of rule, which decides its other fields."),
        );
        json_schema!({
            "type": "object",
            "description": "What a program checks: a budget, a pattern file, a judged rubric, a version floor or measures a suite's latest run must report as 0, named by `check`.",
            "properties": {"check": check},
            "required": ["check"],
            "allOf": kinds,
            "unevaluatedProperties": false,
        })
    }
}

/// `T`'s fields as one kind of rule's `$defs` entry, and a reference to it.
fn kind<T: JsonSchema>(generator: &mut SchemaGenerator, check: Check) -> Value {
    let mut schema = T::json_schema(generator);
    let required = schema.remove("required").unwrap_or_else(|| json!([]));
    schema.remove("type");
    schema.remove("additionalProperties");
    let fields: Vec<Value> = (schema.get("properties").and_then(Value::as_object))
        .into_iter()
        .flat_map(|properties| properties.keys())
        .map(|field| json!({"required": [field]}))
        .collect();
    schema.insert(
        "if".to_owned(),
        json!({"properties": {"check": {"const": check.name()}}, "required": ["check"]}),
    );
    schema.insert("then".to_owned(), json!({"required": required}));
    schema.insert("else".to_owned(), json!({"not": {"anyOf": fields}}));
    let name = T::schema_name().into_owned();
    let reference = json!({"$ref": format!("#/$defs/{name}")});
    generator.definitions_mut().insert(name, schema.into());
    reference
}

/// A rule as an output repeats it, or null for a control without one.
pub(crate) fn rule_output_schema(generator: &mut SchemaGenerator) -> Schema {
    // Null widens the object's type. A `oneOf` with null would hide the
    // board-ref fields inside it, and Board checks no reference there.
    let mut rule = <Checker as JsonSchema>::json_schema(generator);
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

// What a pattern finding quotes: the match only, or the expression too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum Quote {
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
