//! Record and reference shapes for schemars
//! (docs/architecture/records/shapes.md): a record's `$type`, a typed
//! reference, the shared shapes a string field uses, such as a web address,
//! and the settings every schema a built-in plugin ships is made with; and
//! what its readers share.

use std::borrow::Cow;
use std::marker::PhantomData;

use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};

mod url;

pub use url::{Url, UrlError};

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

/// A record's `$type` field: the string `R::TYPE` and nothing else.
#[derive(Debug)]
pub struct Tag<R>(PhantomData<fn() -> R>);

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

/// Each shared shape's schema by its name. `crates/board-plugin/shapes.json`
/// is this map, committed, and the shapes lint admits a string field whose
/// schema is one of its entries.
pub fn registry() -> Map<String, Value> {
    let mut generator = SchemaGenerator::default();
    Map::from_iter([
        entry::<Date>(&mut generator),
        entry::<Prose>(&mut generator),
        entry::<Url>(&mut generator),
    ])
}

fn entry<T: JsonSchema>(generator: &mut SchemaGenerator) -> (String, Value) {
    (
        T::schema_name().into_owned(),
        T::json_schema(generator).into(),
    )
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
