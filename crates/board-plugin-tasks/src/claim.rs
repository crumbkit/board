//! `claim`: set who holds a task, refused when someone else holds it.

use board_ops::{Host, PluginError};
use board_plugin::shape::{Person, SessionId, Slug};
use board_repos::Date;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::task::actor;
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
}

/// Sets the task's owner, refused when another owner holds it.
pub(crate) fn claim(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: ClaimInput = PluginError::read_input(PLUGIN, "claim", input)?;
    let owner = actor(given.session, given.program, given.person);
    let Some(owner) = owner else {
        let why = format!(
            "claiming {} names nobody; give --session, --program or --person",
            given.path
        );
        return Err(refusal(
            "tasks::claim::no_owner",
            why,
            [("path", json!(given.path))],
        ));
    };
    let owner = json!(owner);
    let change = |record: &Map<String, Value>, _: Date| -> Result<Values, PluginError> {
        match record.get("owner") {
            None | Some(Value::Null) => {}
            Some(held) if *held == owner => {}
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
