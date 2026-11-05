//! `check`: the controls that apply under a path, each budget and version
//! rule measured against the files it applies to.

use std::collections::{BTreeMap, BTreeSet};

use board_model::LogicalPath;
use board_ops::{Absent, Host, PluginError, RefsError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::index::{self, Control, File, Health};
use crate::logical;
use crate::records::{Budget, Kind, Measure, Per, Rule, ShowAt, Version};
use crate::version;

/// Check the controls that apply to a file at or under `path`, `/` when left out.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    /// The directory or file whose controls to check, by its logical path, such as /docs; / when left out.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: Option<String>,
}

/// The controls that apply under the path, by the control's path, each
/// budget and version rule measured over the files under the path.
/// `passed` is false when a budget is over its max, a file pins a version
/// older than a rule's, or a file is unreadable.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CheckOutput")]
pub(crate) struct Output {
    passed: bool,
    measured: Vec<Checked>,
    versions: Vec<Pinned>,
    stated: Vec<Stated>,
}

/// A version control, the files it reads under the path, each version
/// they pin that is older than `at_least`, and each file it could not read.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pinned {
    control: String,
    requirement: String,
    at_least: String,
    files: Vec<String>,
    behind: Vec<FileBehind>,
    unreadable: Vec<Unreadable>,
    kind: Kind,
    show: ShowAt,
}

/// A version a file pins that is older than the rule's, and its line.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FileBehind {
    file: String,
    line: usize,
    found: String,
}

/// A budget control and what its files under the path use: all of them
/// summed, or its largest file when `per` is `file`, which only then
/// carries `per`, `largest` and `over`. `unreadable` lists each file it
/// could not read, which counts nothing toward `used`.
#[derive(Serialize, JsonSchema)]
pub(crate) struct Checked {
    control: String,
    requirement: String,
    budget: Measure,
    max: u64,
    used: u64,
    files: Vec<String>,
    unreadable: Vec<Unreadable>,
    kind: Kind,
    show: ShowAt,
    #[serde(flatten)]
    each: Option<EachFile>,
}

/// A file a measured control reaches that Board cannot read, with Board's
/// code for why, null when it gives none.
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Unreadable {
    file: String,
    code: Option<String>,
}

impl Checked {
    /// Whether the budget is met and every file was read.
    fn passed(&self) -> bool {
        self.used <= self.max && self.unreadable.is_empty()
    }
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
    kind: Kind,
    show: ShowAt,
}

fn path_or_null(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"oneOf": [{"type": "string"}, {"type": "null"}]})
}

