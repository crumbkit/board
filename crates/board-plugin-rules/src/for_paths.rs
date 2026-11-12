//! `for-paths`: the controls that apply to any of several files and
//! patterns, such as the paths a change's plan declares.

use std::collections::{BTreeMap, BTreeSet};

use board_model::LogicalPath;
use board_ops::{Host, PluginError, literal};
use board_plugin::shape::{LogicalGlob, Prose};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::check::read;
use crate::for_path;
use crate::index::{self, Control, FILES};
use crate::overlap::overlap;
use crate::records::{Checker, RULE_TYPES};
use crate::summary::summary;
use crate::version;

/// The files and patterns to list the controls of.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    /// Files and `SQLite` GLOB patterns by logical path, such as `/repos/inbox/crates/inbox-store/**`, where `*` also matches `/`.
    #[schemars(length(min = 1), with = "Vec<LogicalGlob>")]
    paths: Vec<String>,
}

/// Each control that applies to one of the paths, once, by the control's
/// path.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ForPathsOutput")]
pub(crate) struct Output {
    controls: Vec<Applying>,
}

/// A control, its summary, or else its requirement's first sentence cut
/// to 25 words, the asked paths that reached it, in the order they were
/// asked, and how to close it when it is an obligation.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Applying {
    control: String,
    summary: String,
    matched: Vec<String>,
    /// For a version control, how many versions its capture finds, older
    /// or not, in the files that both an asked path and the control reach;
    /// left out for any other control, and when one of those files cannot
    /// be read or a pattern asked matches no file yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    versions_found: Option<usize>,
    /// How to close an obligation; left out for a standing control.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    close: Option<String>,
    /// What left an obligation owed; left out when it names nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    owed_by: Option<String>,
}

/// Each readable control that applies to one of the paths.
///
/// A path with no `*`, `?` or `[` is a file, matched as `for-path` matches
/// it. Any other path is a pattern. A pattern reaches a control whose
/// `applies_to` names a path the pattern matches, and a control one of
/// whose `paths` matches a file the pattern matches, by the file's display
/// path. It also reaches a control one of whose `paths` some path the
/// pattern matches could match, whether or not a file is there yet, so
/// `/crates/new/**` reaches a control on `crates/*.rs`.
///
/// A version control is measured as `check` measures it, over the files
/// that both an asked path and the control reach, at the commit each mount
/// pins: a file path that names a file, and each file a pattern matches
/// that one of the control's `paths` matches or its `applies_to` names. A
/// file that cannot be read, or a pattern that matches no file yet, leaves
/// the count unknown.
pub(crate) fn for_paths(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { paths } = PluginError::read_input("controls", "for-paths", input)?;
    let mut controls = index::controls(host)?;
    let mut reached: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for path in paths.iter().filter(|path| seen.insert(*path)) {
        let applying = if path.contains(['*', '?', '[']) {
            let pattern = path.strip_prefix('/').unwrap_or(path);
            let mut applying = globbed(host, pattern)?;
            applying.extend(
                (controls.iter())
                    .filter(|(_, control)| {
                        (control.paths.iter().flatten()).any(|glob| overlap(pattern, glob))
                    })
                    .map(|(logical, _)| logical.clone()),
            );
            applying
        } else {
            for_path::applying(host, path)?
        };
        for control in applying {
            reached.entry(control).or_default().push(path.clone());
        }
    }
    let mut found = versions_found(host, &reached, &controls)?;
    let controls = (reached.into_iter())
        .filter_map(|(logical, matched)| {
            let control = controls.remove(&logical)?;
            Some(Applying {
                close: control.kind().close(&logical),
                owed_by: control.owed(),
                versions_found: found.remove(&logical),
                control: logical,
                summary: (control.summary).unwrap_or_else(|| summary(&control.requirement)),
                matched,
            })
        })
        .collect();
    let output = Output { controls };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and lists"))
}

/// The records in the board's own tree that `pattern` reaches through the
/// display path of a file it matches, which a link or mount can make differ
/// from the pattern, or through `applies_to`.
fn globbed(host: &dyn Host, pattern: &str) -> Result<BTreeSet<String>, PluginError> {
    let pattern = literal(pattern);
    let sql = format!(
        "WITH planned AS MATERIALIZED (SELECT full FROM {FILES} WHERE full GLOB {pattern}) \
         SELECT r.logical FROM records r, json_each(r.body, '$.paths') p \
         WHERE r.type IN {RULE_TYPES} AND r.source = 'worktree' \
         AND EXISTS (SELECT 1 FROM planned WHERE full GLOB p.value) \
         UNION SELECT r.logical FROM records r, json_each(r.body, '$.applies_to') a \
         WHERE r.type IN {RULE_TYPES} AND r.source = 'worktree' \
         AND substr(a.value, 2) GLOB {pattern}"
    );
    index::first_strings(host, &sql)
}

