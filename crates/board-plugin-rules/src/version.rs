//! A version rule: every version its expression captures in a file must be
//! at least the rule's `at_least`.

use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value};

use board_ops::PluginError;

use crate::evaluate::lines;
use crate::records::Version;

/// A captured version older than the rule's, and the line it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Behind {
    pub(crate) line: usize,
    pub(crate) found: String,
}

/// The rule's expression, compiled with `^` and `$` matching at each line,
/// or `rules::capture::invalid` when it does not compile or has no group.
pub(crate) fn compile(control: &str, rule: &Version) -> Result<Regex, PluginError> {
    let built = RegexBuilder::new(&rule.capture).multi_line(true).build();
    let why = match built {
        Ok(expression) if expression.captures_len() > 1 => return Ok(expression),
        Ok(_) => "it has no group to capture the version".to_owned(),
        Err(error) => error.to_string(),
    };
    Err(PluginError {
        code: "rules::capture::invalid".to_owned(),
        message: format!(
            "{control}'s capture {:?} cannot find a version: {why}; give it one group, such as \
             `channel = \"([0-9.]+)\"`",
            rule.capture
        ),
        fields: Map::from_iter([("rule".to_owned(), Value::from(control))]),
        hint: None,
    })
}

/// Each version `expression`'s first group captures in `text` that is older
/// than `at_least`, by line.
pub(crate) fn behind(expression: &Regex, at_least: &str, text: &str) -> Vec<Behind> {
    let floor = numbers(at_least).expect("the schema admits only dotted numbers as at_least");
    (pins(expression, text).into_iter())
        .filter(|(_, pinned, _)| older(pinned, &floor))
        .map(|(line, _, found)| Behind { line, found })
        .collect()
}

/// How many versions `expression`'s first group captures in `text`, older
/// or not.
pub(crate) fn found(expression: &Regex, text: &str) -> usize {
    pins(expression, text).len()
}

/// Each version `expression`'s first group captures in `text`: its line,
/// its numbers and its text. A capture that is not numbers separated by
/// dots is no version and is skipped.
fn pins(expression: &Regex, text: &str) -> Vec<(usize, Vec<u64>, String)> {
    let mut found = Vec::new();
    for (line, content) in (1..).zip(lines(text)) {
        for captures in expression.captures_iter(content) {
            let Some(version) = captures.get(1).map(|group| group.as_str()) else {
                continue;
            };
            if let Some(pinned) = numbers(version) {
                found.push((line, pinned, version.to_owned()));
            }
        }
    }
    found
}

/// The numbers of a version such as `1.99.1`, or none when it is not
/// numbers separated by single dots.
fn numbers(version: &str) -> Option<Vec<u64>> {
    version.split('.').map(|part| part.parse().ok()).collect()
}

/// Whether `pinned` is older than `floor`, compared over only the numbers
/// `pinned` names: a pin of `26` follows every `26.x` release, so it is not
/// older than `26.13.0`.
fn older(pinned: &[u64], floor: &[u64]) -> bool {
    let named = &floor[..pinned.len().min(floor.len())];
    pinned < named
}

#[cfg(test)]
mod tests;
