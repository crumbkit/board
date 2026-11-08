//! `migrate --plans`: moves each open change's `Considered` and `Obligations
//! left owed` sections from its plan's text to the plan's fields.
//!
//! For each change not done or dropped whose current plan's text has either
//! section, it writes a new plan snapshot: the same uses, paths and attached
//! records, the sections cut from the text, and their lines in `considered`
//! and `owed`. A line it cannot read as paths and a reason, a path that
//! reaches nothing, or an owed path that is no control is listed under
//! `unread`, and that change keeps its old plan.
//!
//! The parser reads the forms the sections have held, and no others: list
//! items that start with paths and a reason, list items of a reason and then
//! only paths, and a prose line ending in a colon followed by lines of only
//! paths. Anything else is `unread`, which a person rewrites by hand.

use std::sync::LazyLock;

use board_ops::{Host, PluginError};
use board_plugin::shape::Record;
use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use super::flows::stored;
use super::{LISTED, compiled, conforms};
use crate::board;
use crate::change::{plan_at, referenced};
use crate::gates::{FENCES, HEADING};
use crate::plan::{MOVED_HEADINGS, Planned, plan_lines};
use crate::records::{CONTROL_TYPE, ChangeRecord, ChangeV1, Considered, Owed, is_final};

/// The code of a change the migration leaves as it is.
const UNREAD: &str = "changes::migrate::unread";

/// A list item's marker, and what follows it.
static ITEM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:[-*+]|\d+[.)])\s+(.*)$")
        .expect("the pattern is a constant; the migrate tests compile it")
});
/// A board path at the start of some text, ended by a space, a quote, a
/// bracket or a list's punctuation.
static PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^/[^\s`'")>\],;]+"#)
        .expect("the pattern is a constant; the migrate tests compile it")
});

/// What moving the sections wrote, or would write.
///
/// `changes` lists the changes given a new snapshot, at most 100, with
/// `total` and `truncated`. `considered` and `owed` count the entries the
/// snapshots hold. `unread` lists each line that kept its change's old plan.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "MigratePlansOutput")]
pub(super) struct Output {
    applied: bool,
    changes: Vec<String>,
    total: usize,
    truncated: bool,
    considered: usize,
    owed: usize,
    unread: Vec<Unread>,
}

impl Output {
    /// The output, saying whether it was written.
    pub(super) fn applied(mut self, applied: bool) -> Self {
        self.applied = applied;
        self
    }
}

/// A line that kept its change's old plan, and why.
#[derive(Debug, Serialize, JsonSchema)]
struct Unread {
    path: String,
    code: &'static str,
    line: String,
    why: String,
}

/// A plan's text with the two sections cut, and what each held.
#[derive(Debug, Default, PartialEq, Eq)]
struct Cut {
    kept: String,
    considered: Vec<String>,
    owed: Vec<String>,
    found: bool,
}

/// `text` with each section named in `MOVED_HEADINGS` cut. A section runs to
/// the next heading of its level or higher. A line inside a code fence is
/// never a heading.
fn cut(text: &str) -> Cut {
    let mut result = Cut::default();
    let mut kept: Vec<&str> = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    let mut fenced = false;
    for line in text.lines() {
        let heading = if fenced {
            None
        } else {
            HEADING.captures(line).and_then(|captures| captures.get(1))
        };
        if FENCES
            .iter()
            .any(|fence| line.trim_start().starts_with(fence))
        {
            fenced = !fenced;
        }
        if let Some(heading) = heading {
            let level = line.len() - line.trim_start_matches('#').len();
            if open.is_some_and(|(at, _)| level <= at) {
                open = None;
            }
            let name = heading.as_str().to_lowercase();
            if open.is_none()
                && let Some(which) = MOVED_HEADINGS.iter().position(|each| *each == name)
            {
                open = Some((level, which));
                result.found = true;
                continue;
            }
        }
        match open {
            Some((_, 0)) => result.considered.push(line.to_owned()),
            Some(_) => result.owed.push(line.to_owned()),
            None => kept.push(line),
        }
    }
    let mut tidy: Vec<&str> = Vec::new();
    for line in kept {
        if !(line.trim().is_empty() && tidy.last().is_some_and(|last| last.trim().is_empty())) {
            tidy.push(line);
        }
    }
    while tidy.last().is_some_and(|last| last.trim().is_empty()) {
        tidy.pop();
    }
    result.kept = format!("{}\n", tidy.join("\n"));
    result
}

/// The paths `text` starts with, separated by commas, `and` or spaces, in
/// backticks or not, and what follows them.
fn leading(text: &str) -> (Vec<String>, &str) {
    let mut paths = Vec::new();
    let mut rest = text;
    loop {
        let start = rest.trim_start_matches([' ', '`', ',']);
        let start = start
            .strip_prefix("and ")
            .map_or(start, |joined| joined.trim_start_matches([' ', '`']));
        let Some(found) = PATH.find(start) else {
            return (paths, rest);
        };
        let path = found.as_str().trim_end_matches(['.', ':']);
        paths.push(path.to_owned());
        rest = &start[path.len()..];
    }
}

