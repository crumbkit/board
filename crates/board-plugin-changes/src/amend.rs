//! The `plan-amend` capability: a new plan version that only adds text
//! under the two test headings of the change's current plan, and the
//! `changes/amendment/v1` record that names the headings it filled, which
//! `next` and the pull request body show the owner.
//!
//! It takes no paths and no owed entries: those widen the scope the owner
//! approved, so they still need the owner's resend through `changes plan`.

use board_ops::{Host, PluginError, literal};
use board_plugin::shape::{Record, Ref, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board::{self, rows, text};
use crate::change::{line, plan_at, read, touch};
use crate::gates::{FENCES, HEADING, sections};
use crate::phases::final_phase;
use crate::plan::{PLAN_WORDS, Planned, plan_lines, stamp, thousands, words};
use crate::records::{PlanRecord, is_final};
use crate::{PLUGIN, refusal, to_value};

/// `changes/amendment/v1`: the test headings one `changes.plan-amend`
/// filled in a plan.
///
/// It lives at `/changes/<name>/amendments/<stamp>`, with the stamp of the
/// plan it wrote.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/{change}/amendments/"))]
pub(crate) struct AmendmentRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// The plan version the amendment wrote.
    #[schemars(with = "Ref<PlanRecord>")]
    pub(crate) plan: String,
    /// The plan version it added to.
    #[schemars(with = "Ref<PlanRecord>")]
    pub(crate) from: String,
    /// The test headings it filled in the plan's text, one or both.
    #[schemars(length(min = 1))]
    pub(crate) sections: Vec<Section>,
}

impl Record for AmendmentRecord {
    const TYPE: &'static str = "changes/amendment/v1";
}

/// A heading of a plan's text that an amendment may fill, which the land
/// gate's `test-sections` check reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(inline)]
pub(crate) enum Section {
    #[serde(rename = "What the tests prove")]
    Proves,
    #[serde(rename = "What the tests do not prove")]
    DoesNotProve,
}

impl Section {
    pub(crate) fn heading(self) -> &'static str {
        match self {
            Self::Proves => "What the tests prove",
            Self::DoesNotProve => "What the tests do not prove",
        }
    }
}

/// What `changes.plan-amend` adds, as flags: text for one or both test
/// headings, and nothing else.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PlanAmendInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(
        regex(pattern = r"\S"),
        description = "Markdown for the plan's \"What the tests prove\" heading, refused when \
                       that heading already holds text."
    )]
    proves: Option<String>,
    #[schemars(
        regex(pattern = r"\S"),
        description = "Markdown for the plan's \"What the tests do not prove\" heading, refused \
                       when that heading already holds text."
    )]
    does_not_prove: Option<String>,
}

/// The plan version written, its text, the amendment record, the plan it
/// added to, and the headings it filled.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "PlanAmendOutput")]
pub(crate) struct Output {
    path: String,
    text: String,
    amendment: String,
    from: String,
    sections: Vec<Section>,
}

/// One amendment as `next` and the pull request body show it: its path, the
/// plan it added to, and the headings it filled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct Amended {
    pub(crate) path: String,
    pub(crate) from: String,
    pub(crate) sections: Vec<Section>,
}

pub(crate) fn plan_amend(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        proves,
        does_not_prove,
    } = PluginError::read_input(PLUGIN, "plan-amend", input)?;
    let (mut change, blob) = read(host, &path)?;
    if is_final(&change.phase) {
        return Err(final_phase(
            &path,
            &change.phase,
            format!(
                "the change at {path} is {}, so its plan takes no amendment; list open changes \
                 with `board changes open`, or start a new one",
                change.phase
            ),
        ));
    }
    let (from, plan) = current(host, &path, change.plan.as_deref())?;
    let held = board::get_text(host, &plan.text)?.unwrap_or_default();
    let given = [
        (Section::Proves, proves),
        (Section::DoesNotProve, does_not_prove),
    ];
    let (text, filled) = new_sections(&path, held, given)?;
    if filled.is_empty() {
        return Err(refusal(
            "changes::amend::nothing",
            format!(
                "the amendment adds nothing to the plan of {path}; give text for `proves` or \
                 `does_not_prove`. To add a path or an owed entry, the owner resends the plan \
                 with `board changes plan`"
            ),
            [("path", json!(path))],
        ));
    }
    let counted = words(&text);
    if counted > PLAN_WORDS {
        return Err(refusal(
            "changes::plan::too_long",
            format!(
                "the amended plan for {path} would hold {} words, over the 3,000-word limit; add \
                 less text",
                thousands(counted)
            ),
            [("path", json!(path)), ("words", json!(counted))],
        ));
    }
    let now = host.settings().now;
    let planned = Planned {
        uses: plan.uses.clone(),
        paths: plan.paths.clone().unwrap_or_default(),
        considered: plan.considered.clone().unwrap_or_default(),
        owed: plan.owed.clone().unwrap_or_default(),
        lifecycle: plan.lifecycle.clone(),
        answers: plan.answers,
    };
    let kept = plan.records.clone().filter(|records| !records.is_empty());
    let (record, mut lines) = plan_lines(now, &path, &text, planned, kept);
    let amendment = format!("{path}/amendments/{}", stamp(now));
    let body = AmendmentRecord {
        tag: Tag::default(),
        plan: record.clone(),
        from: from.clone(),
        sections: filled.clone(),
    };
    lines.push(json!({"path": amendment, "expect": "none", "record": body}));
    change.plan = Some(record.clone());
    lines.push(line(&path, &blob, &touch(host, change)));
    board::put(host, &lines)?;
    Ok(to_value(Output {
        text: format!("{record}.md"),
        path: record,
        amendment,
        from,
        sections: filled,
    }))
}

