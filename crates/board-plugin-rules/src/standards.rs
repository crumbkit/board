//! `from-standards`: the controls a standards repository's rule tables give
//! a repository that adopts some of its profiles.

use std::collections::BTreeMap;

use board_model::LogicalPath;
use board_ops::{Host, PluginError, literal};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::index;
use crate::logical;
use crate::records::{CONTROL_TYPE, RULE_TYPES};
use crate::summary::summary;

/// The heading of the column that makes a table's rows rules a control
/// carries, compared without case.
const WHERE: &str = "where it applies";

/// The standards, the adopter's map and where the controls go.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    /// The standards repository, by the logical path of its mount, such as /repos/crumbkit-rust-standards; its profiles are `profiles/<name>.md`.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    standards: String,
    /// The adopter's map, a TOML file whose `profiles` lists the profiles it adopts, such as /repos/inbox/rust-standards.toml; patterns are anchored at its directory.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    map: String,
    /// The directory in the board's own tree that holds the controls, such as /controls/inbox/standards.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    into: String,
}

/// The profiles read with the rules each gave, and the `board put --batch`
/// lines that make `into` hold exactly the generated controls.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "FromStandardsOutput")]
pub(crate) struct Output {
    profiles: Vec<Profile>,
    #[schemars(with = "Vec<Map<String, Value>>")]
    lines: Vec<Value>,
}

/// One adopted profile, the file it was read from and how many rules it
/// gave.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Profile {
    name: String,
    file: String,
    rules: usize,
}

/// One rule of a table with a "Where it applies" column.
#[derive(Debug)]
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) rule: String,
    pub(crate) patterns: Vec<String>,
}

/// The adopter's map, as far as this capability reads it.
#[derive(Deserialize)]
struct AdoptionMap {
    profiles: Vec<String>,
}

/// Reads the map and each profile it adopts, and returns a write line for
/// each rule and a remove line for each control under `into` that no rule
/// gives any more. It writes nothing.
pub(crate) fn from_standards(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        standards,
        map,
        into,
    } = PluginError::read_input("rules", "from-standards", input)?;
    let [(map_display, map_text)] = read(host, &[map.as_str()])?
        .try_into()
        .expect("read answers each path it is asked once");
    let adopted: AdoptionMap = toml::from_str(&map_text).map_err(|error| PluginError {
        code: "rules::standards::map_invalid".to_owned(),
        message: format!(
            "{map} is not an adoption map with a `profiles` list of names: {error}; \
             check it against the standards' adoption.md"
        ),
        fields: Map::from_iter([("map".to_owned(), json!(map))]),
        hint: None,
    })?;
    if let Some(twice) = (adopted.profiles.iter())
        .enumerate()
        .find_map(|(at, profile)| adopted.profiles[..at].contains(profile).then_some(profile))
    {
        let message = format!(
            "{map} lists the profile {twice} twice, so each of its rules would be written twice; \
             list it once"
        );
        return Err(duplicate(message, ("profile", twice), &[map.as_str()]));
    }
    let anchor = map_display.parent();
    let anchor = anchor.as_str().trim_start_matches('/');
    let files: Vec<String> = (adopted.profiles.iter())
        .map(|profile| format!("{}/profiles/{profile}.md", standards.trim_end_matches('/')))
        .collect();
    let asked: Vec<&str> = files.iter().map(String::as_str).collect();
    let texts = read(host, &asked)?;
    let mut generated: BTreeMap<String, (Value, &str)> = BTreeMap::new();
    let mut profiles = Vec::new();
    for ((profile, file), (_, text)) in adopted.profiles.iter().zip(&files).zip(&texts) {
        let rows = rules(file, text)?;
        profiles.push(Profile {
            name: profile.clone(),
            file: file.clone(),
            rules: rows.len(),
        });
        for row in rows {
            let paths: Vec<String> = (row.patterns.iter())
                .flat_map(|pattern| globs(anchor, pattern))
                .collect();
            // The type before the rename, which every board holds the
            // schema of until it migrates.
            let record = json!({
                "$type": CONTROL_TYPE,
                "requirement": row.rule,
                "purpose": format!(
                    "{} is a rule of the {profile} profile of the standards at {standards}, \
                     which {map} adopts.",
                    row.id
                ),
                "summary": summary(&row.rule),
                "paths": paths,
                "sources": [file],
            });
            let path = format!("{}/{}", into.trim_end_matches('/'), row.id.to_lowercase());
            if let Some((_, first)) = generated.get(&path) {
                let message = if *first == file.as_str() {
                    format!(
                        "{file} gives the rule ID {} twice, ignoring case, so both rules would write \
                         {path}; give one of the two rules another ID",
                        row.id
                    )
                } else {
                    format!(
                        "{first} and {file} both give the rule ID {}, ignoring case, so both rules \
                         would write {path}; give one of the two rules another ID",
                        row.id
                    )
                };
                return Err(duplicate(message, ("rule", &row.id), &[first, file]));
            }
            generated.insert(path, (record, file));
        }
    }
    let held = held(host, &into)?;
    let mut lines: Vec<Value> = (generated.iter())
        .map(|(path, (record, _))| {
            let expect = held.get(path).map_or("none", String::as_str);
            json!({"path": path, "expect": expect, "record": record})
        })
        .collect();
    lines.extend(
        (held.iter())
            .filter(|(path, _)| !generated.contains_key(*path))
            .map(|(path, blob)| json!({"path": path, "expect": blob, "remove": true})),
    );
    let output = Output { profiles, lines };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and lists"))
}

