//! The `ask` capability: one question that asks the owner to approve a
//! change's plan, its data understanding checks and its mockups at once,
//! filed through `plans.ask` and attached to the change. Its approving
//! option names each of them, so one answer passes owner-approval,
//! data-signed and mockups.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::board::call;
use crate::change::{attached, read, referenced};
use crate::gates::design::versioned;
use crate::gates::{UNDERSTANDING_TYPE, mockups, sections};
use crate::phases::{attach, plan_text};
use crate::{PLUGIN, refusal, to_value};

/// The change to ask about, the project its question is filed under, and
/// who asks.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AskInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(
        regex(pattern = r"^[a-z0-9][a-z0-9._-]*$"),
        description = "The project the question is filed under, such as board: \
                       /projects/<project>/questions/."
    )]
    project: String,
    #[schemars(
        description = "Who asks, as a plans/question/v1 `by`: the asking session's \
                              `session`, or the `person` who asks without one."
    )]
    by: Map<String, Value>,
}

/// The question attached to the change: its path, what `plans.ask` did
/// with it (`filed`, `merged` into the same open question, or `answered`
/// earlier), and the approving option's text.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "AskOutput")]
pub(crate) struct Output {
    path: String,
    question: String,
    outcome: String,
    approve: String,
}

/// The approving and the revising option of the question about the change
/// at `path`: the first names the change, its current `plan`, each
/// understanding check in `records` and each mockup in `named` with its
/// version; the second names none of them.
pub(crate) fn options(
    path: &str,
    plan: &str,
    records: &[String],
    named: &[String],
) -> (String, String) {
    let data = if records.is_empty() {
        String::new()
    } else {
        format!(
            ", with the data its understanding checks record: {}",
            records.join(", ")
        )
    };
    let shown = if named.is_empty() {
        String::new()
    } else {
        format!(", and the mockups {}", named.join(", "))
    };
    let approve = format!("Approve {path} as planned in {plan}{data}{shown}.");
    let revise = "Revise the plan, then ask again.".to_owned();
    (approve, revise)
}

pub(crate) fn ask(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path, project, by } = PluginError::read_input(PLUGIN, "ask", input)?;
    let path = path.trim_end_matches('/').to_owned();
    let (change, _) = read(host, &path)?;
    let cited = attached(host, &path)?;
    let found = referenced(host, &cited)?;
    let records: Vec<String> = (found.of_type(UNDERSTANDING_TYPE))
        .filter(|(_, body)| {
            body.get("change").and_then(Value::as_str) == Some(path.as_str())
                && body.get("subject").and_then(Value::as_str) == Some("data")
        })
        .map(|(record, _)| record.to_owned())
        .collect();
    let plan = sections(plan_text(host, &change)?.as_deref().unwrap_or_default());
    let versions = versioned(&plan);
    let named: Vec<String> = (mockups(&plan).into_iter())
        .map(
            |mockup| match versions.iter().find(|(each, _)| *each == mockup) {
                Some((_, version)) if !mockup.contains("/versions/") => {
                    format!("{mockup} version {version}")
                }
                _ => mockup,
            },
        )
        .collect();
    let Some(current) = change.plan.as_deref() else {
        return Err(refusal(
            "changes::ask::no_plan",
            format!("{path} names no plan to approve; write one with `board changes plan`"),
            [("path", json!(path))],
        ));
    };
    let (approve, revise) = options(&path, current, &records, &named);
    // `plans.ask` matches an earlier question by its words, so naming the
    // plan asks again once the plan is revised.
    let text = format!(
        "Do you approve the plan {current} of {path} ({})?",
        change.title
    );
    let question = json!({
        "question": text,
        "options": [{"id": "approve", "text": approve}, {"id": "revise", "text": revise}],
        "recommendation": "approve",
        "blocks": path,
        "mount": change.mount,
        "by": by,
    });
    let asked = call(
        host,
        "plans.ask",
        &json!({"project": project, "question": question}),
    )?;
    let field = |key: &str| {
        asked
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let filed = field("path");
    attach(host, &json!({"path": path, "ref": filed}))?;
    Ok(to_value(Output {
        path: filed,
        question: text,
        outcome: field("outcome"),
        approve,
    }))
}

#[cfg(test)]
mod tests {
    use super::options;
    use crate::gates::kinds::names_whole;

    #[test]
    fn the_approving_option_names_each_record_and_mockup_and_the_revising_one_names_none() {
        let records = ["/changes/t/understanding/data".to_owned()];
        let named = ["https://claude.ai/artifact/m1 version v2".to_owned()];
        let plan = "/changes/t/plans/2";
        let (approve, revise) = options("/changes/t", plan, &records, &named);
        for each in [
            "/changes/t",
            plan,
            &records[0],
            "https://claude.ai/artifact/m1",
        ] {
            assert!(names_whole(&approve, each), "{each}: {approve}");
            assert!(!names_whole(&revise, each), "{each}: {revise}");
        }
        let (bare, _) = options("/changes/t", plan, &[], &[]);
        assert_eq!(bare, "Approve /changes/t as planned in /changes/t/plans/2.");
    }
}