/// The path of the change's current plan and its record, refusing with
/// `changes::amend::no_plan` when the change names no plan Board reads.
fn current(
    host: &dyn Host,
    path: &str,
    plan: Option<&str>,
) -> Result<(String, PlanRecord), PluginError> {
    let found = match plan {
        Some(plan) => plan_at(host, plan)?.map(|(record, _)| (plan.to_owned(), record)),
        None => None,
    };
    found.ok_or_else(|| {
        refusal(
            "changes::amend::no_plan",
            format!(
                "the change at {path} names no plan record to amend; write its plan with \
                 `board changes plan`, then run this again"
            ),
            [("path", json!(path))],
        )
    })
}

/// `text` with each given section's text under its heading, and the
/// headings filled, refusing when any given heading already holds text.
fn new_sections(
    path: &str,
    mut text: String,
    given: [(Section, Option<String>); 2],
) -> Result<(String, Vec<Section>), PluginError> {
    let mut filled = Vec::new();
    let mut held = Vec::new();
    for (section, added) in given {
        let Some(added) = added else {
            continue;
        };
        match with_section(&text, section, &added) {
            Some(amended) => {
                text = amended;
                filled.push(section);
            }
            None => held.push(section.heading()),
        }
    }
    if !held.is_empty() {
        return Err(refusal(
            "changes::amend::section_held",
            format!(
                "the plan of {path} already has text under {}; an amendment only adds, so leave \
                 that heading out, or rewrite the plan with `board changes plan`",
                held.join(" and ")
            ),
            [("path", json!(path)), ("sections", json!(held))],
        ));
    }
    Ok((text, filled))
}

/// `text` with `added`, trimmed, under the heading of `section`: in place
/// when the text has that heading with nothing under it, or after the text
/// under a new `##` heading when it has none. None when the heading holds
/// text already. Every other byte of `text` is kept.
pub(crate) fn with_section(text: &str, section: Section, added: &str) -> Option<String> {
    let heading = section.heading();
    let name = heading.to_lowercase();
    let added = added.trim();
    match sections(text).get(&name) {
        Some(under) if !under.is_empty() => return None,
        Some(_) => {
            let mut fenced = false;
            let mut at = 0;
            for each in text.split_inclusive('\n') {
                let line = each.trim_end_matches(['\n', '\r']);
                let fence = FENCES.iter().any(|f| line.trim_start().starts_with(f));
                let named = !fenced
                    && (HEADING.captures(line).and_then(|found| found.get(1)))
                        .is_some_and(|found| found.as_str().to_lowercase() == name);
                if fence {
                    fenced = !fenced;
                }
                at += each.len();
                if named {
                    let newline = if each.ends_with('\n') { "" } else { "\n" };
                    return Some(format!(
                        "{}{newline}\n{added}\n{}",
                        &text[..at],
                        &text[at..]
                    ));
                }
            }
        }
        None => {}
    }
    let newline = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    Some(format!("{text}{newline}\n## {heading}\n\n{added}\n"))
}

/// The amendments since the last plan `changes.plan` wrote for the change at
/// `path`, oldest first: the amendment that wrote `plan`, the one that wrote
/// the plan it added to, and so on.
pub(crate) fn chain(
    host: &dyn Host,
    path: &str,
    plan: Option<&str>,
) -> Result<Vec<Amended>, PluginError> {
    let Some(mut current) = plan.map(str::to_owned) else {
        return Ok(Vec::new());
    };
    let prefix = format!("{path}/amendments/");
    let sql = format!(
        "SELECT logical, body FROM records WHERE type = {} AND code IS NULL AND \
         substr(logical, 1, {}) = {}",
        literal(AmendmentRecord::TYPE),
        prefix.chars().count(),
        literal(&prefix)
    );
    let mut by_plan: std::collections::HashMap<String, (String, AmendmentRecord)> =
        std::collections::HashMap::new();
    for row in rows(host, &sql)? {
        let Some(logical) = text(row.first()) else {
            continue;
        };
        let parsed =
            serde_json::from_value::<AmendmentRecord>(Value::Object(board::body(row.get(1))));
        if let Ok(record) = parsed {
            by_plan.insert(record.plan.clone(), (logical.to_owned(), record));
        }
    }
    let mut found = Vec::new();
    // Each step removes an entry, so a cycle of `from` cannot loop.
    while let Some((logical, record)) = by_plan.remove(&current) {
        current.clone_from(&record.from);
        found.push(Amended {
            path: logical,
            from: record.from,
            sections: record.sections,
        });
    }
    found.reverse();
    Ok(found)
}

#[cfg(test)]
mod tests;
