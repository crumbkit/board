//! `check`: the controls that apply under a path, each budget measured
//! against the files it applies to.

use std::collections::{BTreeMap, BTreeSet};

use board_model::LogicalPath;
use board_ops::{Absent, Host, PluginError, RefsError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::index::{self, Control, File, Health};
use crate::logical;
use crate::records::{Budget, Measure, Per, Rule};

/// Check the controls that apply to a file at or under `path`, `/` when left out.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    /// The directory or file whose controls to check, by its logical path, such as /docs; / when left out.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: Option<String>,
}

/// The controls that apply under the path, by the control's path; each budget is met.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CheckOutput")]
pub(crate) struct Output {
    measured: Vec<Checked>,
    stated: Vec<Stated>,
}

/// A budget control and what its files use: all of them summed, or its
/// largest file when `per` is `file`, which only then carries `per`,
/// `largest` and `over`.
#[derive(Serialize, JsonSchema)]
pub(crate) struct Checked {
    control: String,
    requirement: String,
    budget: Measure,
    max: u64,
    used: u64,
    files: Vec<String>,
    #[serde(flatten)]
    each: Option<EachFile>,
}

/// What a per-file budget adds: the file it measured as `used`, null when
/// it has none, and every file over `max`.
#[derive(Serialize, JsonSchema)]
struct EachFile {
    per: Each,
    // `schema_of` takes null out of a field a flattened Option leaves out,
    // so this field states its null in a `oneOf`, which it keeps.
    #[schemars(schema_with = "path_or_null")]
    largest: Option<String>,
    over: Vec<FileUsed>,
}

/// `per` of a budget that limits each file on its own.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Each {
    File,
}

/// What one file uses of a per-file budget.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FileUsed {
    file: String,
    used: u64,
}

/// A control without a budget, its patterns, and how many of its files are
/// at or under the path.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Stated {
    control: String,
    requirement: String,
    purpose: String,
    paths: Vec<String>,
    files: usize,
}

fn path_or_null(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"oneOf": [{"type": "string"}, {"type": "null"}]})
}

/// The controls that apply to a file at or under the path asked, `/` when
/// left out.
///
/// Under `/`, every control applies, even one that reaches no file. Under
/// another path, only those reaching a file there do. A control with a
/// budget is measured over all its files, wherever they are, and must be
/// within it; any other, such as one with a pattern or judged rule, is
/// stated, with its patterns and the number of its files under the path. A path that names a link or mount is
/// checked where it leads. A control Board marks degraded is
/// `controls::control::degraded` under any path, since what it applies to
/// is unknown; a measured control over its budget is
/// `controls::budget::exceeded`. Each error names every control it applies
/// to. A file reached by a reference and a pattern, or through a link,
/// counts once, and each file is read once, however many controls reach it.
pub(crate) fn check(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path } = PluginError::read_input("controls", "check", input)?;
    let broken = index::degraded(host)?;
    if !broken.is_empty() {
        return Err(degraded(&broken));
    }
    let scope = scope(host, path.as_deref().unwrap_or("/"))?;
    let controls = index::controls(host)?;
    let named: Vec<&str> = (controls.iter())
        .filter(|(_, control)| control.applies_to.as_ref().is_some_and(|to| !to.is_empty()))
        .map(|(logical, _)| logical.as_str())
        .collect();
    let mut files = referenced(host, &named)?;
    for logical in controls.keys() {
        files.entry(logical.clone()).or_default();
    }
    let mut unreadable: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
    // Files with no display path, by control. They lie under no path but `/`.
    let mut unnamed: BTreeMap<String, usize> = BTreeMap::new();
    for found in index::matched(host)? {
        let Some(reached) = files.get_mut(&found.control) else {
            continue;
        };
        let shown = match found.file {
            File::Named(path) => {
                let shown = path.to_string();
                reached.insert(path);
                shown
            }
            File::Unnamed(path) => {
                *unnamed.entry(found.control.clone()).or_default() += 1;
                path
            }
        };
        if let Health::Degraded(code) = found.health {
            unreadable.entry(found.control).or_insert((shown, code));
        }
    }
    let root = scope == LogicalPath::root();
    // Under the root every control is listed, so one whose pattern reaches nothing shows with no files.
    let applying: Vec<&String> = (controls.keys())
        .filter(|logical| root || files[*logical].iter().any(|path| under(path, &scope)))
        .collect();
    let budgets: Vec<(&String, &Budget)> = (applying.iter())
        .filter_map(|logical| match &controls[*logical].rule {
            Some(Rule::Budget(budget)) => Some((*logical, budget)),
            _ => None,
        })
        .collect();
    if let Some((logical, (file, code))) = (budgets.iter())
        .find_map(|(logical, _)| unreadable.get(*logical).map(|found| (*logical, found)))
    {
        return Err(unreadable_file(logical, file, code.as_deref()));
    }
    let distinct: BTreeSet<&LogicalPath> = (budgets.iter())
        .flat_map(|(logical, _)| &files[*logical])
        .collect();
    let reads = read(host, distinct)?;
    let mut measured = Vec::new();
    for (logical, budget) in budgets {
        let control = &controls[logical];
        measured.push(checked(logical, control, budget, &files[logical], &reads)?);
    }
    let failing: Vec<&Checked> = (measured.iter())
        .filter(|item| item.used > item.max)
        .collect();
    if !failing.is_empty() {
        return Err(exceeded(&failing));
    }
    let stated = (applying.into_iter())
        .filter(|logical| !matches!(controls[*logical].rule, Some(Rule::Budget(_))))
        .map(|logical| {
            let control = &controls[logical];
            Stated {
                control: logical.clone(),
                requirement: control.requirement.clone(),
                purpose: control.purpose.clone(),
                paths: control.paths.clone().unwrap_or_default(),
                files: files[logical]
                    .iter()
                    .filter(|path| under(path, &scope))
                    .count()
                    + if root {
                        unnamed.get(logical).copied().unwrap_or(0)
                    } else {
                        0
                    },
            }
        })
        .collect();
    let output = Output { measured, stated };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and lists"))
}

