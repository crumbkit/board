//! `claim`: set who holds a task, refused when someone else holds it.

use board_ops::{Host, PluginError};
use board_plugin::shape::{Actor, Person, Prose, SessionId, Slug};
use board_repos::Date;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::write::{Values, stamped};
use crate::{PLUGIN, refusal};

/// The task to claim, and who claims it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClaimInput {
    #[schemars(
        regex(pattern = r"^/.*\.md$"),
        description = "The task's path in the board, such as /tasks/call-the-bank.md or \
                       /repos/personal/tasks/x.md."
    )]
    pub(crate) path: String,
    #[schemars(
        length(min = 1),
        description = "Who holds the task, as one text in place of session, program, person and \
                       agent, read as the shapes migration reads an old owner: a local_ id in it \
                       is the session, Codex or Claude the program, Luke the person, and the \
                       text the agent's name."
    )]
    pub(crate) owner: Option<String>,
    #[schemars(
        with = "Option<SessionId>",
        description = "The session that holds the task, by its local_ id."
    )]
    pub(crate) session: Option<String>,
    #[schemars(
        with = "Option<Slug>",
        description = "The program that holds the task, such as claude-code."
    )]
    pub(crate) program: Option<String>,
    #[schemars(
        with = "Option<Person>",
        description = "The person who holds the task, such as luke."
    )]
    pub(crate) person: Option<String>,
    #[schemars(
        with = "Option<Prose>",
        description = "The name the agent that holds the task was given, such as \
                       inbox foundation builder."
    )]
    pub(crate) agent: Option<String>,
}

/// Sets the task's owner, refused when another owner holds it. An owner
/// with the same session as the one that holds it is the same owner.
pub(crate) fn claim(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: ClaimInput = PluginError::read_input(PLUGIN, "claim", input)?;
    let parts = [given.session, given.program, given.person, given.agent];
    let Some(owner) = owner(given.owner, parts)? else {
        let why = format!(
            "claiming {} names nobody; give --owner, --session, --program, --person or --agent",
            given.path
        );
        return Err(refusal(
            "tasks::claim::no_owner",
            why,
            [("path", json!(given.path))],
        ));
    };
    let same = |held: &Value| {
        serde_json::from_value::<Actor>(held.clone()).is_ok_and(|held| held.same(&owner))
    };
    let owner = json!(owner);
    let change = |record: &Map<String, Value>, _: Date| -> Result<Values, PluginError> {
        match record.get("owner") {
            None | Some(Value::Null) => {}
            // The same session claims again: the owner it was given stays.
            Some(held) if same(held) => return Ok(vec![("owner", Some(held.clone()))]),
            Some(held) => {
                let path = &given.path;
                let shown = held
                    .as_str()
                    .map_or_else(|| held.to_string(), str::to_owned);
                let why = format!(
                    "{path} is held by {shown}, so it is left as it is; leave it to {shown}, or \
                     claim another task"
                );
                return Err(refusal(
                    "tasks::task::held",
                    why,
                    [("path", json!(path)), ("owner", held.clone())],
                ));
            }
        }
        Ok(vec![("owner", Some(owner.clone()))])
    };
    stamped(host, &given.path, &change, false)
}

/// The owner that `owner` or the parts `[session, program, person, agent]`
/// name, or none when they name nobody. `owner` is the one text a released
/// Board took, read as [`Actor::from_text`] reads it, so it names the owner
/// the shapes migration made of the same text. It is refused beside any
/// part, since the two would name the owner twice.
pub(crate) fn owner(
    owner: Option<String>,
    parts: [Option<String>; 4],
) -> Result<Option<Actor>, PluginError> {
    let [session, program, person, agent] = parts;
    let Some(text) = owner else {
        return Ok(Actor::of(session, program, person, agent));
    };
    if [&session, &program, &person, &agent]
        .iter()
        .any(|part| part.is_some())
    {
        let why = format!(
            "--owner {text:?} names the owner and so do --session, --program, --person or \
             --agent; give --owner alone, or the parts alone"
        );
        return Err(refusal(
            "tasks::owner::twice",
            why,
            [("owner", json!(text))],
        ));
    }
    Ok(Actor::from_text(&text))
}