/// The controls that apply to a file at or under the path asked, `/` when
/// left out.
///
/// Under `/`, every control applies, even one that reaches no file. Under
/// another path, only those reaching a file there do. A control with a
/// budget or version rule is measured over its files under the path alone,
/// so another repository's files never decide one repository's check; any
/// other, such as one with a pattern or judged rule, is stated, with its
/// patterns and the number of its files under the path. A path that names
/// a link or mount is checked where it leads. A budget over its max, a pin
/// older than a version rule's and a file Board cannot read are reported in
/// the output, and `passed` is false; none fails the call. A control Board
/// marks degraded is `controls::control::degraded` under any path, since
/// what it applies to is unknown. A file reached by a reference and a
/// pattern, or through a link, counts once, and each file is read once,
/// however many controls reach it.
pub(crate) fn check(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path } = PluginError::read_input("controls", "check", input)?;
    let broken = index::degraded(host)?;
    if !broken.is_empty() {
        return Err(degraded(&broken));
    }
    let scope = scope(host, path.as_deref().unwrap_or("/"))?;
    let root = scope == LogicalPath::root();
    let controls = index::controls(host)?;
    let named: Vec<&str> = (controls.iter())
        .filter(|(_, control)| control.applies_to.as_ref().is_some_and(|to| !to.is_empty()))
        .map(|(logical, _)| logical.as_str())
        .collect();
    let mut files = referenced(host, &named)?;
    for logical in controls.keys() {
        files.entry(logical.clone()).or_default();
    }
    // The files Board's index marks degraded, by control, under the path.
    let mut unreadable: BTreeMap<String, Vec<Unreadable>> = BTreeMap::new();
    // Files with no display path, by control. They lie under no path but `/`.
    let mut unnamed: BTreeMap<String, usize> = BTreeMap::new();
    for found in index::matched(host)? {
        let Some(reached) = files.get_mut(&found.control) else {
            continue;
        };
        let (shown, inside) = match found.file {
            File::Named(path) => {
                let inside = under(&path, &scope);
                let shown = path.to_string();
                reached.insert(path);
                (shown, inside)
            }
            File::Unnamed(path) => {
                *unnamed.entry(found.control.clone()).or_default() += 1;
                (path, root)
            }
        };
        if let (Health::Degraded(code), true) = (found.health, inside) {
            (unreadable.entry(found.control).or_default()).push(Unreadable { file: shown, code });
        }
    }
    for reached in files.values_mut() {
        reached.retain(|path| under(path, &scope));
    }
    // Under the root every control is listed, so one whose pattern reaches nothing shows with no files.
    let applying: Vec<&String> = (controls.keys())
        .filter(|logical| root || !files[*logical].is_empty())
        .collect();
    let (measured_controls, versions) =
        measure_files(host, &applying, &controls, &files, &unreadable)?;
    let stated = (applying.into_iter())
        .filter(|logical| !measured(&controls[*logical]))
        .map(|logical| {
            let control = &controls[logical];
            Stated {
                control: logical.clone(),
                requirement: control.requirement.clone(),
                purpose: control.purpose.clone(),
                kind: control.kind,
                show: control.show(),
                paths: control.paths.clone().unwrap_or_default(),
                files: files[logical].len()
                    + if root {
                        unnamed.get(logical).copied().unwrap_or(0)
                    } else {
                        0
                    },
            }
        })
        .collect();
    let passed = (measured_controls.iter()).all(Checked::passed)
        && (versions.iter()).all(|item| item.behind.is_empty() && item.unreadable.is_empty());
    let output = Output {
        passed,
        measured: measured_controls,
        versions,
        stated,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and lists"))
}

/// Each budget control and each version control among `applying`,
/// measured over its files under the path, which are each read once. A
/// file the index marks degraded is not read; it and each file Board
/// cannot read go in the control's `unreadable`.
fn measure_files(
    host: &dyn Host,
    applying: &[&String],
    controls: &BTreeMap<String, Control>,
    files: &BTreeMap<String, BTreeSet<LogicalPath>>,
    unreadable: &BTreeMap<String, Vec<Unreadable>>,
) -> Result<(Vec<Checked>, Vec<Pinned>), PluginError> {
    let read_by: Vec<&String> = (applying.iter().copied())
        .filter(|logical| measured(&controls[*logical]))
        .collect();
    // A degraded file is degraded for every control that reaches it.
    let skipped: BTreeMap<&str, &Unreadable> = (read_by.iter())
        .filter_map(|logical| unreadable.get(*logical))
        .flatten()
        .map(|item| (item.file.as_str(), item))
        .collect();
    let distinct: BTreeSet<&LogicalPath> = (read_by.iter())
        .flat_map(|logical| &files[*logical])
        .filter(|path| !skipped.contains_key(path.as_str()))
        .collect();
    let reads = read(host, distinct)?;
    let (mut budgets, mut versions) = (Vec::new(), Vec::new());
    for logical in read_by {
        let (control, reached) = (&controls[logical], &files[logical]);
        let marked = unreadable.get(logical).map_or(&[][..], Vec::as_slice);
        let (texts, failed) = texts(reached, marked, &skipped, &reads);
        match &control.rule {
            Some(Rule::Budget(budget)) => {
                budgets.push(checked(logical, control, budget, &texts, failed));
            }
            Some(Rule::Version(rule)) => {
                versions.push(pinned(logical, control, rule, &texts, failed)?);
            }
            _ => {}
        }
    }
    Ok((budgets, versions))
}

