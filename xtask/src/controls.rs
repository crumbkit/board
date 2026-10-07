//! `controls/budgets/<section>.json` records the lint.toml word budget
//! `[section]`, which this lint enforces. The two must agree.

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::LintError;
use crate::config::Config;
use crate::files::Root;

const DIRECTORY: &str = "controls/budgets";

/// The fields this check reads. The control's schema decides the rest.
#[derive(Debug, Deserialize)]
struct Control {
    requirement: String,
    #[serde(default)]
    applies_to: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
    rule: Option<serde_json::Value>,
}

/// One field of a budget control that disagrees with lint.toml: the control,
/// the field, its value, what lint.toml says, and what to change.
#[derive(Debug, thiserror::Error)]
#[error("{0}: {1} = {2}, but lint.toml {3}; {4}")]
pub(crate) struct Divergence(String, &'static str, String, String, &'static str);

impl Divergence {
    pub(crate) const CODE: &str = "xtask::control::differs";
}

/// Every way the controls in `controls/budgets/` and lint.toml's word budgets
/// disagree. A control that is not JSON or has no requirement stops the lint.
pub(crate) fn check(root: &Root, config: &Config) -> Result<Vec<Divergence>, LintError> {
    let budgets = config.word_budgets();
    let mut divergences = Vec::new();
    for path in root.files_with_extension(DIRECTORY, "json")? {
        let name = path.file_stem().unwrap_or_default().to_string_lossy();
        if !budgets.iter().any(|(section, ..)| *section == name) {
            divergences.push(Divergence(
                root.display(&path),
                "file name",
                name.to_string(),
                format!("has no [{name}] word budget"),
                "rename the control after a word budget, or remove it",
            ));
        }
    }
    for (section, covers, max_words) in budgets {
        let control = format!("{DIRECTORY}/{section}.json");
        let differs =
            |field, found, expected, fix| Divergence(control.clone(), field, found, expected, fix);
        if !root.path().join(&control).is_file() {
            divergences.push(differs(
                "file",
                "absent".to_owned(),
                format!("[{section}] sets max_words = {max_words}"),
                "add the control, naming the same files and stating that number in its requirement",
            ));
            continue;
        }
        let record: Control =
            serde_json::from_str(&root.read(&control)?).map_err(|source| LintError::Control {
                path: control.clone(),
                source,
            })?;
        let found: BTreeSet<&str> = record
            .applies_to
            .iter()
            .map(|reference| reference.strip_prefix('/').unwrap_or(reference))
            .chain(record.paths.iter().map(String::as_str))
            .collect();
        let expected: BTreeSet<&str> = covers.iter().map(String::as_str).collect();
        if found != expected {
            divergences.push(differs(
                "applies_to and paths",
                format!("{found:?}"),
                format!("[{section}] names {expected:?}"),
                "change the control to name the same files",
            ));
        }
        if numbers(&record.requirement) != [max_words] {
            divergences.push(differs(
                "requirement",
                format!("{:?}", record.requirement),
                format!("[{section}] sets max_words = {max_words}"),
                "make that the requirement's only number, or change lint.toml in a change that says why",
            ));
        }
        if let Some(rule) = record.rule {
            divergences.push(differs(
                "rule",
                rule.to_string(),
                format!("[{section}] enforces this budget"),
                "remove rule, because a budget control only records lint.toml",
            ));
        }
    }
    Ok(divergences)
}

/// The numbers `text` writes, with or without thousands separators.
fn numbers(text: &str) -> Vec<usize> {
    text.split(|character: char| !(character.is_ascii_digit() || character == ','))
        .filter_map(|number| number.replace(',', "").parse().ok())
        .collect()
}

#[cfg(test)]
mod tests;
