//! The `plan` capability: a change's plan, the records it uses and the
//! paths it will edit.

use std::time::SystemTime;

use board_ops::{Host, PluginError};
use board_plugin::shape::{LogicalGlob, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board::{self};
use crate::change::{Referenced, kind_of, line, plan_at, read, referenced, touch};
use crate::phases::final_phase;
use crate::records::{Kind, PlanRecord, is_final};
use crate::{PLUGIN, refusal, to_value};

/// The most words a plan's text holds, and the most words the documents it
/// uses hold in all.
const PLAN_WORDS: usize = 3_000;
const USES_WORDS: usize = 3_000;

/// A plan for a change, given on stdin because `uses` and `paths` are
/// arrays.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PlanInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(
        regex(pattern = r"\S"),
        description = "The plan in Markdown, at most 3,000 words."
    )]
    text: String,
    #[serde(default)]
    #[schemars(
        with = "Vec<BoardPath>",
        description = "The records and documents the plan relies on, a list given on stdin; the \
                       documents hold at most 3,000 words in all, and none when left out."
    )]
    uses: Vec<String>,
    #[schemars(
        with = "Option<Vec<LogicalGlob>>",
        description = "The files and SQLite GLOB patterns the change will edit, a list given on \
                       stdin, by logical path under the change's mount, such as /repos/inbox/crates/inbox-store/**, \
                       where `*` also matches `/`; a build change needs one."
    )]
    paths: Option<Vec<String>>,
}

/// A logical path `board call` takes as a flag.
#[derive(Debug)]
pub(crate) enum BoardPath {}

impl JsonSchema for BoardPath {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        "BoardPath".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "string", "pattern": "^/", "format": "board-path"})
    }
}

/// The plan record written, its text, and how many references it uses.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "PlanOutput")]
pub(crate) struct Output {
    path: String,
    text: String,
    uses: usize,
}

/// How many whitespace-separated words `text` holds.
pub(crate) fn words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Each entry once, in order.
fn once(listed: Vec<String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for each in listed {
        if !kept.contains(&each) {
            kept.push(each);
        }
    }
    kept
}

/// The text of each Markdown document among `refs`, by reference.
pub(crate) fn documents(
    host: &dyn Host,
    refs: &[String],
    records: &Referenced,
) -> Result<Vec<(String, String)>, PluginError> {
    let texts: Vec<String> = (refs.iter())
        .filter(|each| {
            // Python's `endswith(".md")` is case-sensitive, so `A.MD` is no text.
            #[expect(
                clippy::case_sensitive_file_extension_comparisons,
                reason = "the Python plugin read only paths ending in lowercase .md"
            )]
            let text = each.ends_with(".md");
            text && records.get(each).is_some_and(|(found, _)| found == "file")
        })
        .cloned()
        .collect();
    let found = board::get(host, &texts)?;
    Ok((texts.into_iter().zip(found))
        .filter_map(|(each, bytes)| Some((each, String::from_utf8_lossy(&bytes?).into_owned())))
        .collect())
}

pub(crate) fn plan(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        text,
        uses,
        paths,
    } = PluginError::read_input(PLUGIN, "plan", input)?;
    let uses = once(uses);
    let paths = once(paths.unwrap_or_default());
    let (mut change, blob) = read(host, &path)?;
    if is_final(&change.phase) {
        return Err(final_phase(
            &path,
            &change.phase,
            format!(
                "the change at {path} is {}, so it takes no new plan; list open changes with \
                 `board call changes.open`, or start a new one",
                change.phase
            ),
        ));
    }
    check_paths(&path, &change.mount, &paths, kind_of(host, &path)?)?;
    let counted = words(&text);
    if counted > PLAN_WORDS {
        return Err(refusal(
            "changes::plan::too_long",
            format!(
                "the plan for {path} holds {} words, over the 3,000-word limit; cut it, and move \
                 detail into the records it uses",
                thousands(counted)
            ),
            [("path", json!(path)), ("words", json!(counted))],
        ));
    }
    let records = referenced(host, &uses)?;
    let dangling: Vec<&String> = uses
        .iter()
        .filter(|each| records.get(each).is_none())
        .collect();
    if !dangling.is_empty() {
        let listed: Vec<&str> = dangling.iter().map(|each| each.as_str()).collect();
        return Err(refusal(
            "changes::ref::dangling",
            format!(
                "these references in the plan reach nothing or are degraded: {}; check each path \
                 with `board ls` or `board check`",
                listed.join(", ")
            ),
            [("path", json!(path)), ("refs", json!(listed))],
        ));
    }
    let directories: Vec<&str> = (uses.iter())
        .filter(|each| {
            records
                .get(each)
                .is_some_and(|(found, _)| found == "directory")
        })
        .map(String::as_str)
        .collect();
    if !directories.is_empty() {
        return Err(refusal(
            "changes::plan::directory",
            format!(
                "the plan uses the directories {}, whose words the limit cannot count; use the \
                 documents in them that apply, choosing from each directory's index.md",
                directories.join(", ")
            ),
            [("path", json!(path)), ("refs", json!(directories))],
        ));
    }
    let words_used: usize = (documents(host, &uses, &records)?.iter())
        .map(|(_, text)| words(text))
        .sum();
    if words_used > USES_WORDS {
        return Err(refusal(
            "changes::plan::uses_too_long",
            format!(
                "the documents the plan for {path} uses hold {} words in all, over the \
                 3,000-word limit; use fewer, choosing from each directory's index.md",
                thousands(words_used)
            ),
            [("path", json!(path)), ("words", json!(words_used))],
        ));
    }
    let count = uses.len();
    let kept = kept_records(host, change.plan.as_deref())?;
    let (record, mut lines) = plan_lines(host.settings().now, &path, &text, uses, paths, kept);
    let written = format!("{record}.md");
    change.plan = Some(record.clone());
    lines.push(line(&path, &blob, &touch(host, change)));
    board::put(host, &lines)?;
    Ok(to_value(Output {
        path: record,
        text: written,
        uses: count,
    }))
}