/// The bytes of each of `reached` that was read, and each file that was
/// not: those `marked` degraded for this control, those `skipped` as
/// degraded for any, and those whose read failed, by file.
fn texts<'a>(
    reached: &'a BTreeSet<LogicalPath>,
    marked: &[Unreadable],
    skipped: &BTreeMap<&str, &Unreadable>,
    reads: &'a BTreeMap<LogicalPath, Result<Vec<u8>, String>>,
) -> (Vec<(&'a LogicalPath, &'a [u8])>, Vec<Unreadable>) {
    let mut failed = marked.to_vec();
    let mut texts = Vec::new();
    for path in reached {
        match reads.get(path) {
            Some(Ok(data)) => texts.push((path, data.as_slice())),
            Some(Err(code)) => failed.push(Unreadable {
                file: path.to_string(),
                code: Some(code.clone()),
            }),
            None => failed.extend(skipped.get(path.as_str()).map(|item| (*item).clone())),
        }
    }
    failed.sort_by(|a, b| a.file.cmp(&b.file));
    failed.dedup_by(|a, b| a.file == b.file);
    (texts, failed)
}

/// Whether `check` measures the control's files: it has a budget or a
/// version rule.
fn measured(control: &Control) -> bool {
    matches!(control.rule, Some(Rule::Budget(_) | Rule::Version(_)))
}

/// The display path `path` resolves to, through any link or mount.
fn scope(host: &dyn Host, path: &str) -> Result<LogicalPath, PluginError> {
    if path == "/" {
        return Ok(LogicalPath::root());
    }
    let refused =
        |error: &RefsError| PluginError::board_refused("controls", "refs", &error.refusal());
    let answers = host
        .refs(&[logical(path)?], Absent::Refuse, &[])
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
    let answers = host
        .refs(&paths, Absent::Refuse, &[])
        .map_err(|e| refused(&e))?;
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
    texts: &[(&LogicalPath, &[u8])],
    unreadable: Vec<Unreadable>,
) -> Checked {
    let usage: Vec<(String, u64)> = (texts.iter())
        .map(|(path, data)| (path.to_string(), measure(data, budget)))
        .collect();
    let mut checked = Checked {
        control: logical.to_owned(),
        requirement: control.requirement.clone(),
        budget,
        max,
        used: usage.iter().map(|(_, used)| used).sum(),
        files: listed(texts, &unreadable),
        unreadable,
        kind: control.kind,
        show: control.show(),
        each: None,
    };
    if per.unwrap_or(Per::Total) == Per::Total {
        return checked;
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
    checked
}

/// The display paths of the files a control reaches under the path, read
/// or not, sorted; a degraded file with no display path is listed by its
/// path in the tree.
fn listed(texts: &[(&LogicalPath, &[u8])], unreadable: &[Unreadable]) -> Vec<String> {
    let all: BTreeSet<String> = (texts.iter().map(|(path, _)| path.to_string()))
        .chain(unreadable.iter().map(|item| item.file.clone()))
        .collect();
    all.into_iter().collect()
}

/// A version control's verdict on its files: each version they pin that is
/// older than the rule's, in file order and then line order.
fn pinned(
    logical: &str,
    control: &Control,
    rule: &Version,
    texts: &[(&LogicalPath, &[u8])],
    unreadable: Vec<Unreadable>,
) -> Result<Pinned, PluginError> {
    let expression = version::compile(logical, rule)?;
    let mut behind = Vec::new();
    for (path, data) in texts {
        let text = String::from_utf8_lossy(data);
        let found = version::behind(&expression, &rule.at_least, &text);
        behind.extend(found.into_iter().map(|old| FileBehind {
            file: path.to_string(),
            line: old.line,
            found: old.found,
        }));
    }
    Ok(Pinned {
        control: logical.to_owned(),
        requirement: control.requirement.clone(),
        at_least: rule.at_least.clone(),
        files: listed(texts, &unreadable),
        behind,
        unreadable,
        kind: control.kind,
        show: control.show(),
    })
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
        hint: None,
    }
}

#[cfg(test)]
pub(crate) mod tests;
