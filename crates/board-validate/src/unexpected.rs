//! The fields a record holds that its schema does not name, where the
//! schema closes an object with `additionalProperties: false` or
//! `unevaluatedProperties: false`, which `board put` drops
//! (docs/architecture/writes.md, Fills and drops).

use jsonschema::error::ValidationErrorKind;
use serde_json::Value;

use crate::Schema;

/// A field of the object at `at`, a JSON pointer, that the schema does not
/// name, and the one field the schema names there that is one edit from it,
/// when there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unexpected {
    pub at: String,
    pub name: String,
    pub near: Option<String>,
}

impl Unexpected {
    /// The JSON pointer of the field.
    pub fn pointer(&self) -> String {
        format!("{}/{}", self.at, escaped(&self.name))
    }

    /// The JSON pointer of the field it is near, when there is one.
    pub fn near_pointer(&self) -> Option<String> {
        (self.near.as_ref()).map(|near| format!("{}/{}", self.at, escaped(near)))
    }
}

impl Schema {
    /// Every field of `record` that the schema refuses as unnamed, in the
    /// order of their pointers. A field that only a branch of `anyOf` or
    /// `oneOf` refuses is not listed, since the validator reports the branch
    /// and not the field.
    pub fn unexpected(&self, record: &Value) -> Vec<Unexpected> {
        let mut found: Vec<Unexpected> = Vec::new();
        for error in self.validator.iter_errors(record) {
            let (ValidationErrorKind::AdditionalProperties { unexpected }
            | ValidationErrorKind::UnevaluatedProperties { unexpected }) = error.kind()
            else {
                continue;
            };
            let at = error.instance_path().to_string();
            let named = self.named(&error.schema_path().to_string());
            for name in unexpected {
                let near = (named.iter()).find(|known| one_edit(known, name)).cloned();
                found.push(Unexpected {
                    at: at.clone(),
                    name: name.clone(),
                    near,
                });
            }
        }
        found.sort_by_key(Unexpected::pointer);
        found.dedup();
        found
    }

    /// The property names of the schema object that holds the keyword at
    /// `keyword`, a pointer into the document with no `$ref` followed, in
    /// sorted order.
    fn named(&self, keyword: &str) -> Vec<String> {
        let parent = keyword.rsplit_once('/').map_or("", |(parent, _)| parent);
        let properties = (self.document.pointer(parent))
            .and_then(|schema| schema.get("properties"))
            .and_then(Value::as_object);
        let mut named: Vec<String> =
            properties.map_or_default(|named| named.keys().cloned().collect());
        named.sort();
        named
    }
}

/// `name` as one JSON pointer token.
fn escaped(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

/// Whether `one` becomes `other` by inserting, deleting or replacing one
/// character.
fn one_edit(one: &str, other: &str) -> bool {
    let (one, other): (Vec<char>, Vec<char>) = (one.chars().collect(), other.chars().collect());
    let (short, long) = if one.len() <= other.len() {
        (&one, &other)
    } else {
        (&other, &one)
    };
    if long.len() - short.len() > 1 {
        return false;
    }
    let start = (short.iter().zip(long.iter()))
        .take_while(|(a, b)| a == b)
        .count();
    if short.len() == long.len() {
        start < short.len() && short[start + 1..] == long[start + 1..]
    } else {
        short[start..] == long[start + 1..]
    }
}

#[cfg(test)]
mod tests;
