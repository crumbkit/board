//! The `lifecycle` capability: the states, events, transition owners and
//! crash points of what a change creates, stops, replaces, retires, retries,
//! restores or migrates, in a record of its own beside the plan.

use board_ops::{Host, PluginError};
use board_plugin::shape::{PathKey, Record, Ref, Summary, Tag};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::change::{line, plan_at, read, touch};
use crate::flow::blob_at;
use crate::gates::sections;
use crate::phases::final_phase;
use crate::plan::{thousands, words};
use crate::records::is_final;
use crate::{PLUGIN, board, refusal, to_value};

/// The most words a change's lifecycle text holds.
const LIFECYCLE_WORDS: usize = 1_500;
/// How a plan's Lifecycle section says nothing changes state, as the
/// development flow's checks read it.
pub(crate) const NONE: &str = r"^none\s*(?:[.:]|\n?\z)";
/// The characters that make a logical path a pattern.
const GLOB: [char; 3] = ['*', '?', '['];

/// `changes/lifecycle/v1`: the states, events, transition owners and crash
/// points of what a change creates, stops, replaces, retires, retries,
/// restores or migrates.
///
/// It lives at `/changes/<name>/lifecycle`, beside its text at
/// `lifecycle.md`, and the plan's `lifecycle` names it. The builder's brief
/// names its text and the reviewer reads it.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LifecycleRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// The state table, the crash points and the invariants in Markdown,
    /// at most 1,500 words.
    #[schemars(with = "Ref")]
    pub(crate) text: String,
    /// The repository file, by logical path, where the landed table lives
    /// for the next author. A path, no pattern.
    #[schemars(with = "PathKey")]
    pub(crate) doc: String,
    /// The external systems whose state the lifecycle touches, each named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<Summary>>")]
    pub(crate) external: Option<Vec<String>>,
}

impl Record for LifecycleRecord {
    const TYPE: &'static str = "changes/lifecycle/v1";
}

/// What `changes.lifecycle` takes, on stdin because `external` is a list.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "LifecycleInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(
        regex(pattern = r"\S"),
        description = "The state table, the crash points and the invariants in Markdown, at most \
                       1,500 words."
    )]
    text: String,
    #[schemars(
        with = "PathKey",
        description = "The repository file, by logical path under the change's mount, where the \
                       landed table lives for the next author; a path, no pattern."
    )]
    doc: String,
    #[serde(default)]
    #[schemars(
        with = "Vec<Summary>",
        description = "The external systems whose state the lifecycle touches, each named, a \
                       list given on stdin."
    )]
    external: Vec<String>,
}

/// The lifecycle record written and the path of its text.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "LifecycleOutput")]
pub(crate) struct Output {
    path: String,
    text: String,
}

pub(crate) fn lifecycle(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        text,
        doc,
        external,
    } = PluginError::read_input(PLUGIN, "lifecycle", input)?;
    let (change, blob) = read(host, &path)?;
    if is_final(&change.phase) {
        return Err(final_phase(
            &path,
            &change.phase,
            format!(
                "the change at {path} is {}, so it takes no new lifecycle record; list open \
                 changes with `board changes open`, or start a new one",
                change.phase
            ),
        ));
    }
    let counted = words(&text);
    if counted > LIFECYCLE_WORDS {
        return Err(refusal(
            "changes::lifecycle::too_long",
            format!(
                "the lifecycle text for {path} holds {} words, over the 1,500-word limit; cut it",
                thousands(counted)
            ),
            [("path", json!(path)), ("words", json!(counted))],
        ));
    }
    check_doc(&path, &change.mount, &doc)?;
    let found = match &change.plan {
        Some(plan) => plan_at(host, plan)?.map(|(record, held)| (plan.clone(), record, held)),
        None => None,
    };
    let Some((plan_path, mut plan, plan_blob)) = found else {
        return Err(refusal(
            "changes::lifecycle::no_plan",
            format!(
                "the change at {path} names no plan record to point at the lifecycle record; \
                 write its plan with `board changes plan`, then run this again"
            ),
            [("path", json!(path))],
        ));
    };
    let record = format!("{path}/lifecycle");
    let written = format!("{record}.md");
    plan.lifecycle = Some(record.clone());
    let body = LifecycleRecord {
        tag: Tag::default(),
        text: written.clone(),
        doc,
        external: (!external.is_empty()).then_some(external),
    };
    let lines = vec![
        json!({"path": written, "expect": blob_at(host, &written)?, "text": text}),
        json!({"path": record, "expect": blob_at(host, &record)?, "record": body}),
        json!({"path": plan_path, "expect": plan_blob, "record": plan}),
        line(&path, &blob, &touch(host, change)),
    ];
    board::put(host, &lines)?;
    Ok(to_value(Output {
        path: record,
        text: written,
    }))
}

/// Whether the Lifecycle section of the plan `text` says nothing changes
/// state.
pub(crate) fn says_none(text: &str) -> bool {
    let none = Regex::new(&format!("(?i){NONE}")).expect("the pattern is a regular expression");
    (sections(text).get("lifecycle")).is_some_and(|section| none.is_match(section))
}

/// Refuses a `doc` that is a pattern, that has a `.`, `..` or empty segment
/// or a trailing slash, or that names a file outside the change's `mount`.
fn check_doc(path: &str, mount: &str, doc: &str) -> Result<(), PluginError> {
    if doc.contains(GLOB) {
        return Err(refusal(
            "changes::lifecycle::doc_is_a_pattern",
            format!("the lifecycle doc for {path} is the pattern {doc}; name one file"),
            [("path", json!(path)), ("doc", json!(doc))],
        ));
    }
    if doc
        .split('/')
        .skip(1)
        .any(|each| matches!(each, "" | "." | ".."))
    {
        return Err(refusal(
            "changes::lifecycle::doc_not_a_file",
            format!(
                "the lifecycle doc for {path} is {doc}, which names no one file; give a path with \
                 no . or .. segment, no empty segment and no trailing slash"
            ),
            [("path", json!(path)), ("doc", json!(doc))],
        ));
    }
    if doc.starts_with(&format!("{}/", mount.trim_end_matches('/'))) {
        return Ok(());
    }
    Err(refusal(
        "changes::lifecycle::doc_outside_mount",
        format!(
            "the lifecycle doc {doc} is outside the change's repository {mount}; start it with \
             {mount}/, so the table lands where the next author finds it"
        ),
        [("path", json!(path)), ("doc", json!(doc))],
    ))
}