/// `rules::standards::duplicate_rule` with `message`, naming what
/// repeats, as `(field, value)` such as `("rule", "BASE-31")`, and the
/// `files` that repeat it.
fn duplicate(message: String, (field, value): (&str, &str), files: &[&str]) -> PluginError {
    PluginError {
        code: "rules::standards::duplicate_rule".to_owned(),
        message,
        fields: Map::from_iter([
            (field.to_owned(), json!(value)),
            ("files".to_owned(), json!(files)),
        ]),
        hint: None,
    }
}

/// Each of `paths` read through one `get_each`: its display path and text,
/// or `rules::standards::unreadable` for the first that cannot be read.
fn read(host: &dyn Host, paths: &[&str]) -> Result<Vec<(LogicalPath, String)>, PluginError> {
    let asked = (paths.iter())
        .map(|path| logical(path))
        .collect::<Result<Vec<_>, _>>()?;
    let answers = (host.get_each(&asked))
        .map_err(|error| PluginError::board_refused("rules", "get", &error.refusal("get")))?;
    (paths.iter().zip(answers))
        .map(|(path, answer)| match answer {
            Ok(report) => Ok((
                report.display,
                String::from_utf8_lossy(&report.bytes).into_owned(),
            )),
            Err(error) => Err(PluginError {
                code: "rules::standards::unreadable".to_owned(),
                message: format!(
                    "{path} cannot be read ({}); check the standards mount and the profile names \
                     the map adopts with `board ls`",
                    error.code().as_str()
                ),
                fields: Map::from_iter([
                    ("path".to_owned(), json!(path)),
                    ("reason".to_owned(), json!(error.code().as_str())),
                ]),
                hint: None,
            }),
        })
        .collect()
}

/// Each control under `into` in the board's own tree, by logical path, with
/// its blob.
fn held(host: &dyn Host, into: &str) -> Result<BTreeMap<String, String>, PluginError> {
    let prefix = format!("{}/", into.trim_end_matches('/'));
    let sql = format!(
        "SELECT logical, blob FROM entries WHERE source = 'worktree' \
         AND type IN {RULE_TYPES} AND substr(logical, 1, {}) = {}",
        prefix.chars().count(),
        literal(&prefix),
    );
    index::string_pairs(host, &sql)
}

