//! Record and reference shapes for schemars
//! (docs/architecture/records/shapes.md): a record's `$type`, a typed
//! reference, the shared shapes a string field uses, and the settings every
//! schema a built-in plugin ships is made with; and what its readers share.

use std::borrow::Cow;
use std::marker::PhantomData;

use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

/// Codes that depend on the tree rather than the record. A record holding
/// one is still a record to read.
pub const TREE_CODES: [&str; 2] = ["board::ref::dangling", "board::ref::wrong_type"];

/// Reads a JSON number, such as `5` or `5.0`, as a `u16`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "each input schema that uses it admits only whole numbers from 1 to 10,000"
)]
pub fn whole<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    f64::deserialize(deserializer).map(|number| number as u16)
}

/// A record type, named by its `$type`.
pub trait Record {
    const TYPE: &'static str;
}

/// The record types a reference may point at; none means any entry.
pub trait Accepts {
    const TYPES: &'static [&'static str];
}

impl<R: Record> Accepts for R {
    const TYPES: &'static [&'static str] = &[R::TYPE];
}

/// Any entry: a record of any type, a Markdown file, a directory or a mount.
#[derive(Debug)]
pub enum Any {}

impl Accepts for Any {
    const TYPES: &'static [&'static str] = &[];
}

/// A record's `$type` field: the string `R::TYPE` and nothing else. It
/// writes `R::TYPE` and reads only that string.
pub struct Tag<R>(PhantomData<fn() -> R>);

impl<R> std::fmt::Debug for Tag<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Tag")
    }
}

impl<R> Default for Tag<R> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<R: Record> Serialize for Tag<R> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(R::TYPE)
    }
}

impl<'de, R: Record> Deserialize<'de> for Tag<R> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text == R::TYPE {
            Ok(Self::default())
        } else {
            Err(D::Error::custom(format!(
                "$type is {text:?}, not {:?}",
                R::TYPE
            )))
        }
    }
}

impl<R: Record> JsonSchema for Tag<R> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Tag".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "const": R::TYPE})
    }
}

/// A reference: an absolute logical path to an entry of one of `T::TYPES`.
/// Always inlined, so references to different types never share a `$defs`
/// entry.
#[derive(Debug)]
pub struct Ref<T: Accepts = Any>(PhantomData<fn() -> T>);

impl<T: Accepts> JsonSchema for Ref<T> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Ref".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let mut schema = json_schema!({"type": "string", "format": "board-ref"});
        match T::TYPES {
            [] => {}
            [one] => {
                schema.insert("x-board-type".into(), (*one).into());
            }
            many => {
                schema.insert("x-board-type".into(), many.iter().copied().collect());
            }
        }
        schema
    }
}

/// Free text that a person reads and no program parses. A `String` field
/// takes it with `#[schemars(with = "Prose")]`.
#[derive(Debug)]
pub enum Prose {}

impl JsonSchema for Prose {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Prose".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "x-board-prose": true})
    }
}

/// A calendar date, such as `2026-11-01`. A `String` field takes it with
/// `#[schemars(with = "Date")]`.
#[derive(Debug)]
pub enum Date {}

impl JsonSchema for Date {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Date".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "format": "date"})
    }
}

/// A web address: an absolute `http` or `https` URI with a host, such as
/// `https://example.com/pull/1`. A `urls` field takes it with
/// `#[schemars(with = "Vec<Url>")]`. Board's validator is the only check.
#[derive(Debug)]
pub enum Url {}

/// `format: uri` admits `https://`, `https://:80` and `https://u@`, which
/// have no host. After any user information the pattern needs a host
/// character, and its tail is anchored at the next `/`, `?`, `#` or the end,
/// so that the optional user-information group must take an `@` rather than
/// let the regex read the user as the host. JSON Schema patterns are
/// case-sensitive, so `HTTPS://` is refused.
const URL_PATTERN: &str = "^https?://([^/?#@]*@)?[^/?#@:][^/?#@]*([/?#]|$)";

impl JsonSchema for Url {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Url".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "format": "uri", "pattern": URL_PATTERN})
    }
}