/// `text` without the punctuation that joins a path to its reason.
fn reason(text: &str) -> &str {
    text.trim_start_matches([' ', '`', ',', ':', '-', '\u{2014}', '\u{2013}', '('])
        .trim()
}

/// Whether `text`, what follows some paths, holds nothing but punctuation.
fn nothing_left(text: &str) -> bool {
    text.trim_matches([' ', '`', ',', '.', ':']).is_empty()
}

/// The paths and reason of one list item, or none when it is neither paths
/// then a reason nor a reason then only paths.
fn read_item(item: &str) -> Option<(Vec<String>, String)> {
    let (paths, rest) = leading(item);
    if !paths.is_empty() {
        let why = reason(rest);
        return (!why.is_empty()).then(|| (paths, why.to_owned()));
    }
    let (head, tail) = item.split_once(':')?;
    let (paths, rest) = leading(tail.trim_start());
    (!paths.is_empty() && nothing_left(rest)).then(|| (paths, head.trim().to_owned()))
}

/// Each group of paths with one reason a section's `lines` hold, and each
/// line that reads as neither.
fn read_section(lines: &[String]) -> (Vec<(Vec<String>, String)>, Vec<String>) {
    let (mut groups, mut unread) = (Vec::new(), Vec::new());
    // The list item being read, across its continuation lines.
    let mut item: Option<String> = None;
    // The prose line ending in a colon that opens lines of only paths, and
    // the paths those lines gave.
    let mut label: Option<(String, Vec<String>)> = None;
    let finish_item = |item: &mut Option<String>,
                       groups: &mut Vec<(Vec<String>, String)>,
                       unread: &mut Vec<String>| {
        if let Some(text) = item.take() {
            match read_item(&text) {
                Some(found) => groups.push(found),
                None => unread.push(format!("- {text}")),
            }
        }
    };
    let finish_label = |label: &mut Option<(String, Vec<String>)>,
                        groups: &mut Vec<(Vec<String>, String)>| {
        if let Some((why, paths)) = label.take()
            && !paths.is_empty()
        {
            groups.push((paths, why));
        }
    };
    for line in lines {
        if line.trim().is_empty() {
            finish_item(&mut item, &mut groups, &mut unread);
            finish_label(&mut label, &mut groups);
            continue;
        }
        if let Some(captures) = ITEM.captures(line) {
            finish_item(&mut item, &mut groups, &mut unread);
            finish_label(&mut label, &mut groups);
            item = Some(captures[1].trim().to_owned());
            continue;
        }
        if line.starts_with([' ', '\t'])
            && let Some(text) = item.as_mut()
        {
            text.push(' ');
            text.push_str(line.trim());
            continue;
        }
        finish_item(&mut item, &mut groups, &mut unread);
        let (paths, rest) = leading(line.trim());
        if !paths.is_empty() && nothing_left(rest) {
            match label.as_mut() {
                Some((_, held)) => held.extend(paths),
                None => unread.push(line.clone()),
            }
            continue;
        }
        finish_label(&mut label, &mut groups);
        match line.trim().strip_suffix(':') {
            Some(words) => label = Some((words.trim().to_owned(), Vec::new())),
            None => unread.push(line.clone()),
        }
    }
    finish_item(&mut item, &mut groups, &mut unread);
    finish_label(&mut label, &mut groups);
    (groups, unread)
}

/// One change's new snapshot, or the lines that keep it from one.
enum Moved {
    Lines {
        lines: Vec<Value>,
        considered: usize,
        owed: usize,
        record: String,
    },
    Unread(Vec<(String, String)>),
}

