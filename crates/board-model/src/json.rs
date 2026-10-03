//! The strict, bounded JSON parse that every untyped JSON read goes through
//! (docs/architecture.md, Records; STANDARDS.md 3).

use std::cell::RefCell;
use std::fmt;

use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

use crate::code::Code;
use crate::pointer::MAX_RECORD_BYTES;

/// Parses `bytes` as one JSON document. The first failure wins, in this
/// order: too large, not UTF-8, git conflict markers, not JSON (a byte-order
/// mark or nesting past `serde_json`'s limit included), a duplicate key at any
/// depth.
pub fn parse_json(bytes: &[u8]) -> Result<Value, JsonError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(JsonError::TooLarge { bytes: bytes.len() });
    }
    let text = std::str::from_utf8(bytes).map_err(|source| JsonError::NotUtf8 { source })?;
    if let Some(line) = conflict_marker(text) {
        return Err(JsonError::MergeConflict { line });
    }
    serde_json::from_str::<IgnoredAny>(text).map_err(|source| JsonError::Parse { source })?;
    let duplicate = RefCell::new(None);
    let mut deserializer = serde_json::Deserializer::from_str(text);
    Strict {
        duplicate: &duplicate,
    }
    .deserialize(&mut deserializer)
    .map_err(|source| match duplicate.take() {
        Some(key) => JsonError::DuplicateKey { key },
        None => JsonError::Parse { source },
    })
}

/// The first line, counted from 1, that git writes to mark a conflict.
fn conflict_marker(text: &str) -> Option<usize> {
    text.lines()
        .position(|line| {
            line.starts_with("<<<<<<< ")
                || line.starts_with(">>>>>>> ")
                || line.starts_with("||||||| ")
                || line == "======="
        })
        .map(|index| index + 1)
}

/// Builds a [`Value`] like `serde_json` does, but refuses a key that an
/// object already holds, recording it in `duplicate`.
#[derive(Clone, Copy)]
struct Strict<'a> {
    duplicate: &'a RefCell<Option<String>>,
}

impl<'de> DeserializeSeed<'de> for Strict<'_> {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(self)? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value_seed(self)?;
            if object.contains_key(&key) {
                *self.duplicate.borrow_mut() = Some(key);
                return Err(de::Error::custom("duplicate key"));
            }
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}

/// Bytes that are not one strict JSON document.
#[derive(Debug, thiserror::Error)]
pub enum JsonError {
    #[error("json is {bytes} bytes, over {MAX_RECORD_BYTES}")]
    TooLarge { bytes: usize },
    #[error("json is not utf-8: {source}")]
    NotUtf8 {
        #[source]
        source: std::str::Utf8Error,
    },
    #[error("json holds a git conflict marker on line {line}")]
    MergeConflict { line: usize },
    #[error("json does not parse: {source}")]
    Parse {
        #[source]
        source: serde_json::Error,
    },
    #[error("json repeats the key {key:?} in one object")]
    DuplicateKey { key: String },
}

impl JsonError {
    pub fn code(&self) -> Code {
        match self {
            Self::TooLarge { .. } => Code::JsonTooLarge,
            Self::NotUtf8 { .. } => Code::JsonNotUtf8,
            Self::MergeConflict { .. } => Code::JsonMergeConflict,
            Self::Parse { .. } => Code::JsonParse,
            Self::DuplicateKey { .. } => Code::JsonDuplicateKey,
        }
    }
}

#[cfg(test)]
mod tests;
