//! How each old field becomes its new shape. These functions only change
//! values; `migrate` reads and writes the files.

use std::fmt::Write as _;

use board_plugin::shape::Actor;
use serde_json::{Map, Value, json};

/// What happened to one field of one record.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The field was rewritten to its new shape.
    Changed(&'static str),
    /// An optional actor held only blank text, so the field went.
    Dropped(&'static str, String),
    /// The old value maps to nothing, so it was left for a person to fix.
    Unmapped(&'static str, String),
    /// A subject named `name`, which now names the topic at
    /// `/topics/<slug>`, as the `--map` entry gave it when one did.
    Topic {
        slug: String,
        name: String,
        given: Option<Value>,
    },
}

/// Why a `--map` file cannot be used.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MapError {
    /// The file is not a JSON object.
    NotObject,
    /// The entry with this key maps to neither an `Actor`, a `Subject` nor
    /// a topic.
    Entry(String),
    /// The shared shapes' schemas did not compile, so nothing can be checked.
    Shapes,
}

/// The `--map` file: an old string to the value that replaces it, an actor
/// object, a subject string, or a `topics/topic/v1` record, which the
/// subject names by `/topics/<slug of its title>`.
#[derive(Default)]
pub(crate) struct Context {
    map: Map<String, Value>,
}

impl Context {
    /// The map `map`, refused before anything is written when it is not a
    /// JSON object or an entry's value is neither an `Actor` nor a `Subject`.
    pub(crate) fn new(map: Value) -> Result<Self, MapError> {
        let Value::Object(map) = map else {
            return Err(MapError::NotObject);
        };
        let shapes = board_plugin::shape::registry();
        let compiled = |name: &str| {
            let schema = shapes.get(name).cloned().unwrap_or(Value::Null);
            board_validate::Schema::compile(schema).ok()
        };
        let topic = (serde_json::from_str(super::topics::SCHEMA).ok())
            .and_then(|schema| board_validate::Schema::compile(schema).ok());
        let (Some(actor), Some(subject), Some(topic)) =
            (compiled("Actor"), compiled("Subject"), topic)
        else {
            return Err(MapError::Shapes);
        };
        for (key, value) in &map {
            let is_topic = value
                .as_object()
                .is_some_and(|object| object.contains_key("$type"));
            let shape = match value {
                _ if is_topic => &topic,
                Value::Object(_) => &actor,
                _ => &subject,
            };
            // A topic's path comes from its title, so the title needs a slug.
            let untitled = is_topic
                && (value.get("title").and_then(Value::as_str))
                    .and_then(super::topics::slug)
                    .is_none();
            if shape.validate(value).is_err() || untitled {
                return Err(MapError::Entry(key.clone()));
            }
        }
        Ok(Self { map })
    }
}

/// Rewrites the record `value`, whose file is named `name` without its
/// extension, in place. `mount` gives the GitHub `owner/name` of the
/// repository a board mounts at `/repos/<name>`.
pub(crate) fn record(
    value: &mut Value,
    name: &str,
    context: &Context,
    mount: &dyn Fn(&str) -> Option<String>,
) -> Vec<Outcome> {
    let Some(kind) = value.get("$type").and_then(Value::as_str) else {
        return Vec::new();
    };
    let kind = kind.to_owned();
    let Some(record) = value.as_object_mut() else {
        return Vec::new();
    };
    let mut outcomes = Vec::new();
    match kind.as_str() {
        "plans/question/v1" => renamed(
            record,
            "plans/question/v1.session",
            true,
            context,
            &mut outcomes,
        ),
        "plans/decision/v1" => renamed(
            record,
            "plans/decision/v1.session",
            false,
            context,
            &mut outcomes,
        ),
        "artifacts/version/v1" => version(record, context, &mut outcomes),
        "plans/answer/v1" => optional(record, "by", "plans/answer/v1.by", context, &mut outcomes),
        "runs/run/v1" => optional(record, "by", "runs/run/v1.by", context, &mut outcomes),
        "review/report/v1" => optional(record, "by", "review/report/v1.by", context, &mut outcomes),
        "tasks/task/v2" => owner(record, context, &mut outcomes),
        "artifacts/artifact/v1" => {
            if let Some(Value::String(email)) = record.get("account") {
                let person = json!({"person": email});
                record.insert("account".into(), person);
                outcomes.push(Outcome::Changed("artifacts/artifact/v1.account"));
            }
        }
        "evals/eval/v1" => subjects(
            record,
            ("evals/eval/v1.subjects", name, false),
            context,
            mount,
            &mut outcomes,
        ),
        "runs/suite/v1" => subjects(
            record,
            ("runs/suite/v1.subjects", name, true),
            context,
            mount,
            &mut outcomes,
        ),
        "requests/reply/v1" => digest(record, "effect", "requests/reply/v1.effect", &mut outcomes),
        "boundary/crossing/v1" => {
            digest(
                record,
                "digest",
                "boundary/crossing/v1.digest",
                &mut outcomes,
            );
            digest(
                record,
                "override",
                "boundary/crossing/v1.override",
                &mut outcomes,
            );
        }
        _ => {}
    }
    outcomes
}

/// A task's owner, which becomes an actor. An owner holds a claim, so a
/// blank one is left for a person to fix rather than dropped, which would
/// free the task.
fn owner(record: &mut Map<String, Value>, context: &Context, outcomes: &mut Vec<Outcome>) {
    let Some(Value::String(owner)) = record.get("owner") else {
        return;
    };
    match actor(owner, context) {
        Some(by) => {
            record.insert("owner".into(), by);
            outcomes.push(Outcome::Changed("tasks/task/v2.owner"));
        }
        None => outcomes.push(Outcome::Unmapped("tasks/task/v2.owner", owner.clone())),
    }
}

/// A version's `session`, which becomes `by`, and its hash.
fn version(record: &mut Map<String, Value>, context: &Context, outcomes: &mut Vec<Outcome>) {
    // The released artifacts mod wrote the session's bare uuid, which
    // names no `local_` session. The mod is the program that wrote it, and
    // the uuid is kept as the agent's name so nothing is lost.
    if let Some(Value::String(session)) = record.remove("session") {
        let by = (actor(&session, context).filter(|by| by.get("session").is_some()))
            .unwrap_or_else(|| json!({"program": "claude-code", "agent": session}));
        record.insert("by".into(), by);
        outcomes.push(Outcome::Changed("artifacts/version/v1.session"));
    }
    digest(record, "sha256", "artifacts/version/v1.sha256", outcomes);
}

/// A required actor stored as the string `session`, which becomes `by`.
/// When `session` is true the actor must hold a session, so one without is
/// left for a person to map.
fn renamed(
    record: &mut Map<String, Value>,
    field: &'static str,
    session: bool,
    context: &Context,
    outcomes: &mut Vec<Outcome>,
) {
    let Some(Value::String(text)) = record.get("session") else {
        return;
    };
    match actor(text, context).filter(|by| !session || by.get("session").is_some()) {
        Some(by) => {
            record.remove("session");
            record.insert("by".into(), by);
            outcomes.push(Outcome::Changed(field));
        }
        None => outcomes.push(Outcome::Unmapped(field, text.clone())),
    }
}

/// An optional actor stored as a string in `key`, which goes only when it
/// is blank.
fn optional(
    record: &mut Map<String, Value>,
    key: &str,
    field: &'static str,
    context: &Context,
    outcomes: &mut Vec<Outcome>,
) {
    let Some(Value::String(text)) = record.get(key) else {
        return;
    };
    if let Some(by) = actor(text, context) {
        record.insert(key.into(), by);
        outcomes.push(Outcome::Changed(field));
    } else {
        outcomes.push(Outcome::Dropped(field, text.clone()));
        record.remove(key);
    }
}

/// The subjects of the record named `name`: each that is a `Subject`
/// stays, the map or a mount's repository replaces the rest, and a repeat
/// goes. A record is not its own subject, so its own name goes, and a
/// suite left with no subject has none. An eval must have a subject, so an
/// eval whose only subject is its own name keeps it, and like any other
/// name that nothing maps it becomes a topic.
fn subjects(
    record: &mut Map<String, Value>,
    (field, name, optional): (&'static str, &str, bool),
    context: &Context,
    mount: &dyn Fn(&str) -> Option<String>,
    outcomes: &mut Vec<Outcome>,
) {
    let Some(Value::Array(subjects)) = record.get_mut("subjects") else {
        return;
    };
    let own = |entry: &Value| entry.as_str() == Some(name) && !context.map.contains_key(name);
    let only = subjects.iter().all(own);
    if subjects.iter().any(own) && (optional || !only) {
        subjects.retain(|entry| !own(entry));
        outcomes.push(Outcome::Changed(field));
    }
    if subjects.is_empty() {
        record.remove("subjects");
        return;
    }
    for entry in subjects.iter_mut() {
        let Value::String(text) = entry else {
            continue;
        };
        if subject(text) {
            continue;
        }
        let given = context.map.get(text.as_str());
        // A name the map gives a topic for, or maps to no subject and no
        // mount resolves, becomes a topic of its own.
        let topic = |slug: String, given: Option<&Value>| Outcome::Topic {
            slug,
            name: text.clone(),
            given: given.cloned(),
        };
        let (mapped, made) = match given {
            Some(Value::Object(object)) if object.contains_key("$type") => {
                let title = object
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let slug = super::topics::slug(title).unwrap_or_default();
                (Some(format!("/topics/{slug}")), Some(topic(slug, given)))
            }
            Some(Value::String(mapped)) => {
                let made = super::topics::named(mapped).map(|slug| topic(slug.to_owned(), None));
                (Some(mapped.clone()), made)
            }
            _ => match mount(text) {
                Some(mapped) => (Some(mapped), None),
                None => match super::topics::slug(text) {
                    Some(slug) => (Some(format!("/topics/{slug}")), Some(topic(slug, None))),
                    None => (None, None),
                },
            },
        };
        match mapped {
            Some(mapped) => {
                *entry = Value::String(mapped);
                outcomes.push(Outcome::Changed(field));
                outcomes.extend(made);
            }
            None => outcomes.push(Outcome::Unmapped(field, text.clone())),
        }
    }
    // A name the map or a mount resolved may repeat a subject already there.
    let mut kept = Vec::new();
    subjects.retain(|entry| {
        let first = !kept.contains(entry);
        if first {
            kept.push(entry.clone());
        }
        first
    });
}

/// A hash in `key` that is bare hex, which gains its `sha256:` prefix. A
/// value that is neither bare hex nor a digest, such as `sha256:ab`, is
/// reported.
fn digest(
    record: &mut Map<String, Value>,
    key: &str,
    field: &'static str,
    outcomes: &mut Vec<Outcome>,
) {
    let Some(text) = record.get(key).and_then(Value::as_str) else {
        return;
    };
    if text.strip_prefix("sha256:").is_some_and(hex) {
        return;
    }
    if !hex(text) {
        outcomes.push(Outcome::Unmapped(field, text.to_owned()));
        return;
    }
    let bare = text.to_owned();
    let prefixed = format!("sha256:{bare}");
    record.insert(key.into(), Value::String(prefixed));
    outcomes.push(Outcome::Changed(field));
}

/// The actor the old text names, as [`Actor::from_text`] reads it. The
/// map's entry for the text wins when it is an actor; an entry that gives a
/// topic is for a subject, so the text is read as it is.
pub(crate) fn actor(text: &str, context: &Context) -> Option<Value> {
    let mapped = context.map.get(text);
    let topic = mapped.and_then(|value| value.get("$type")).is_some();
    if let Some(mapped) = mapped.filter(|value| value.is_object() && !topic) {
        return Some(mapped.clone());
    }
    Actor::from_text(text).map(|actor| json!(actor))
}

/// Whether `text` is a `Subject`: an `owner/name` repository or a board path.
pub(crate) fn subject(text: &str) -> bool {
    if let Some(path) = text.strip_prefix('/') {
        return !path.is_empty() && !text.chars().any(char::is_whitespace);
    }
    let Some((owner, name)) = text.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && !name.is_empty()
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The `owner/name` of a GitHub remote URL.
pub(crate) fn github(url: &str) -> Option<String> {
    let (_, path) = url
        .split_once("github.com/")
        .or_else(|| url.split_once("github.com:"))?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    subject(path).then(|| path.to_owned())
}

fn hex(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

/// Rewrites the `owner:` line of a `tasks/task/v2` Markdown record's
/// frontmatter, or `None` when the text has nothing to change.
pub(crate) fn markdown(text: &str, context: &Context) -> Option<(String, Vec<Outcome>)> {
    let body = text.strip_prefix("---\n")?;
    let end = body.find("\n---")?;
    let (front, rest) = body.split_at(end + 1);
    let task = front.lines().any(|line| {
        line.strip_prefix("$type:")
            .is_some_and(|kind| kind.trim().trim_matches('"') == "tasks/task/v2")
    });
    if !task {
        return None;
    }
    let field = "tasks/task/v2.owner";
    let mut outcomes = Vec::new();
    let mut lines = String::new();
    for line in front.lines() {
        let Some(owner) = line.strip_prefix("owner:").map(str::trim) else {
            lines.push_str(line);
            lines.push('\n');
            continue;
        };
        if owner.starts_with('{') || owner.is_empty() || owner == "null" {
            lines.push_str(line);
            lines.push('\n');
            continue;
        }
        let owner = serde_json::from_str::<String>(owner).unwrap_or_else(|_| owner.to_owned());
        if let Some(by) = actor(&owner, context) {
            let _ = writeln!(lines, "owner: {by}");
            outcomes.push(Outcome::Changed(field));
        } else {
            lines.push_str(line);
            lines.push('\n');
            outcomes.push(Outcome::Unmapped(field, owner));
        }
    }
    (!outcomes.is_empty()).then(|| (format!("---\n{lines}{rest}"), outcomes))
}
