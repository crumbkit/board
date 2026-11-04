//! Rendering a prompt: its body, the input values it takes, and one pass of
//! substitution (plugins/prompts/README.md, Rendering). Every function here
//! is pure.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde_json::{Value, json};
use unicode_general_category::{GeneralCategory, get_general_category};

use crate::refusal;
use board_ops::PluginError;

/// `{name}` for an input's name, as the schema's `inputs` admits it.
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{([a-z][a-z0-9_]{0,31})\}").expect("the placeholder pattern compiles")
});

/// The longest input value, in characters. A value is a short identifier,
/// such as a pull request number; the limit keeps a value from carrying a
/// second set of instructions into the prompt.
pub(crate) const MAX_VALUE: usize = 200;

/// Input values by name.
pub(crate) type Values = BTreeMap<String, String>;

/// Whether `value` is too long, spans lines, or holds a control or format
/// character such as U+202E or U+200B.
pub(crate) fn unsafe_value(value: &str) -> bool {
    value.chars().count() > MAX_VALUE
        || value.chars().any(|c| {
            matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' | '\u{2028}' | '\u{2029}')
                || get_general_category(c) == GeneralCategory::Format
        })
}

/// The text after the line that closes the frontmatter, `---` or `...`, as
/// Board reads a Markdown record; empty when no line closes it.
pub(crate) fn body(text: &str) -> &str {
    let mut at = 0;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        at += line.len();
        let bare = line.trim_end_matches(['\r', '\n']);
        if index > 0 && (bare == "---" || bare == "...") {
            return &text[at..];
        }
    }
    ""
}

/// Names joined by `, `.
fn joined(names: &[&str]) -> String {
    names.join(", ")
}

/// The values `given`, once each names a declared input, every declared
/// input has one, and each is one line.
pub(crate) fn checked(
    declared: &BTreeMap<String, Value>,
    given: &Values,
) -> Result<Values, PluginError> {
    let unknown: Vec<&str> = (given.keys())
        .filter(|name| !declared.contains_key(*name))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        let takes: Vec<&str> = declared.keys().map(String::as_str).collect();
        let takes = if takes.is_empty() {
            "none".to_owned()
        } else {
            joined(&takes)
        };
        let why = format!(
            "no input is named {}; the prompt takes {takes}",
            joined(&unknown)
        );
        return Err(refusal(
            "prompts::input::unknown",
            why,
            [("inputs", json!(unknown))],
        ));
    }
    let missing: Vec<&str> = (declared.keys())
        .filter(|name| !given.contains_key(*name))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        let why = format!(
            "the prompt needs a value for {}; give each in inputs on stdin",
            joined(&missing)
        );
        return Err(refusal(
            "prompts::input::missing",
            why,
            [("inputs", json!(missing))],
        ));
    }
    let bad: Vec<&str> = (given.iter())
        .filter(|(_, value)| unsafe_value(value))
        .map(|(name, _)| name.as_str())
        .collect();
    if !bad.is_empty() {
        let why = format!(
            "the value of {} is longer than {MAX_VALUE} characters, spans lines or holds a control \
             or format character; put a longer text in the prompt itself or in a separate brief",
            joined(&bad)
        );
        return Err(refusal(
            "prompts::input::bad_value",
            why,
            [("inputs", json!(bad))],
        ));
    }
    Ok(given.clone())
}

/// `text` with each `{name}` of an input in `values` replaced by its value,
/// in one pass; other braces stay.
pub(crate) fn render(text: &str, values: &Values) -> String {
    (PLACEHOLDER.replace_all(text, |found: &Captures<'_>| {
        values
            .get(&found[1])
            .cloned()
            .unwrap_or_else(|| found[0].to_owned())
    }))
    .into_owned()
}