/// A hash Board defines: its algorithm, `sha256:`, and 64 lowercase hex
/// digits, such as an effect's id in a record or the digest of some
/// content. A later Board can admit a second algorithm by widening the
/// pattern. A `String` field takes it with `#[schemars(with = "Digest")]`.
#[derive(Debug)]
pub enum Digest {}

impl JsonSchema for Digest {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Digest".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"})
    }
}

/// An `SQLite` GLOB pattern, matched with case: `*` matches any characters,
/// `/` and `.` included, `?` one character, and `[...]` one of a set, with
/// `[^...]` its complement. A `String` field takes it with
/// `#[schemars(with = "Glob")]`, and a plugin matches it with GLOB in a
/// query, never with code of its own.
#[derive(Debug)]
pub enum Glob {}

impl JsonSchema for Glob {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Glob".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "minLength": 1, "x-board-glob": true})
    }
}

/// A [`Glob`] by logical path, such as `/repos/inbox/crates/*.rs`, which
/// starts at the board's root as a logical path does. A `String` field
/// takes it with `#[schemars(with = "LogicalGlob")]`.
#[derive(Debug)]
pub enum LogicalGlob {}

impl JsonSchema for LogicalGlob {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "LogicalGlob".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "minLength": 1, "pattern": "^/", "x-board-glob": true})
    }
}

/// A one-line summary that a person or an agent reads in place of longer
/// text: at most [`SUMMARY_WORDS`] words, separated by single spaces, with
/// no line break. A `String` field takes it with
/// `#[schemars(with = "Summary")]`.
#[derive(Debug)]
pub enum Summary {}

/// The most words a [`Summary`] holds.
pub const SUMMARY_WORDS: usize = 25;

impl JsonSchema for Summary {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Summary".into()
    }

    /// One to [`SUMMARY_WORDS`] runs of non-space characters, each after the
    /// first preceded by one space, so the text is one line and its words
    /// are counted as Board counts words, by whitespace.
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let pattern = format!("^\\S+( \\S+){{0,{}}}$", SUMMARY_WORDS - 1);
        json_schema!({"type": "string", "pattern": pattern})
    }
}

/// A string shape: a `pattern` with an inline schema named `$name`. A
/// `String` field takes it with `#[schemars(with = "Name")]`.
macro_rules! patterned {
    ($(#[$doc:meta])* $name:ident = $pattern:expr) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub enum $name {}

        impl JsonSchema for $name {
            fn inline_schema() -> bool {
                true
            }

            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({"type": "string", "pattern": $pattern})
            }
        }
    };
}

patterned! {
    /// A Claude Code session's id: `local_` and a lowercase uuid, such as
    /// `local_84ccdcef-4c52-487a-9ac6-ebf2f157e440`.
    SessionId = "^local_[0-9a-f-]+$"
}

patterned! {
    /// A lowercase name of letters and digits in words joined by `-`, such
    /// as `claude-code`.
    Slug = "^[a-z0-9]+(-[a-z0-9]+)*$"
}

patterned! {
    /// A person: a [`Slug`], such as `luke`, or an email address, such as
    /// `owner@example.com`.
    Person = "^([a-z0-9]+(-[a-z0-9]+)*|[^@\\s]+@[^@\\s]+)$"
}

patterned! {
    /// A git object id: 40 lowercase hex digits in a SHA-1 repository, or
    /// 64 in a SHA-256 one.
    ObjectId = "^[0-9a-f]{40}([0-9a-f]{24})?$"
}

patterned! {
    /// What an eval or a suite tests: a repository as `owner/name`, such as
    /// `crumbkit/board`, or a board path, such as `/repos/board/crates`.
    Subject = "^([A-Za-z0-9-]+/[A-Za-z0-9._-]+|/\\S+)$"
}

/// Who wrote a record or acted: the session, the program and the person,
/// each when known, and at least one. Every record type that names who
/// acted holds one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("minProperties" = 1))]
pub struct Actor {
    /// The Claude Code session's `local_` id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<SessionId>")]
    pub session: Option<String>,
    /// The program that acted, such as `claude-code` or `codex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Slug>")]
    pub program: Option<String>,
    /// The person who acted, such as `luke`, or their email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Person>")]
    pub person: Option<String>,
}

