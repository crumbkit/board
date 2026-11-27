//! `plans.decide`: records a decision from flags, so an agent never builds
//! a `plans/decision/v1` by hand.
//!
//! It fills `$type`, `decided` with the host's time and `by` from the
//! session, person and program it is given, and writes the decision under
//! `/projects/<project>/decisions/` as `put --new` names a new one. A
//! project that held nothing before is still written to, with a warning
//! that names the nearest projects, so a typo in its name shows.

use board_model::{PluginCode, PluginName, Refusal, utc_seconds};
use board_ops::{Host, Limit, PluginError};
use board_plugin::shape::{Person, Record, Ref, SessionId, Slug, Url};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::DecisionRecord;

/// The code of the warning for a project that held nothing before.
const NEW_PROJECT: &str = "plans::decide::new_project";

/// How many of the nearest projects the warning names.
const NEAREST: usize = 3;

/// The decision to record, who made it and the project it concerns.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DecideInput")]
pub(crate) struct Input {
    /// The project the decision concerns, such as `board`; the decision is written under `/projects/<project>/decisions/`.
    #[schemars(regex(pattern = r"^[a-z0-9][a-z0-9._-]*$"))]
    project: String,
    /// What was chosen.
    #[schemars(length(min = 1))]
    decision: String,
    /// The reason for the choice.
    #[schemars(length(min = 1))]
    why: String,
    /// The deciding session, by its local_ id; give it, the person, or both.
    #[schemars(with = "Option<SessionId>")]
    session: Option<String>,
    /// The person who decided, such as ana; give it only for a decision that person made.
    #[schemars(with = "Option<Person>")]
    person: Option<String>,
    /// The program that decided, such as claude-code.
    #[schemars(with = "Option<Slug>")]
    program: Option<String>,
    /// Whether the owner should look at the decision, recorded as review wanted.
    #[serde(default)]
    review: bool,
    /// What was not chosen.
    #[serde(default)]
    alternatives: Vec<String>,
    /// The board paths the decision concerns, such as a plan or an answer.
    #[serde(default)]
    #[schemars(with = "Vec<Ref>")]
    refs: Vec<String>,
    /// The web pages the decision concerns, such as a pull request.
    #[serde(default)]
    #[schemars(with = "Vec<Url>")]
    urls: Vec<String>,
    /// The one board path the decision decides, such as the plan revision `/changes/<name>/plans/<id>` it approves.
    #[schemars(with = "Option<Ref>")]
    decides: Option<String>,
}

/// The decision written. `path` is where, `line` says so in one sentence,
/// and `warning` is given when the project held nothing before.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DecideOutput")]
pub(crate) struct Output {
    path: String,
    line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<Warning>,
}

/// A decision written to a project that held nothing before, which may be a
/// typo of a project's name.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DecideWarning")]
struct Warning {
    code: &'static str,
    message: String,
}