/// The records attached to the plan the change names as `plan`, in order,
/// which a revised plan keeps; none when it names no plan Board reads or
/// that plan has none.
fn kept_records(host: &dyn Host, plan: Option<&str>) -> Result<Option<Vec<String>>, PluginError> {
    let Some(plan) = plan else {
        return Ok(None);
    };
    Ok(plan_at(host, plan)?
        .and_then(|(record, _)| record.records)
        .filter(|records| !records.is_empty()))
}

/// `count` with a comma between each group of three digits, as Python's
/// `{:,}` writes it.
fn thousands(count: usize) -> String {
    let digits = count.to_string();
    let mut shown = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            shown.push(',');
        }
        shown.push(digit);
    }
    shown
}

/// Refuses a build change's plan that declares no paths, and any path
/// outside the change's mount.
fn check_paths(path: &str, mount: &str, paths: &[String], kind: Kind) -> Result<(), PluginError> {
    if paths.is_empty() && kind == Kind::Build {
        return Err(refusal(
            "changes::plan::no_paths",
            format!(
                "the plan for {path} declares no paths; give `paths`, the files and SQLite GLOB \
                 patterns it will change by mounted path, such as {mount}/src/**, so its brief \
                 can list their rules"
            ),
            [("path", json!(path))],
        ));
    }
    let inside = format!("{}/", mount.trim_end_matches('/'));
    let outside: Vec<&str> = (paths.iter())
        .filter(|planned| !planned.starts_with(&inside))
        .map(String::as_str)
        .collect();
    if outside.is_empty() {
        return Ok(());
    }
    Err(refusal(
        "changes::plan::outside_mount",
        format!(
            "these planned paths are outside the change's repository {mount}: {}; start each \
             with {mount}/",
            outside.join(", ")
        ),
        [("path", json!(path)), ("paths", json!(outside))],
    ))
}

/// The stamp a plan written at `now` is named by, `%Y%m%dT%H%M%S%fZ` in
/// UTC, such as 20261105T115534421737Z.
pub(crate) fn stamp(now: SystemTime) -> String {
    let (day, time, micros) = parts(now);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}{micros:06}Z",
        day.year,
        day.month,
        day.day,
        time / 3600,
        time / 60 % 60,
        time % 60,
    )
}

/// `now` as an RFC 3339 date-time in UTC to the second, such as
/// 2026-11-05T11:55:34Z.
pub(crate) fn time_of(now: SystemTime) -> String {
    let (day, time, _) = parts(now);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        day.year,
        day.month,
        day.day,
        time / 3600,
        time / 60 % 60,
        time % 60,
    )
}

/// The UTC day of `now`, its seconds into that day and its microseconds.
fn parts(now: SystemTime) -> (board_repos::Date, i64, u32) {
    let since = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    let day = board_repos::Date::from_days(seconds.div_euclid(86_400));
    (day, seconds.rem_euclid(86_400), since.subsec_micros())
}

/// A new plan record for the change at `path` that starts with `records`
/// attached, and the `put` lines that write its text and then the record.
pub(crate) fn plan_lines(
    now: SystemTime,
    path: &str,
    text: &str,
    uses: Vec<String>,
    paths: Vec<String>,
    records: Option<Vec<String>>,
) -> (String, Vec<Value>) {
    let record = format!("{path}/plans/{}", stamp(now));
    let written = format!("{record}.md");
    let body = PlanRecord {
        tag: Tag::default(),
        text: written.clone(),
        uses,
        paths: (!paths.is_empty()).then_some(paths),
        records,
    };
    let lines = vec![
        json!({"path": written, "expect": "none", "text": text}),
        json!({"path": record, "expect": "none", "record": body}),
    ];
    (record, lines)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{stamp, thousands};

    #[test]
    fn a_stamp_names_the_utc_time_to_the_microsecond() {
        let now = SystemTime::UNIX_EPOCH + Duration::new(1_793_966_134, 421_737_123);
        assert_eq!(stamp(now), "20261106T115534421737Z");
    }

    #[test]
    fn a_count_is_written_with_commas_between_thousands() {
        assert_eq!(thousands(3), "3");
        assert_eq!(thousands(3_001), "3,001");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
