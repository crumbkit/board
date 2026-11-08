//! The record types the plugin ships, and the rule its outputs repeat. The
//! schemas are generated from these structs; the files under `types/` are
//! those schemas, committed.

use std::borrow::Cow;

use board_plugin::shape::{Date, Glob, Open, Prose, Record, Ref, Summary, Tag, Url};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

/// `controls/control/v1`: a requirement on files, why it exists, and
/// optionally a rule a program checks.
///
/// It applies to the entries `applies_to` references and to every entry of
/// the board whose path matches one of `paths`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlRecord {
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
    /// a reader can judge.
    pub rule: Option<Rule>,
    /// `standing`, the default, for a rule that stays until it is changed,
    /// or `obligation` for work a landed change left owed, which the work
    /// that discharges it deletes.
    pub kind: Option<Kind>,
    /// When to show the control: `start` at session start, `edit` before
    /// the first edit of a file it covers, or `failing` only when `check`
    /// reports it failing. Left out, an obligation shows at `start`, a
    /// control with a budget when `failing`, and any other at `edit`.
    pub show: Option<ShowAt>,
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
    /// budget or a version rule), and `edit` for any other.
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
}

impl Check {
    const ALL: [Self; 4] = [Self::Budget, Self::Pattern, Self::Judged, Self::Version];

    fn name(self) -> &'static str {
        match self {
            Self::Budget => "budget",
            Self::Pattern => "pattern",
            Self::Judged => "judged",
            Self::Version => "version",
        }
    }
}

/// What a program checks, of the kind its `check` names. A rule whose
/// `check` names a kind this Board does not know, such as one an edited
/// copy of the schema admits, is `Other`, held as the record holds it, so
/// its control is stated and not measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rule {
    Budget(Budget),
    Pattern(Pattern),
    Judged(Judged),
    Version(Version),
    Other(Map<String, Value>),
}

impl Rule {
    /// Whether this Board can apply the rule: not when its `check` or one
    /// of its values is one this Board does not know, so its control is
    /// stated and not measured.
    pub fn applies(&self) -> bool {
        match self {
            Self::Budget(budget) => budget.known().is_some(),
            Self::Pattern(pattern) => pattern.quote.known().is_some(),
            Self::Judged(_) | Self::Version(_) => true,
            Self::Other(_) => false,
        }
    }
}

/// A rule of a kind this Board knows, as the record writes it: its fields
/// beside `check`.
#[derive(Deserialize, Serialize)]
#[serde(tag = "check", rename_all = "lowercase")]
enum Tagged<B, P, J, V> {
    Budget(B),
    Pattern(P),
    Judged(J),
    Version(V),
}

impl<'de> Deserialize<'de> for Rule {
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
        let tagged = Tagged::<Budget, Pattern, Judged, Version>::deserialize(Value::Object(fields))
            .map_err(D::Error::custom)?;
        Ok(match tagged {
            Tagged::Budget(rule) => Self::Budget(rule),
            Tagged::Pattern(rule) => Self::Pattern(rule),
            Tagged::Judged(rule) => Self::Judged(rule),
            Tagged::Version(rule) => Self::Version(rule),
        })
    }
}

impl Serialize for Rule {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let tagged: Tagged<&Budget, &Pattern, &Judged, &Version> = match self {
            Self::Budget(rule) => Tagged::Budget(rule),
            Self::Pattern(rule) => Tagged::Pattern(rule),
            Self::Judged(rule) => Tagged::Judged(rule),
            Self::Version(rule) => Tagged::Version(rule),
            Self::Other(fields) => return fields.serialize(serializer),
        };
        tagged.serialize(serializer)
    }
}

/// A budget rule: a limit on the words or bytes of the files the control
/// applies to, or of the text `evaluate` is given.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "BudgetRule")]
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
#[schemars(rename = "PatternRule")]
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
#[schemars(rename = "JudgedRule")]
pub struct Judged {
    /// The rubric the model reads.
    #[schemars(with = "Ref")]
    pub rubric: String,
}

/// A version rule: every version the first group of `capture` finds is at
/// least `at_least`, compared over the numbers the version names.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "VersionRule")]
pub struct Version {
    /// The oldest version a file may pin, as numbers separated by dots,
    /// such as `1.99.1`.
    #[schemars(regex(pattern = r"^[0-9]+(\.[0-9]+)*$"))]
    pub at_least: String,
    /// A regular expression in the syntax of Rust's regex crate, whose
    /// first group finds each version a file pins.
    #[schemars(length(min = 1))]
    pub capture: String,
}

/// A rule's schema: `check` names its kind, and each kind's `$defs` entry
/// holds that kind's fields, which it requires when `check` names the kind
/// and refuses otherwise. The kinds are applied through `allOf`, where
/// Board searches the pattern file and the rubric as references; it
/// searches none under `oneOf` or `then`. `unevaluatedProperties` refuses a
/// field no kind names.
impl JsonSchema for Rule {
    fn schema_name() -> Cow<'static, str> {
        "Rule".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let kinds = vec![
            kind::<Budget>(generator, Check::Budget),
            kind::<Pattern>(generator, Check::Pattern),
            kind::<Judged>(generator, Check::Judged),
            kind::<Version>(generator, Check::Version),
        ];
        let mut check = generator.subschema_for::<Check>();
        check.insert(
            "description".to_owned(),
            json!("The kind of rule, which decides its other fields."),
        );
        json_schema!({
            "type": "object",
            "description": "What a program checks: a budget, a pattern file, a judged rubric or a version floor, named by `check`.",
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
    let mut rule = <Rule as JsonSchema>::json_schema(generator);
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