/// The display path `path` resolves to, through any link or mount.
fn scope(host: &dyn Host, path: &str) -> Result<LogicalPath, PluginError> {
    if path == "/" {
        return Ok(LogicalPath::root());
    }
    let refused =
        |error: &RefsError| PluginError::board_refused("controls", "refs", &error.refusal());
    let answers = host
        .refs(&[logical(path)?], Absent::Refuse)
        .map_err(|e| refused(&e))?;
    let [answer] = <[_; 1]>::try_from(answers).expect("refs answers each path it is asked once");
    Ok(answer.map_err(|e| refused(&e))?.display)
}

/// For each of `controls`, the display path of each entry its `applies_to`
/// reaches, every link replaced by its target. One `refs` answers every
/// control, and no controls ask none.
fn referenced(
    host: &dyn Host,
    controls: &[&str],
) -> Result<BTreeMap<String, BTreeSet<LogicalPath>>, PluginError> {
    if controls.is_empty() {
        return Ok(BTreeMap::new());
    }
    let paths = (controls.iter())
        .map(|control| logical(control))
        .collect::<Result<Vec<LogicalPath>, PluginError>>()?;
    let refused =
        |error: &RefsError| PluginError::board_refused("controls", "refs", &error.refusal());
    let answers = host.refs(&paths, Absent::Refuse).map_err(|e| refused(&e))?;
    let mut found = BTreeMap::new();
    for (control, answer) in controls.iter().zip(answers) {
        let report = answer.map_err(|e| refused(&e))?;
        let reached = (report.from.iter())
            .filter(|outgoing| outgoing.pointer.starts_with("/applies_to/"))
            .filter_map(|outgoing| {
                let location = outgoing.location.as_ref()?;
                Some(
                    location
                        .display_path()
                        .unwrap_or_else(|| outgoing.target.clone()),
                )
            })
            .collect();
        found.insert((*control).to_owned(), reached);
    }
    Ok(found)
}

/// Each of `paths`, read through one `get_each`: its bytes, or Board's code
/// for why it cannot be read.
fn read(
    host: &dyn Host,
    paths: BTreeSet<&LogicalPath>,
) -> Result<BTreeMap<LogicalPath, Result<Vec<u8>, String>>, PluginError> {
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }
    let asked: Vec<LogicalPath> = paths.into_iter().cloned().collect();
    let answers = (host.get_each(&asked))
        .map_err(|error| PluginError::board_refused("controls", "get", &error.refusal("get")))?;
    Ok((asked.into_iter().zip(answers))
        .map(|(path, answer)| {
            let read = answer
                .map(|report| report.bytes)
                .map_err(|error| error.code().as_str().to_owned());
            (path, read)
        })
        .collect())
}

/// Whether `path` is `scope` or lies beneath it.
fn under(path: &LogicalPath, scope: &LogicalPath) -> bool {
    let mut segments = path.segments();
    scope
        .segments()
        .all(|segment| segments.next() == Some(segment))
}

