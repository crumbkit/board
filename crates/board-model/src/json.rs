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
/// depth, a number that cannot be held exactly.
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
    .and_then(|value| match lossy_number(text) {
        Some(lexeme) => Err(JsonError::LossyNumber {
            lexeme: lexeme.to_owned(),
        }),
        None => Ok(value),
    })
}

/// The document's canonical bytes (docs/architecture.md, Canonical form):
/// keys sorted by code point, two-space indentation, one key or array
/// element per line, empty objects and arrays on one line, numbers as
/// `serde_json` prints them, `\n` endings and one trailing newline.
///
/// # Panics
///
/// Never: a `Value`'s keys are all strings, and writing to a `Vec` cannot
/// fail.
pub fn canonical_json(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .expect("a Value writes to a Vec: every key is a string and no write can fail");
    bytes.push(b'\n');
    bytes
}

/// The first number in the valid JSON `text` that Board cannot hold exactly,
/// as written. A number is held exactly when its value survives being read:
/// an integer must fit `i64` or `u64`, and any other number must have the
/// same decimal value as the shortest decimal that reads back to the same
/// `f64`. When the `f64` lies exactly halfway between two equally short
/// decimals, either is held. So `0.1`, `1.50`, `1111111111111111.2` and
/// `1111111111111111.3` (the `f64` is `.25`) are held, and `2.5e-324` and
/// `9007199254740993.0` (the `f64` is `9007199254740992`) are not.
fn lossy_number(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'"' => {
                at += 1;
                while let Some(&byte) = bytes.get(at) {
                    at += if byte == b'\\' { 2 } else { 1 };
                    if byte == b'"' {
                        break;
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = at;
                while bytes.get(at).is_some_and(|byte| {
                    matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                }) {
                    at += 1;
                }
                let lexeme = text.get(start..at)?;
                if !exact(lexeme) {
                    return Some(lexeme);
                }
            }
            _ => at += 1,
        }
    }
    None
}

/// Whether the number written `lexeme` is held exactly; see [`lossy_number`].
fn exact(lexeme: &str) -> bool {
    if !lexeme.contains(['.', 'e', 'E']) {
        return lexeme.parse::<i64>().is_ok() || lexeme.parse::<u64>().is_ok();
    }
    let Ok(value) = lexeme.parse::<f64>() else {
        return false;
    };
    let (Some(written), Some(shortest)) = (decimal(lexeme), decimal(&format!("{value:e}"))) else {
        return false;
    };
    value.is_finite()
        && (written == shortest
            || written.1.len() == shortest.1.len() && halfway(value, &written, &shortest))
}

/// Whether `value` is exactly halfway between the decimals `a` and `b`.
/// Rust prints a double's exact value when given a precision, and no double
/// has more than 767 significant digits.
fn halfway(value: f64, a: &(bool, String, i64), b: &(bool, String, i64)) -> bool {
    let low = a.2.min(b.2);
    let scaled = |(_, digits, exponent): &(bool, String, i64)| {
        let shift = u32::try_from(exponent.checked_sub(low)?).ok()?;
        digits
            .parse::<u128>()
            .ok()?
            .checked_mul(10u128.checked_pow(shift)?)
    };
    let Some(twice) = scaled(a).zip(scaled(b)).and_then(|(a, b)| a.checked_add(b)) else {
        return false;
    };
    let sign = if a.0 { "-" } else { "" };
    let middle = twice.checked_mul(5).zip(low.checked_sub(1));
    middle.is_some_and(|(digits, exponent)| {
        decimal(&format!("{sign}{digits}e{exponent}")) == decimal(&format!("{value:.800e}"))
    })
}

/// The value of a decimal as its sign, its significant digits and the power
/// of ten of the last of them; zero is one value whatever its sign.
fn decimal(text: &str) -> Option<(bool, String, i64)> {
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i64>().ok()?),
        None => (text, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    let significant = digits.trim_start_matches('0').trim_end_matches('0');
    if significant.is_empty() {
        return Some((false, String::new(), 0));
    }
    let trailing = digits.trim_start_matches('0').len() - significant.len();
    let exponent = exponent
        .checked_sub(i64::try_from(fraction.len()).ok()?)?
        .checked_add(i64::try_from(trailing).ok()?)?;
    Some((negative, significant.to_owned(), exponent))
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
    #[error("number {lexeme} cannot be held exactly")]
    LossyNumber { lexeme: String },
}

impl JsonError {
    pub fn code(&self) -> Code {
        match self {
            Self::TooLarge { .. } => Code::JsonTooLarge,
            Self::NotUtf8 { .. } => Code::JsonNotUtf8,
            Self::MergeConflict { .. } => Code::JsonMergeConflict,
            Self::Parse { .. } => Code::JsonParse,
            Self::DuplicateKey { .. } => Code::JsonDuplicateKey,
            Self::LossyNumber { .. } => Code::JsonLossyNumber,
        }
    }
}

#[cfg(test)]
mod tests;
