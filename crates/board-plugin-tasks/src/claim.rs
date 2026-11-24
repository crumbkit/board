//! `claim`: set who holds a task, refused when someone else holds it.

use board_model::{Code, LogicalPath};
use board_ops::{Host, Plugin as _, PluginError};
use board_plugin::shape::{Actor, Person, Prose, SessionId, Slug};
use board_repos::{Date, Repo, listed};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::write::{Landing, Values, held, stamped};
use crate::{PLUGIN, Tasks, hinted, refusal};

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
                       is the session, Codex or Claude the program, the owner's own name the person, \
                       and the text the agent's name."
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
        description = "The person who holds the task, such as ana."
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
    let repo = held(&listed(host, PLUGIN)?, &given.path);
    if let Some(stale) = stale_schema(host, &repo)? {
        return Err(schema_stale(&repo, &given.path, &stale));
    }
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
    stamped(host, &given.path, &change, Landing::open())
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

/// The first task schema `repo`'s clone stores whose `owner` is not the
/// shape the plugin ships, or that it lacks: tasks/task/v2, which every
/// notes repo holds, and tasks/task/v3 when it holds it. `put` checks a
/// task against the schemas stored where the task lives, so a copy from
/// before the shapes migration, whose `owner` is text, refuses every claim.
/// The board's own tree is left out, since `board call` checks its schemas.
fn stale_schema(host: &dyn Host, repo: &Repo) -> Result<Option<String>, PluginError> {
    if repo.clone.is_none() {
        return Ok(None);
    }
    let refused = |error: &board_ops::ReadError| {
        PluginError::board_refused(PLUGIN, "get", &error.refusal("get"))
    };
    let root = repo.root.trim_end_matches('/');
    for (kind, shipped) in Tasks.schemas() {
        let stored = format!("{root}/.board/types/{kind}.schema");
        let path: LogicalPath = stored.parse().expect("a repo's root is a board path");
        let answer = host
            .get_each(&[path])
            .map_err(|error| refused(&error))?
            .pop();
        let schema: Value = match answer {
            Some(Ok(report)) => serde_json::from_slice(&report.bytes).unwrap_or(Value::Null),
            Some(Err(error)) if error.code() != Code::PathNotFound => {
                return Err(refused(&error));
            }
            // A repo without v3 takes v2 tasks alone; one without v2 has
            // never had the plugin's schemas.
            _ if kind.as_str() != crate::records::V2_TYPE => continue,
            _ => return Ok(Some(stored)),
        };
        if owner_shape(&schema) != owner_shape(&shipped) {
            return Ok(Some(stored));
        }
    }
    Ok(None)
}

/// The schema's `owner` property, its `$ref` into `$defs` followed, without
/// descriptions, which change no record's validity.
fn owner_shape(schema: &Value) -> Option<Value> {
    let mut owner = schema.pointer("/properties/owner")?.clone();
    if let Some(name) = (owner.get("$ref").and_then(Value::as_str))
        .and_then(|target| target.strip_prefix("#/$defs/"))
    {
        owner = schema.get("$defs")?.get(name)?.clone();
    }
    without_descriptions(&mut owner);
    Some(owner)
}

/// Removes every `description` keyword from `schema`.
fn without_descriptions(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            map.remove("description");
            map.values_mut().for_each(without_descriptions);
        }
        Value::Array(items) => items.iter_mut().for_each(without_descriptions),
        _ => {}
    }
}

/// The refusal of a claim on `logical` in `repo`, whose clone stores
/// `schema` older than the plugin's.
fn schema_stale(repo: &Repo, logical: &str, schema: &str) -> PluginError {
    let clone = repo.clone.as_deref().unwrap_or_default();
    let mount = &repo.root;
    let why = format!(
        "{logical} is in {clone}, whose {schema}.json predates the owner the plugin writes, so \
         `put` would refuse the claim; nothing was written"
    );
    let hint = format!(
        "run `cargo xtask migrate-shapes {clone}`, then with `--write`, then `board -C {clone} \
         plugin enable tasks` and commit, then `board mount update {mount}`, and claim again"
    );
    hinted(
        "tasks::repo::schema_stale",
        why,
        [
            ("path", json!(logical)),
            ("clone", json!(clone)),
            ("mount", json!(mount)),
            ("schema", json!(schema)),
        ],
        &hint,
    )
}
