//! Record and reference shapes for schemars
//! (docs/architecture/records/shapes.md): a record's `$type`, a typed
//! reference, and the settings every schema the plugin ships is made with.

use std::borrow::Cow;
use std::marker::PhantomData;

use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde_json::{Value, json};

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
