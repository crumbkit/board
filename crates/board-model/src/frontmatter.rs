//! YAML frontmatter in Markdown files, which makes a file a record when it
//! names a `$type` (docs/architecture.md, Records).

use std::fmt;

use saphyr_parser::{Event, Parser, ScalarStyle};
use serde_json::{Map, Number, Value};

use crate::code::Code;
use crate::json::exact;

/// How many arrays and objects frontmatter may nest, as many as
/// `serde_json` reads back: its recursion limit of 128 counts one more than
/// the deepest value it accepts.
const MAX_DEPTH: usize = 127;

/// The YAML between a first line `---` and the next line `---` or `...`,
/// and the text after that line, ignoring one leading byte order mark.
/// `None` when the text does not open with such a block.
pub(crate) fn split(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n').trim_end_matches('\r');
        if bare == "---" || bare == "..." {
            return Some((&rest[..at], &rest[at + line.len()..]));
        }
        at += line.len();
    }
    None
}

/// The block and text of [`split`] when the block claims Board's reading by
/// a line that opens with `$type:`. Board leaves other frontmatter, such as
/// a notes app's, to its writer and reads the file as a whole.
fn claimed(text: &str) -> Option<(&str, &str)> {
    split(text).filter(|(yaml, _)| yaml.lines().any(|line| line.starts_with("$type:")))
}

/// The frontmatter of the Markdown file holding `bytes`, read as YAML 1.2
/// with the core schema into JSON, or `None` when the file is not UTF-8 or
/// has no block that claims a `$type`.
pub fn frontmatter(bytes: &[u8]) -> Result<Option<Value>, FrontmatterError> {
    let Some((yaml, _)) = std::str::from_utf8(bytes).ok().and_then(claimed) else {
        return Ok(None);
    };
    value(yaml).map(Some)
}

/// A container being built: its items, or its members and the key whose
/// value comes next.
enum Frame {
    Sequence(Vec<Value>),
    Mapping(Map<String, Value>, Option<String>),
}

/// The JSON value of one YAML document, built from the parser's events
/// without recursion.
fn value(yaml: &str) -> Result<Value, FrontmatterError> {
    let mut parser = Parser::new_from_str(yaml);
    let mut stack: Vec<Frame> = Vec::new();
    let mut root = None;
    let mut documents = 0;
    while let Some(next) = parser.next_event() {
        let (event, span) = next.map_err(|error| FrontmatterError::Parse {
            line: error.marker().line() + 1,
            info: error.info().to_owned(),
        })?;
        // The block starts on the file's second line.
        let line = span.start.line() + 1;
        let unsupported = |feature| FrontmatterError::Unsupported { line, feature };
        let (anchor, tag) = match &event {
            Event::Scalar(_, _, anchor, tag)
            | Event::SequenceStart(anchor, tag)
            | Event::MappingStart(anchor, tag) => (*anchor, tag.as_ref()),
            _ => (0, None),
        };
        if anchor != 0 {
            return Err(unsupported(Feature::Anchor));
        }
        if let Some(tag) = tag {
            return Err(unsupported(Feature::Tag(format!(
                "{}{}",
                tag.handle, tag.suffix
            ))));
        }
        let awaits_key = matches!(stack.last(), Some(Frame::Mapping(_, None)));
        let built = match event {
            Event::DocumentStart(_) => {
                documents += 1;
                if documents > 1 {
                    return Err(unsupported(Feature::Documents));
                }
                continue;
            }
            // An alias names an anchor, which is refused where it is set, so
            // one that reaches here names no anchor and the parser refuses it.
            Event::Alias(_) => return Err(unsupported(Feature::Anchor)),
            Event::SequenceStart(..) | Event::MappingStart(..) if awaits_key => {
                return Err(unsupported(Feature::Key));
            }
            Event::SequenceStart(..) | Event::MappingStart(..) if stack.len() >= MAX_DEPTH => {
                return Err(unsupported(Feature::Deep));
            }
            Event::SequenceStart(..) => {
                stack.push(Frame::Sequence(Vec::new()));
                continue;
            }
            Event::MappingStart(..) => {
                stack.push(Frame::Mapping(Map::new(), None));
                continue;
            }
            Event::SequenceEnd | Event::MappingEnd => match stack.pop() {
                Some(Frame::Sequence(items)) => Value::Array(items),
                Some(Frame::Mapping(members, _)) => Value::Object(members),
                None => continue,
            },
            Event::Scalar(text, style, ..) => {
                let scalar = scalar(&text, style)
                    .map_err(|lexeme| unsupported(Feature::Number(lexeme.to_owned())))?;
                if awaits_key {
                    let Value::String(key) = scalar else {
                        return Err(unsupported(Feature::Key));
                    };
                    if let Some(Frame::Mapping(members, pending)) = stack.last_mut() {
                        if members.contains_key(&key) {
                            return Err(FrontmatterError::DuplicateKey { line, key });
                        }
                        *pending = Some(key);
                    }
                    continue;
                }
                scalar
            }
            Event::Nothing | Event::StreamStart | Event::StreamEnd | Event::DocumentEnd => {
                continue;
            }
        };
        match stack.last_mut() {
            None => root = Some(built),
            Some(Frame::Sequence(items)) => items.push(built),
            Some(Frame::Mapping(members, pending)) => {
                if let Some(key) = pending.take() {
                    members.insert(key, built);
                }
            }
        }
    }
    Ok(root.unwrap_or(Value::Null))
}