/// The lines that move the sections of the plan at `plan` for the change at
/// `path`; none when its text has neither section.
fn moved(host: &dyn Host, path: &str, plan: &str) -> Result<Option<Moved>, PluginError> {
    let Some((record, _)) = plan_at(host, plan)? else {
        return Ok(None);
    };
    let Some(text) = board::get_text(host, &record.text)? else {
        return Ok(None);
    };
    let sections = cut(&text);
    if !sections.found {
        return Ok(None);
    }
    let (considered_groups, mut unread) = read_section(&sections.considered);
    let (owed_groups, owed_unread) = read_section(&sections.owed);
    unread.extend(owed_unread);
    let mut unread: Vec<(String, String)> = (unread.into_iter())
        .map(|line| {
            (
                line,
                "it reads as neither paths and a reason nor a label".to_owned(),
            )
        })
        .collect();
    let named: Vec<String> = (considered_groups.iter().chain(owed_groups.iter()))
        .flat_map(|(paths, _)| paths.iter().cloned())
        .collect();
    let found = referenced(host, &named)?;
    for each in &named {
        if found.get(each).is_none() {
            unread.push((each.clone(), "it reaches nothing or is degraded".to_owned()));
        }
    }
    for (paths, _) in &owed_groups {
        for each in paths {
            if found
                .get(each)
                .is_some_and(|(kind, _)| kind != CONTROL_TYPE)
            {
                unread.push((each.clone(), format!("it is no {CONTROL_TYPE}")));
            }
        }
    }
    if !unread.is_empty() {
        return Ok(Some(Moved::Unread(unread)));
    }
    let mut considered = record.considered.clone().unwrap_or_default();
    considered.extend(
        considered_groups
            .into_iter()
            .map(|(records, why)| Considered { records, why }),
    );
    let mut owed = record.owed.clone().unwrap_or_default();
    for (paths, why) in owed_groups {
        owed.extend(paths.into_iter().map(|control| Owed {
            control,
            why: why.clone(),
        }));
    }
    let counts = (considered.len(), owed.len());
    let (written, lines) = plan_lines(
        host.settings().now,
        path,
        &sections.kept,
        Planned {
            uses: record.uses,
            paths: record.paths.unwrap_or_default(),
            considered,
            owed,
        },
        record.records,
    );
    Ok(Some(Moved::Lines {
        lines,
        considered: counts.0,
        owed: counts.1,
        record: written,
    }))
}

/// Adds to `lines` the lines that give each open change with a section a new
/// snapshot. A change the lines already rewrite has its `plan` set there, so
/// no path is written twice.
pub(super) fn rewrite(host: &dyn Host, lines: &mut Vec<Value>) -> Result<Output, PluginError> {
    let schemas = compiled();
    let mut stores = stored(host, ChangeV1::TYPE)?;
    stores.extend(stored(host, ChangeRecord::TYPE)?);
    stores.sort_by(|a, b| a.path.cmp(&b.path));
    let (mut changes, mut unread) = (Vec::new(), Vec::new());
    let (mut considered, mut owed) = (0, 0);
    for store in stores {
        let open = store
            .body
            .get("phase")
            .and_then(Value::as_str)
            .is_some_and(|phase| !is_final(phase));
        let plan = store
            .body
            .get("plan")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let (true, Some(plan)) = (open, plan) else {
            continue;
        };
        match moved(host, &store.path, &plan)? {
            None => {}
            Some(Moved::Unread(found)) => {
                unread.extend(found.into_iter().map(|(line, why)| Unread {
                    path: store.path.clone(),
                    code: UNREAD,
                    line,
                    why,
                }));
            }
            Some(Moved::Lines {
                lines: mut written,
                considered: held,
                owed: left,
                record,
            }) => {
                if let Err(why) = conforms(&schemas, &written) {
                    unread.push(Unread {
                        path: store.path.clone(),
                        code: UNREAD,
                        line: plan,
                        why,
                    });
                    continue;
                }
                let present = (lines.iter_mut()).find(|line| {
                    line.get("path").and_then(Value::as_str) == Some(store.path.as_str())
                });
                if let Some(line) = present {
                    line["record"]["plan"] = json!(record);
                } else {
                    let mut body = store.body;
                    body.insert("plan".to_owned(), json!(record));
                    written.push(json!({"path": store.path, "expect": store.blob, "record": body}));
                }
                lines.extend(written);
                considered += held;
                owed += left;
                changes.push(store.path);
            }
        }
    }
    let total = changes.len();
    changes.truncate(LISTED);
    Ok(Output {
        applied: false,
        changes,
        total,
        truncated: total > LISTED,
        considered,
        owed,
        unread,
    })
}