/// For each version control in `reached` whose count is known, how many
/// versions its capture finds in the files that both a path it matched and
/// the control reach. Each file is read once, however many controls reach
/// it. The count is unknown, and the control left out, when one of those
/// files cannot be read, such as one not written yet, or when a pattern
/// it matched matches no file yet, because a file written there could pin
/// a version.
fn versions_found(
    host: &dyn Host,
    reached: &BTreeMap<String, Vec<String>>,
    controls: &BTreeMap<String, Control>,
) -> Result<BTreeMap<String, usize>, PluginError> {
    let mut files: BTreeMap<&str, BTreeSet<LogicalPath>> = BTreeMap::new();
    let mut unknown: BTreeSet<&str> = BTreeSet::new();
    let mut empty: BTreeMap<&str, bool> = BTreeMap::new();
    for (logical, matched) in reached {
        let Some(Checker::Version(_)) =
            controls.get(logical).and_then(|each| each.checker.as_ref())
        else {
            continue;
        };
        let reaching = files.entry(logical.as_str()).or_default();
        for path in matched {
            if path.contains(['*', '?', '[']) {
                let pattern = path.strip_prefix('/').unwrap_or(path);
                if !empty.contains_key(pattern) {
                    empty.insert(pattern, matches_nothing(host, pattern)?);
                }
                if empty[pattern] {
                    unknown.insert(logical);
                }
                reaching.extend(matched_files(host, logical, pattern)?);
            } else if let Ok(file) = path.parse() {
                reaching.insert(file);
            } else {
                unknown.insert(logical);
            }
        }
    }
    let reads = read(host, files.values().flatten().collect())?;
    let mut found = BTreeMap::new();
    for (logical, reaching) in &files {
        let Some(Checker::Version(rule)) = &controls[*logical].checker else {
            continue;
        };
        let texts: Option<Vec<&Vec<u8>>> = (reaching.iter())
            .map(|file| reads.get(file)?.as_ref().ok())
            .collect();
        let (Some(texts), false) = (texts, unknown.contains(logical)) else {
            continue;
        };
        let expression = version::compile(logical, rule)?;
        let count = (texts.into_iter())
            .map(|data| version::found(&expression, &String::from_utf8_lossy(data)))
            .sum();
        found.insert((*logical).to_owned(), count);
    }
    Ok(found)
}

/// Whether `pattern` matches no file in the board, by display path.
fn matches_nothing(host: &dyn Host, pattern: &str) -> Result<bool, PluginError> {
    let sql = format!(
        "SELECT full FROM {FILES} WHERE full GLOB {} LIMIT 1",
        literal(pattern)
    );
    let first: Vec<String> = index::first_strings(host, &sql)?;
    Ok(first.is_empty())
}

/// The files `pattern` matches that one of `control`'s `paths` matches, by
/// display path, and those its `applies_to` names that `pattern` matches.
fn matched_files(
    host: &dyn Host,
    control: &str,
    pattern: &str,
) -> Result<Vec<LogicalPath>, PluginError> {
    let (control, pattern) = (literal(control), literal(pattern));
    let sql = format!(
        "WITH planned AS MATERIALIZED (SELECT logical, full FROM {FILES} \
         WHERE full GLOB {pattern} AND logical IS NOT NULL) \
         SELECT f.logical FROM records r, json_each(r.body, '$.paths') p, planned f \
         WHERE r.logical = {control} AND r.type IN {RULE_TYPES} \
         AND r.source = 'worktree' AND f.full GLOB p.value \
         UNION SELECT a.value FROM records r, json_each(r.body, '$.applies_to') a \
         WHERE r.logical = {control} AND r.type IN {RULE_TYPES} \
         AND r.source = 'worktree' AND substr(a.value, 2) GLOB {pattern}"
    );
    let found: Vec<String> = index::first_strings(host, &sql)?;
    Ok(found.iter().filter_map(|file| file.parse().ok()).collect())
}