impl Actor {
    /// The actor that `session`, `program` and `person` name, or none when
    /// they name nobody.
    #[must_use]
    pub fn of(
        session: Option<String>,
        program: Option<String>,
        person: Option<String>,
    ) -> Option<Self> {
        let actor = Self {
            session,
            program,
            person,
        };
        (actor != Self::default()).then_some(actor)
    }
}

impl std::fmt::Display for Actor {
    /// The parts it holds, in the order program, person and session,
    /// separated by `, `, such as `claude-code, luke`.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<&str> = [&self.program, &self.person, &self.session]
            .into_iter()
            .filter_map(Option::as_deref)
            .collect();
        formatter.write_str(&parts.join(", "))
    }
}

/// A reference to a record pinned to the git blob it was read at, such as
/// the brief a review read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pinned {
    /// The record.
    #[schemars(with = "Ref")]
    pub record: String,
    /// The git blob id of the record as it was read.
    #[schemars(with = "ObjectId")]
    pub blob: String,
}

/// Each shared shape's schema by its name. `crates/board-plugin/shapes.json`
/// is this map, committed, and the shapes lint admits a string field whose
/// schema is one of its entries.
pub fn registry() -> Map<String, Value> {
    let mut generator = SchemaGenerator::default();
    Map::from_iter([
        object::<Actor>(),
        entry::<Date>(&mut generator),
        entry::<Glob>(&mut generator),
        entry::<LogicalGlob>(&mut generator),
        entry::<ObjectId>(&mut generator),
        entry::<Person>(&mut generator),
        object::<Pinned>(),
        entry::<Prose>(&mut generator),
        entry::<SessionId>(&mut generator),
        entry::<Digest>(&mut generator),
        entry::<Slug>(&mut generator),
        entry::<Subject>(&mut generator),
        entry::<Summary>(&mut generator),
        entry::<Url>(&mut generator),
    ])
}

fn entry<T: JsonSchema>(generator: &mut SchemaGenerator) -> (String, Value) {
    (
        T::schema_name().into_owned(),
        T::json_schema(generator).into(),
    )
}

/// An object shape as a record schema's `$defs` holds it: made as
/// [`schema_of`] makes it, without the `title` a schema's root carries.
fn object<T: JsonSchema>() -> (String, Value) {
    let mut schema = schema_of::<T>(SchemaSettings::draft2020_12());
    if let Some(keywords) = schema.as_object_mut() {
        keywords.remove("title");
    }
    (T::schema_name().into_owned(), schema)
}

/// `T`'s JSON Schema, draft 2020-12 without `$schema`. A field a writer may
/// leave out admits no null, and an integer has no `format`, which says only
/// how Rust stores it.
pub fn schema_of<T: JsonSchema>(settings: SchemaSettings) -> Value {
    let mut settings = settings.with_transform(RecursiveTransform(|schema: &mut Schema| {
        if schema.get("type").and_then(Value::as_str) == Some("integer") {
            schema.remove("format");
        }
        absent_not_null(schema);
    }));
    settings.meta_schema = None;
    settings.into_generator().into_root_schema_for::<T>().into()
}

/// Takes null out of each property `schema` does not require: out of `type`
/// and `enum`, and an `anyOf` of one schema and null becomes that schema.
fn absent_not_null(schema: &mut Schema) {
    let required = schema.get("required").cloned().unwrap_or_default();
    let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) else {
        return;
    };
    for (name, property) in properties {
        let Value::Object(property) = property else {
            continue;
        };
        if (required.as_array()).is_some_and(|names| names.iter().any(|named| named == name)) {
            continue;
        }
        if let Some(Value::Array(types)) = property.get_mut("type") {
            types.retain(|kind| kind != "null");
            if let [one] = &types[..] {
                let one = one.clone();
                property.insert("type".into(), one);
            }
        }
        if let Some(Value::Array(values)) = property.get_mut("enum") {
            values.retain(|value| !value.is_null());
        }
        if let Some(Value::Array(branches)) = property.get("anyOf") {
            let kept: Vec<Value> = (branches.iter())
                .filter(|branch| **branch != json!({"type": "null"}))
                .cloned()
                .collect();
            if let [Value::Object(only)] = &kept[..] {
                property.remove("anyOf");
                for (keyword, value) in only {
                    property.entry(keyword).or_insert_with(|| value.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