pub(crate) fn decide(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        project,
        decision,
        why,
        session,
        person,
        program,
        review,
        alternatives,
        refs,
        urls,
        decides,
    } = PluginError::read_input("plans", "decide", input)?;
    if session.is_none() && person.is_none() {
        return Err(PluginError {
            code: "plans::decide::no_author".to_owned(),
            message: "the decision names no author: give the deciding session or person".to_owned(),
            fields: Map::new(),
            hint: Some(
                "give --session with your local_ id, or --person for a decision a person made"
                    .to_owned(),
            ),
        });
    }
    let mut by = Map::new();
    for (name, value) in [
        ("session", session),
        ("person", person),
        ("program", program),
    ] {
        if let Some(value) = value {
            by.insert(name.to_owned(), json!(value));
        }
    }
    let mut record = Map::new();
    record.insert("$type".to_owned(), json!(DecisionRecord::TYPE));
    record.insert("decision".to_owned(), json!(decision));
    record.insert("why".to_owned(), json!(why));
    record.insert("by".to_owned(), Value::Object(by));
    record.insert(
        "decided".to_owned(),
        json!(utc_seconds(host.settings().now)),
    );
    for (name, list) in [
        ("alternatives", alternatives),
        ("refs", refs),
        ("urls", urls),
    ] {
        if !list.is_empty() {
            record.insert(name.to_owned(), json!(list));
        }
    }
    if let Some(decides) = decides {
        record.insert("decides".to_owned(), json!(decides));
    }
    if review {
        record.insert("review".to_owned(), json!("wanted"));
    }
    let folder = format!("/projects/{project}");
    let projects = projects(host)?;
    let line = json!({"new": format!("{folder}/decisions/"), "record": record});
    let report = (host.put(&[line]))
        .map_err(|error| PluginError::board_refused("plans", "put --batch", &error.refusal()))?;
    let path = (report.writes.first()).map_or_else(String::new, |write| write.display.to_string());
    let warning = (!projects.contains(&project)).then(|| {
        let warning = Warning {
            code: NEW_PROJECT,
            message: new_project(&folder, &project, &projects),
        };
        host.warn(&warning.refusal(&path));
        warning
    });
    let output = Output {
        line: format!("Recorded {path}."),
        path,
        warning,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings"))
}

impl Warning {
    /// The warning as Board prints it, with a hint to move the decision at
    /// `path` when the project's name was a typo.
    fn refusal(&self, path: &str) -> Refusal {
        let plans: PluginName = "plans".parse().expect("plans is a plugin name");
        let code = PluginCode::parse(&plans, self.code.to_owned()).expect("the code is plans'");
        let mut refusal = Refusal::plugin(code, self.message.clone(), Map::new());
        refusal.hint = Some(format!(
            "when the project's name is a typo, record the decision again under the right one and run `board rm {path}`"
        ));
        refusal
    }
}

/// The names of the projects that hold an entry, each the folder below
/// `/projects/`.
fn projects(host: &dyn Host) -> Result<Vec<String>, PluginError> {
    let sql = "SELECT DISTINCT substr(logical, 11, instr(substr(logical, 11), '/') - 1) \
               FROM entries WHERE logical LIKE '/projects/%/%'";
    let report = (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("plans", "query", &error.refusal()))?;
    Ok((report.rows.iter())
        .filter_map(|row| row.first()?.as_str().map(str::to_owned))
        .filter(|name| !name.is_empty())
        .collect())
}

/// The warning's words for `folder`, which held nothing before, naming the
/// projects of `projects` whose names are nearest `project`.
fn new_project(folder: &str, project: &str, projects: &[String]) -> String {
    let mut ranked: Vec<(usize, &str)> = (projects.iter())
        .map(|name| (distance(project, name), name.as_str()))
        .collect();
    ranked.sort_unstable();
    let nearest: Vec<&str> = (ranked.iter().take(NEAREST))
        .map(|(_, name)| *name)
        .collect();
    let named = match nearest.as_slice() {
        [] => "the board holds no other project".to_owned(),
        [one] => format!("the nearest project is {one}"),
        [rest @ .., last] => format!("the nearest projects are {} and {last}", rest.join(", ")),
    };
    format!("{folder} held nothing before this decision, so it starts a new project; {named}.")
}

/// The Levenshtein distance between `one` and `two`, counted in chars. It
/// ranks a handful of short project names, so this two-row form is enough;
/// board-plugin's capability suggestions use the strsim crate, which this
/// crate would take on once it needs another string metric.
fn distance(one: &str, two: &str) -> usize {
    let two: Vec<char> = two.chars().collect();
    let mut row: Vec<usize> = (0..=two.len()).collect();
    for (i, a) in one.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, b) in two.iter().enumerate() {
            let replaced = previous + usize::from(a != *b);
            previous = row[j + 1];
            row[j + 1] = replaced.min(previous + 1).min(row[j] + 1);
        }
    }
    row[two.len()]
}

#[cfg(test)]
mod tests;
