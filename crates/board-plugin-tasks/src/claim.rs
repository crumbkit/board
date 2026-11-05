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
                       agent: a session's local_ id, a person's handle such as luke, or else an \
                       agent's name."
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
/// Board took: a session by its `local_` id, a person by a handle or an
/// email address, and otherwise an agent by its name. It is refused beside
/// any part, since the two would name the owner twice.
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
    let session = (text.strip_prefix("local_")).is_some_and(|id| {
        !id.is_empty()
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
    });
    let slug = !text.is_empty()
        && (text.split('-')).all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    let email = matches!(text.split_once('@'), Some((name, host)) if !name.is_empty() && !host.is_empty() && !host.contains('@'))
        && !text.contains(char::is_whitespace);
    Ok(Some(match (session, slug || email) {
        (true, _) => Actor {
            session: Some(text),
            ..Actor::default()
        },
        (false, true) => Actor {
            person: Some(text),
            ..Actor::default()
        },
        (false, false) => return Ok(Actor::of(None, None, None, Some(text))),
    }))
}