/// The declared inputs `text` never names, by name.
pub(crate) fn unused(declared: &BTreeMap<String, Value>, text: &str) -> Vec<String> {
    let named: Vec<&str> = (PLACEHOLDER.captures_iter(text))
        .filter_map(|found| found.get(1).map(|name| name.as_str()))
        .collect();
    (declared.keys())
        .filter(|name| !named.contains(&name.as_str()))
        .cloned()
        .collect()
}

/// Whether `value` is a GitHub owner/name whose name is not `.` or `..`.
fn remote(value: &str) -> bool {
    let Some((owner, name)) = value.split_once('/') else {
        return false;
    };
    let in_owner = |c: char| c.is_ascii_alphanumeric() || c == '-';
    let in_name = |c: char| in_owner(c) || matches!(c, '.' | '_');
    !owner.is_empty()
        && owner.chars().all(in_owner)
        && !name.is_empty()
        && name.chars().all(in_name)
        && name != "."
        && name != ".."
}

/// Whether `value` is an absolute path, or one under `~/`, with no `..`
/// segment.
fn directory(value: &str) -> bool {
    let rest = value.strip_prefix("~/").or_else(|| value.strip_prefix('/'));
    rest.is_some_and(|rest| !rest.split('/').any(|segment| segment == ".."))
}

/// `runs_in` rendered, once the rendered value is still an owner/name or
/// an absolute path that stays put. The rendered value obeys the rules for
/// an input value too, so an override cannot carry a control or format
/// character, such as a tab or U+202E, into the place the work starts.
pub(crate) fn placed(key: &str, written: &str, values: &Values) -> Result<Value, PluginError> {
    let value = render(written, values);
    let shaped = if key == "remote" {
        remote(&value)
    } else {
        directory(&value)
    };
    if unsafe_value(&value) || !shaped {
        let named: Vec<&str> = (PLACEHOLDER.captures_iter(written))
            .filter_map(|found| found.get(1).map(|name| name.as_str()))
            .filter(|name| values.contains_key(*name))
            .collect();
        let kind = if key == "remote" {
            "a GitHub owner/name"
        } else {
            "an absolute path or one under ~/ without a .. segment"
        };
        let inputs = if named.is_empty() {
            "none".to_owned()
        } else {
            joined(&named)
        };
        let why = format!(
            "runs_in.{key} renders as {}, which is not {kind}; check the values of the inputs: {inputs}",
            repr(&value)
        );
        let fields = [("runs_in", json!({key: value})), ("inputs", json!(named))];
        return Err(refusal("prompts::runs_in::invalid", why, fields));
    }
    Ok(json!({key: value}))
}

/// `text` as Python's `repr` writes a string, which the Python plugin's
/// messages used: in single quotes unless it holds one and no double
/// quote, with a backslash before each quote and backslash, and each
/// character that is not printable escaped.
pub(crate) fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut shown = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => shown.push_str("\\\\"),
            '\n' => shown.push_str("\\n"),
            '\r' => shown.push_str("\\r"),
            '\t' => shown.push_str("\\t"),
            c if c == quote => {
                shown.push('\\');
                shown.push(c);
            }
            c if printable(c) => shown.push(c),
            c => {
                let code = u32::from(c);
                // Writing to a String does not fail.
                let _ = match code {
                    0..=0xff => write!(shown, "\\x{code:02x}"),
                    0x100..=0xffff => write!(shown, "\\u{code:04x}"),
                    _ => write!(shown, "\\U{code:08x}"),
                };
            }
        }
    }
    shown.push(quote);
    shown
}

/// Whether Python's `str.isprintable` holds for `c`: a space, or a
/// character outside the control, format, surrogate, private-use,
/// unassigned and separator categories.
fn printable(c: char) -> bool {
    use GeneralCategory as G;
    c == ' '
        || !matches!(
            get_general_category(c),
            G::Control
                | G::Format
                | G::Surrogate
                | G::PrivateUse
                | G::Unassigned
                | G::SpaceSeparator
                | G::LineSeparator
                | G::ParagraphSeparator
        )
}
