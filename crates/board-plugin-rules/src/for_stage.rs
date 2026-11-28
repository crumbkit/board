//! `for-stage`: what the imposed rules registered against one workflow
//! stage give one kind of agent there, in at most 150 words.

use board_model::{AgentKind, LogicalPath, Stage, UnknownName};
use board_ops::{Host, PluginError};
use board_plugin::shape::{BoardPath, Prose, Summary};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::check::{self, scope};
use crate::index::{self, Registration};
use crate::records::Provide;
use crate::summary::summary;

/// The most words of skills and context a stage object holds.
pub(crate) const STAGE_WORDS: usize = 150;

/// One stage and one kind of agent, for a repository.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ForStageInput")]
pub(crate) struct Input {
    /// The repository the agent works in, by its logical path, such as /repos/app, or / when the repository is the board itself.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: String,
    /// The workflow stage: orient, plan, mockup, build, review, land or handoff.
    #[schemars(length(min = 1))]
    stage: String,
    /// The kind of agent: lead, ux, builder or reviewer.
    #[schemars(length(min = 1))]
    agent: String,
}

/// What the rules registered under /rules/imposed/ against the stage give
/// the kind of agent, by rule path, fitted into 150 words.
///
/// `skills` holds each skill as a pointer: the rule that registered it,
/// its name, the directory that holds its `SKILL.md`, its file, and the
/// rule's summary, or else the first sentence of its requirement. `context` holds each context file's
/// text whole, with the rule that registered it. Each registration is
/// fitted in path order, skipping any that do not fit, and `left_out`
/// names those skipped. `refused` holds one row for each registration that
/// cannot be given, such as one whose file is missing or that Board marks
/// degraded; the others are still given. `left_out` and `refused` count outside the 150 words.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ForStageOutput")]
pub(crate) struct Output {
    stage: Stage,
    agent: AgentKind,
    skills: Vec<Skill>,
    context: Vec<Context>,
    #[schemars(with = "Vec<BoardPath>")]
    left_out: Vec<String>,
    refused: Vec<Refused>,
}

/// A skill, by the rule that registered it, its name and file, and what
/// it is for.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Skill {
    #[schemars(with = "BoardPath")]
    rule: String,
    name: String,
    #[schemars(with = "BoardPath")]
    file: String,
    #[schemars(with = "Summary")]
    summary: String,
}

/// A context file's text, and the rule that registered it.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Context {
    #[schemars(with = "BoardPath")]
    rule: String,
    #[schemars(with = "Prose")]
    text: String,
}

/// A registration that could not be given, by its rule, with why.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ForStageRefused")]
struct Refused {
    rule: String,
    code: String,
    #[schemars(with = "Prose")]
    message: String,
}

/// One registration resolved: what it gives, or why it cannot.
enum Given {
    Skill(Skill),
    Context(Context),
    Refused(Refused),
}

/// The stage object for `stage` and `agent` in the repository at `path`.
pub(crate) fn for_stage(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path, stage, agent } = PluginError::read_input("rules", "for-stage", input)?;
    let stage: Stage = stage.parse().map_err(unknown)?;
    let agent: AgentKind = agent.parse().map_err(unknown)?;
    scope(host, &path)?;
    let slot: Vec<(String, Registration)> = (index::registrations(host)?.into_iter())
        .filter(|(_, registration)| {
            registration.show.stage == stage && registration.show.agent == agent
        })
        .collect();
    let given = resolve(host, slot)?;
    let mut output = Output {
        stage,
        agent,
        skills: Vec::new(),
        context: Vec::new(),
        left_out: Vec::new(),
        refused: Vec::new(),
    };
    let mut room = STAGE_WORDS;
    let mut fits = |cost: usize| {
        let fit = cost <= room;
        if fit {
            room -= cost;
        }
        fit
    };
    for (rule, given) in given {
        match given {
            Given::Refused(refused) => output.refused.push(refused),
            Given::Skill(skill) => {
                if fits(words(&[
                    &skill.rule,
                    &skill.name,
                    &skill.file,
                    &skill.summary,
                ])) {
                    output.skills.push(skill);
                } else {
                    output.left_out.push(rule);
                }
            }
            Given::Context(context) => {
                if fits(words(&[&context.rule, &context.text])) {
                    output.context.push(context);
                } else {
                    output.left_out.push(rule);
                }
            }
        }
    }
    Ok(serde_json::to_value(output).expect("Output holds only strings and lists"))
}

/// Each registration of the slot, in path order, with what it gives. Every
/// file is read through one `get_each`.
fn resolve(
    host: &dyn Host,
    slot: Vec<(String, Registration)>,
) -> Result<Vec<(String, Given)>, PluginError> {
    let files: Vec<Option<LogicalPath>> = (slot.iter())
        .map(|(_, registration)| registration.provides.file.parse().ok())
        .collect();
    let read = check::read(host, files.iter().flatten().collect())?;
    let mut given = Vec::new();
    for ((rule, registration), file) in slot.into_iter().zip(files) {
        let provides = &registration.provides;
        let missing = |reason: String| {
            let message = format!("the rule {rule} provides a file that {reason}");
            refused(&rule, "rules::stage::file_missing", message)
        };
        let entry = match (file, &registration.degraded) {
            (None, _) => missing(format!("{} names no board path", provides.file)),
            (Some(file), degraded) => match (&read[&file], degraded, provides.provide) {
                (Err(code), _, _) => missing(format!("Board cannot read, {file}: {code}")),
                (Ok(_), Some(code), _) => refused(
                    &rule,
                    "rules::stage::degraded",
                    format!(
                        "Board marks the rule {rule} degraded with {code}; run `board check \
                         {rule}` for why"
                    ),
                ),
                (Ok(_), None, Provide::Skill) => Given::Skill(Skill {
                    rule: rule.clone(),
                    name: skill_name(&file),
                    file: file.to_string(),
                    summary: (registration.summary.clone())
                        .unwrap_or_else(|| summary(&registration.requirement)),
                }),
                (Ok(bytes), None, Provide::Context) => Given::Context(Context {
                    rule: rule.clone(),
                    text: String::from_utf8_lossy(bytes).trim().to_owned(),
                }),
            },
        };
        given.push((rule, entry));
    }
    Ok(given)
}

/// A skill's name: the directory that holds its `SKILL.md`, or the file's
/// own name when it lies at the board's root.
fn skill_name(file: &LogicalPath) -> String {
    let mut segments = file.as_str().rsplit('/');
    let name = segments.next().unwrap_or_default();
    (segments.next())
        .filter(|parent| !parent.is_empty())
        .unwrap_or(name)
        .to_owned()
}

fn refused(rule: &str, code: &str, message: String) -> Given {
    Given::Refused(Refused {
        rule: rule.to_owned(),
        code: code.to_owned(),
        message,
    })
}

/// The words of `texts`, separated by any whitespace.
fn words(texts: &[&str]) -> usize {
    texts
        .iter()
        .map(|text| text.split_whitespace().count())
        .sum()
}

/// The refusal of a stage or agent kind that is not one of its list's.
fn unknown(error: UnknownName) -> PluginError {
    let message = error.to_string();
    let UnknownName { given, known, .. } = error;
    let mut fields = Map::new();
    fields.insert("given".to_owned(), Value::String(given));
    fields.insert("known".to_owned(), serde_json::json!(known));
    PluginError {
        code: "rules::stage::unknown".to_owned(),
        message,
        fields,
        hint: None,
    }
}

#[cfg(test)]
mod tests;