/// The rows of every table in `text` whose header has an ID, a Rule and a
/// "Where it applies" column, read as `CommonMark` with GitHub's tables. A
/// pattern is a span of inline code in the Where it applies cell. A rule
/// with none is `rules::standards::no_pattern`, one whose Rule cell
/// is empty is `rules::standards::empty_rule`, and one whose ID is not a
/// record name is `rules::standards::bad_id`.
pub(crate) fn rules(file: &str, text: &str) -> Result<Vec<Row>, PluginError> {
    let mut rows = Vec::new();
    for table in tables(text) {
        let Some((header, body)) = table.split_first() else {
            continue;
        };
        let named =
            |name: &str| (header.iter()).position(|cell| cell.text.eq_ignore_ascii_case(name));
        let (Some(id), Some(rule), Some(place)) = (named("id"), named("rule"), named(WHERE)) else {
            continue;
        };
        for cells in body {
            let cell = |at: usize| cells.get(at).cloned().unwrap_or_default();
            let id = cell(id).text.trim_matches('`').trim().to_owned();
            let (rule, place) = (cell(rule).text, cell(place).code);
            let refused = |code: &str, message: String| PluginError {
                code: code.to_owned(),
                message,
                fields: Map::from_iter([
                    ("file".to_owned(), json!(file)),
                    ("rule".to_owned(), json!(id)),
                ]),
                hint: None,
            };
            if !record_name(&id) {
                return Err(refused(
                    "rules::standards::bad_id",
                    format!(
                        "the rule ID {id:?} in {file} cannot name a control's record; give the rule \
                         an ID of letters, digits, `-` and `_`, such as BASE-31"
                    ),
                ));
            }
            if rule.is_empty() {
                return Err(refused(
                    "rules::standards::empty_rule",
                    format!("{id} in {file} has an empty Rule cell; write the rule there"),
                ));
            }
            if place.is_empty() {
                return Err(refused(
                    "rules::standards::no_pattern",
                    format!(
                        "{id} in {file} has no pattern in its Where it applies cell; give it one in \
                         inline code, such as `**` for every file"
                    ),
                ));
            }
            rows.push(Row {
                id,
                rule,
                patterns: place,
            });
        }
    }
    Ok(rows)
}

/// Whether `id` can name a record: one or more ASCII letters, digits, `-`
/// and `_`.
fn record_name(id: &str) -> bool {
    !id.is_empty() && (id.bytes()).all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}

/// One table cell: its text, with inline code kept in backticks, and the
/// text of each of its inline code spans.
#[derive(Clone, Default)]
struct Cell {
    text: String,
    code: Vec<String>,
}

/// Each table in `text`, as its rows of cells, the header row first, with
/// each cell's text on one line.
fn tables(text: &str) -> Vec<Vec<Vec<Cell>>> {
    let mut tables: Vec<Vec<Vec<Cell>>> = Vec::new();
    let mut inside = false;
    for event in Parser::new_ext(text, Options::ENABLE_TABLES) {
        let row = tables.last_mut().and_then(|table| table.last_mut());
        match event {
            Event::Start(Tag::Table(_)) => tables.push(Vec::new()),
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                if let Some(table) = tables.last_mut() {
                    table.push(Vec::new());
                }
            }
            Event::Start(Tag::TableCell) => {
                inside = true;
                if let Some(row) = row {
                    row.push(Cell::default());
                }
            }
            Event::End(TagEnd::TableCell) => inside = false,
            event if inside => {
                if let Some(cell) = row.and_then(|row| row.last_mut()) {
                    take(cell, &event);
                }
            }
            _ => {}
        }
    }
    for cell in tables.iter_mut().flatten().flatten() {
        cell.text = cell.text.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    tables
}

/// Adds what `event`, inside a table cell, holds to `cell`: text as it
/// reads, and inline code in as many backticks as it needs.
fn take(cell: &mut Cell, event: &Event<'_>) {
    match event {
        Event::Text(text) => cell.text.push_str(text),
        Event::Code(code) => {
            let fence = if code.contains('`') { "`` " } else { "`" };
            cell.text.push_str(fence);
            cell.text.push_str(code);
            cell.text.extend(fence.chars().rev());
            let code = code.trim();
            if !code.is_empty() {
                cell.code.push(code.to_owned());
            }
        }
        Event::SoftBreak | Event::HardBreak | Event::InlineHtml(_) => {
            cell.text.push(' ');
        }
        _ => {}
    }
}

/// The `SQLite` GLOBs that hold `pattern`, a glob relative to the repository
/// at `anchor`, such as `**/*.rs`. Each `**/` may also match no directory,
/// so it becomes both nothing and `*/`. GLOB's `*` matches `/` too, so a
/// GLOB can match more files than the pattern meant, never fewer.
pub(crate) fn globs(anchor: &str, pattern: &str) -> Vec<String> {
    let mut found = vec![String::new()];
    let mut rest = pattern;
    while let Some(at) = rest.find("**/") {
        let (before, after) = (&rest[..at], &rest[at + 3..]);
        found = (found.iter())
            .flat_map(|done| [format!("{done}{before}"), format!("{done}{before}*/")])
            .collect();
        rest = after;
    }
    let prefix = if anchor.is_empty() {
        String::new()
    } else {
        format!("{anchor}/")
    };
    let mut globs: Vec<String> = found
        .iter()
        .map(|done| format!("{prefix}{done}{rest}"))
        .collect();
    globs.dedup();
    globs
}

#[cfg(test)]
mod tests;
