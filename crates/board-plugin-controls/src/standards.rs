//! `from-standards`: the controls a standards repository's rule tables give
//! a repository that adopts some of its profiles.

use std::collections::BTreeMap;

use board_model::LogicalPath;
use board_ops::{Host, PluginError, literal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::index;
use crate::logical;

/// The heading of the column that makes a table's rows rules a control
/// carries, compared without case.
const WHERE: &str = "where it applies";

/// The most words a control's summary holds.
const SUMMARY_WORDS: usize = 25;

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
    } = PluginError::read_input("controls", "from-standards", input)?;
    let [(map_display, map_text)] = read(host, &[map.as_str()])?
        .try_into()
        .expect("read answers each path it is asked once");
    let adopted: AdoptionMap = toml::from_str(&map_text).map_err(|error| PluginError {
        code: "controls::standards::map_invalid".to_owned(),
        message: format!(
            "{map} is not an adoption map with a `profiles` list of names: {error}; \
             check it against the standards' adoption.md"
        ),
        fields: Map::from_iter([("map".to_owned(), json!(map))]),
        hint: None,
    })?;
    let anchor = map_display.parent();
    let anchor = anchor.as_str().trim_start_matches('/');
    let files: Vec<String> = (adopted.profiles.iter())
        .map(|profile| format!("{}/profiles/{profile}.md", standards.trim_end_matches('/')))
        .collect();
    let asked: Vec<&str> = files.iter().map(String::as_str).collect();
    let texts = read(host, &asked)?;
    let mut generated: BTreeMap<String, Value> = BTreeMap::new();
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
            let record = json!({
                "$type": "controls/control/v1",
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
            generated.entry(path).or_insert(record);
        }
    }
    let held = held(host, &into)?;
    let mut lines: Vec<Value> = (generated.iter())
        .map(|(path, record)| {
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

/// Each of `paths` read through one `get_each`: its display path and text,
/// or `controls::standards::unreadable` for the first that cannot be read.
fn read(host: &dyn Host, paths: &[&str]) -> Result<Vec<(LogicalPath, String)>, PluginError> {
    let asked = (paths.iter())
        .map(|path| logical(path))
        .collect::<Result<Vec<_>, _>>()?;
    let answers = (host.get_each(&asked))
        .map_err(|error| PluginError::board_refused("controls", "get", &error.refusal("get")))?;
    (paths.iter().zip(answers))
        .map(|(path, answer)| match answer {
            Ok(report) => Ok((
                report.display,
                String::from_utf8_lossy(&report.bytes).into_owned(),
            )),
            Err(error) => Err(PluginError {
                code: "controls::standards::unreadable".to_owned(),
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
         AND type = 'controls/control/v1' AND substr(logical, 1, {}) = {}",
        prefix.chars().count(),
        literal(&prefix),
    );
    let report = index::rows(host, &sql)?;
    Ok((report.rows.into_iter())
        .filter_map(|row| match <[Value; 2]>::try_from(row) {
            Ok([Value::String(logical), Value::String(blob)]) => Some((logical, blob)),
            _ => None,
        })
        .collect())
}

/// The rows of every table in `text` whose header has an ID, a Rule and a
/// "Where it applies" column. A pattern is a cell's inline code; a rule
/// with none is `controls::standards::no_pattern`.
pub(crate) fn rules(file: &str, text: &str) -> Result<Vec<Row>, PluginError> {
    let mut rows = Vec::new();
    let mut columns: Option<(usize, usize, usize)> = None;
    for line in text.lines().map(str::trim) {
        if !line.starts_with('|') {
            columns = None;
            continue;
        }
        let cells = cells(line);
        let named = |name: &str| (cells.iter()).position(|cell| cell.eq_ignore_ascii_case(name));
        if let (Some(id), Some(rule), Some(place)) = (named("id"), named("rule"), named(WHERE)) {
            columns = Some((id, rule, place));
            continue;
        }
        let Some((id, rule, place)) = columns else {
            continue;
        };
        if cells
            .iter()
            .all(|cell| cell.chars().all(|c| c == '-' || c == ':'))
        {
            continue;
        }
        let cell = |at: usize| cells.get(at).cloned().unwrap_or_default();
        let patterns = code_spans(&cell(place));
        if patterns.is_empty() {
            return Err(PluginError {
                code: "controls::standards::no_pattern".to_owned(),
                message: format!(
                    "{} in {file} has no pattern in its Where it applies cell; give it one in \
                     inline code, such as `**` for every file",
                    cell(id)
                ),
                fields: Map::from_iter([
                    ("file".to_owned(), json!(file)),
                    ("rule".to_owned(), json!(cell(id))),
                ]),
                hint: None,
            });
        }
        rows.push(Row {
            id: cell(id),
            rule: cell(rule),
            patterns,
        });
    }
    Ok(rows)
}

/// The cells of a table row, trimmed, with `\|` read as `|`.
fn cells(line: &str) -> Vec<String> {
    let inner = line.strip_prefix('|').unwrap_or(line);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = vec![String::new()];
    let mut escaped = false;
    for c in inner.chars() {
        match (escaped, c) {
            (false, '\\') => escaped = true,
            (true, '|') => {
                push(&mut cells, '|');
                escaped = false;
            }
            (true, other) => {
                push(&mut cells, '\\');
                push(&mut cells, other);
                escaped = false;
            }
            (false, '|') => cells.push(String::new()),
            (false, other) => push(&mut cells, other),
        }
    }
    cells.iter().map(|cell| cell.trim().to_owned()).collect()
}

fn push(cells: &mut [String], c: char) {
    if let Some(last) = cells.last_mut() {
        last.push(c);
    }
}

/// The text of each inline code span in `cell`.
fn code_spans(cell: &str) -> Vec<String> {
    (cell.split('`').skip(1).step_by(2))
        .map(str::trim)
        .filter(|span| !span.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The rule's first sentence, outside inline code, on one line and cut to
/// 25 words, the last of them ending in `…` when it was cut.
pub(crate) fn summary(rule: &str) -> String {
    let mut code = false;
    let mut end = rule.len();
    let mut chars = rule.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '`' => code = !code,
            '.' | '!' | '?'
                if !code && chars.peek().is_none_or(|(_, next)| next.is_whitespace()) =>
            {
                end = at + c.len_utf8();
                break;
            }
            _ => {}
        }
    }
    let words: Vec<&str> = rule[..end].split_whitespace().collect();
    if words.len() <= SUMMARY_WORDS {
        return words.join(" ");
    }
    format!("{}…", words[..SUMMARY_WORDS].join(" "))
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