/// A scalar's value under the YAML 1.2 core schema (YAML 1.2.2, 10.3.2): a
/// quoted or block scalar is a string, and a plain one may be null, a
/// boolean, an integer or a float. A number JSON cannot hold exactly, such
/// as `.inf` or an integer past 64 bits, is its lexeme as the error.
fn scalar(text: &str, style: ScalarStyle) -> Result<Value, &str> {
    if style != ScalarStyle::Plain {
        return Ok(Value::String(text.to_owned()));
    }
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Ok(Value::Null),
        "true" | "True" | "TRUE" => return Ok(Value::Bool(true)),
        "false" | "False" | "FALSE" => return Ok(Value::Bool(false)),
        _ => {}
    }
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(text, ".nan" | ".NaN" | ".NAN") {
        return Err(text);
    }
    if let Some((digits, radix)) = integer(text) {
        let number = u64::from_str_radix(digits, radix).ok().map(Number::from);
        return number.map(Value::Number).ok_or(text);
    }
    if float(text) {
        // `exact` reads a lexeme without a leading `+`.
        let lexeme = text.strip_prefix('+').unwrap_or(text);
        let number = if lexeme.contains(['.', 'e', 'E']) {
            lexeme
                .parse::<f64>()
                .ok()
                .filter(|_| exact(lexeme))
                .and_then(Number::from_f64)
        } else {
            let signed = lexeme.parse::<i64>().map(Number::from);
            signed
                .or_else(|_| lexeme.parse::<u64>().map(Number::from))
                .ok()
        };
        return number.map(Value::Number).ok_or(text);
    }
    Ok(Value::String(text.to_owned()))
}

/// The digits and radix of a core-schema octal `0o[0-7]+` or hexadecimal
/// `0x[0-9a-fA-F]+` integer. A decimal integer is read as a float is.
fn integer(text: &str) -> Option<(&str, u32)> {
    let digits =
        |text: &str, valid: fn(&u8) -> bool| !text.is_empty() && text.bytes().all(|b| valid(&b));
    if let Some(octal) = text.strip_prefix("0o") {
        return digits(octal, |b| (b'0'..=b'7').contains(b)).then_some((octal, 8));
    }
    let hex = text.strip_prefix("0x")?;
    digits(hex, u8::is_ascii_hexdigit).then_some((hex, 16))
}

/// Whether `text` is a core-schema decimal integer or a float other than
/// infinity and NaN: `[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?`.
fn float(text: &str) -> bool {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (unsigned, None),
    };
    let all_digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    let mantissa = match mantissa.split_once('.') {
        Some(("", fraction)) => !fraction.is_empty() && all_digits(fraction),
        Some((whole, fraction)) => all_digits(whole) && all_digits(fraction),
        None => !mantissa.is_empty() && all_digits(mantissa),
    };
    let exponent = exponent.is_none_or(|exponent| {
        let digits = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
        !digits.is_empty() && all_digits(digits)
    });
    mantissa && exponent
}

/// The text after a Markdown file's frontmatter, or all of it when it has
/// no block that claims a `$type`.
pub(crate) fn body(text: &str) -> &str {
    claimed(text).map_or(text, |(_, body)| body)
}

/// YAML that JSON cannot hold, which frontmatter refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Feature {
    /// An anchor, or an alias to one.
    Anchor,
    Tag(String),
    /// A key that is not a string, such as `1` or a sequence.
    Key,
    /// A number JSON cannot hold exactly, as written.
    Number(String),
    /// A second YAML document in the block.
    Documents,
    /// Nesting deeper than 127 arrays and objects.
    Deep,
}

impl fmt::Display for Feature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anchor => formatter.write_str("an anchor"),
            Self::Tag(tag) => write!(formatter, "the tag {tag}"),
            Self::Key => formatter.write_str("a key that is not a string"),
            Self::Number(lexeme) => write!(
                formatter,
                "the number {lexeme}, which cannot be held exactly"
            ),
            Self::Documents => formatter.write_str("a second document"),
            Self::Deep => write!(formatter, "nesting deeper than {MAX_DEPTH} levels"),
        }
    }
}

/// Frontmatter that is not YAML, or is YAML that JSON cannot hold. A line
/// is the file's, counted from 1.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrontmatterError {
    #[error("frontmatter does not parse: {info} on line {line}")]
    Parse { line: usize, info: String },
    #[error("frontmatter repeats the key {key:?} in one mapping on line {line}")]
    DuplicateKey { line: usize, key: String },
    #[error("frontmatter holds {feature} on line {line}, which JSON cannot hold")]
    Unsupported { line: usize, feature: Feature },
}

impl FrontmatterError {
    pub fn code(&self) -> Code {
        match self {
            Self::Parse { .. } | Self::DuplicateKey { .. } => Code::FrontmatterParse,
            Self::Unsupported { .. } => Code::FrontmatterUnsupported,
        }
    }
}

#[cfg(test)]
mod tests;