/// The words or bytes in `data`. Words are separated by whitespace as
/// Python's `str.split` finds it: Unicode whitespace and the four ASCII
/// separators U+001C to U+001F, which Rust's `char::is_whitespace` leaves out.
pub(crate) fn measure(data: &[u8], budget: Measure) -> u64 {
    let count = match budget {
        Measure::Bytes => data.len(),
        Measure::Words => String::from_utf8_lossy(data)
            .split(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
            .filter(|word| !word.is_empty())
            .count(),
    };
    u64::try_from(count).expect("a count of bytes in memory fits in u64")
}

fn checked(
    logical: &str,
    control: &Control,
    &Budget { budget, max, per }: &Budget,
    files: &BTreeSet<LogicalPath>,
    reads: &BTreeMap<LogicalPath, Result<Vec<u8>, String>>,
) -> Result<Checked, PluginError> {
    let mut usage = Vec::new();
    for path in files {
        match &reads[path] {
            Ok(data) => usage.push((path.to_string(), measure(data, budget))),
            Err(code) => return Err(unreadable_file(logical, path.as_str(), Some(code))),
        }
    }
    let mut checked = Checked {
        control: logical.to_owned(),
        requirement: control.requirement.clone(),
        budget,
        max,
        used: usage.iter().map(|(_, used)| used).sum(),
        files: files.iter().map(ToString::to_string).collect(),
        each: None,
    };
    if per.unwrap_or(Per::Total) == Per::Total {
        return Ok(checked);
    }
    // The first of the sorted files with the most usage, so ties name the same file every run.
    let most = usage.iter().map(|(_, used)| *used).max();
    let largest = (usage.iter())
        .find(|(_, used)| Some(*used) == most)
        .map(|(path, _)| path.clone());
    let over = (usage.iter())
        .filter(|(_, used)| *used > max)
        .map(|(path, used)| FileUsed {
            file: path.clone(),
            used: *used,
        })
        .collect();
    checked.used = most.unwrap_or(0);
    checked.each = Some(EachFile {
        per: Each::File,
        largest,
        over,
    });
    Ok(checked)
}

fn degraded(broken: &[(String, Value)]) -> PluginError {
    let names: Vec<String> = (broken.iter())
        .map(|(logical, code)| format!("{logical} ({})", code.as_str().unwrap_or("None")))
        .collect();
    let controls: Vec<Value> = (broken.iter())
        .map(|(logical, code)| json!({"control": logical, "code": code}))
        .collect();
    PluginError {
        code: "controls::control::degraded".to_owned(),
        message: format!(
            "controls Board cannot use: {}; run `board check` for why",
            names.join(", ")
        ),
        fields: Map::from_iter([("controls".to_owned(), Value::from(controls))]),
    }
}

/// How a failing control is over its budget, naming each file when it
/// limits files one by one.
fn over(item: &Checked) -> String {
    let budget = serde_json::to_value(item.budget).expect("a measure is a string");
    let budget = budget.as_str().unwrap_or_default();
    match &item.each {
        None => format!("{} ({} of {} {budget})", item.control, item.used, item.max),
        Some(each) => {
            let files: Vec<String> = (each.over.iter())
                .map(|over| format!("{} has {}", over.file, over.used))
                .collect();
            format!(
                "{} (each file at most {} {budget}: {})",
                item.control,
                item.max,
                files.join(", ")
            )
        }
    }
}

fn exceeded(failing: &[&Checked]) -> PluginError {
    let names: Vec<String> = failing.iter().map(|item| over(item)).collect();
    let controls = serde_json::to_value(failing).expect("Checked holds only strings and numbers");
    PluginError {
        code: "controls::budget::exceeded".to_owned(),
        message: format!(
            "over budget: {}; shorten the files, or raise the control's max in a change that says why",
            names.join(", ")
        ),
        fields: Map::from_iter([("controls".to_owned(), controls)]),
    }
}

fn unreadable_file(control: &str, path: &str, code: Option<&str>) -> PluginError {
    let shown = code.unwrap_or("None");
    PluginError {
        code: "controls::file::unreadable".to_owned(),
        message: format!(
            "{control} applies to {path}, which Board cannot read as a file ({shown}); run `board check` \
             for why, or change the control to name only files"
        ),
        fields: Map::from_iter([
            ("control".to_owned(), Value::from(control)),
            ("file".to_owned(), Value::from(path)),
            ("code".to_owned(), Value::from(code)),
        ]),
    }
}

#[cfg(test)]
pub(crate) mod tests;
