//! How each old field becomes its new shape. These functions only change
//! values; `migrate` reads and writes the files.

use std::fmt::Write as _;

use serde_json::{Map, Value, json};

/// What happened to one field of one record.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The field was rewritten to its new shape.
    Changed(&'static str),
    /// An optional actor named nobody a reader can parse, so the field went.
    Dropped(&'static str, String),
    /// The old value maps to nothing, so it was left for a person to fix.
    Unmapped(&'static str, String),
}

/// The `--map` file: an old string to the value that replaces it, an actor
/// object or a subject string.
pub(crate) struct Context {
    map: Map<String, Value>,
}

impl Context {
    pub(crate) fn new(map: Value) -> Self {
        let Value::Object(map) = map else {
            return Self { map: Map::new() };
        };
        Self { map }
    }
}

/// Rewrites the record `value` in place. `mount` gives the GitHub
/// `owner/name` of the repository a board mounts at `/repos/<name>`.
pub(crate) fn record(
    value: &mut Value,
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
            "session",
            "plans/question/v1.session",
            context,
            &mut outcomes,
        ),
        "plans/decision/v1" => renamed(
            record,
            "session",
            "plans/decision/v1.session",
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
            "evals/eval/v1.subjects",
            context,
            mount,
            &mut outcomes,
        ),
        "runs/suite/v1" => subjects(
            record,
            "runs/suite/v1.subjects",
            context,
            mount,
            &mut outcomes,
        ),
        "board/seen/v1" => seen(record, &mut outcomes),
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

/// A task's owner, which becomes an actor. An owner holds a claim, so one
/// that names nobody is left for a person to map rather than dropped, which
/// would free the task.
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
    // names no `local_` session; the mod is the program that wrote it.
    if let Some(Value::String(session)) = record.remove("session") {
        let by = actor(&session, context).unwrap_or_else(|| json!({"program": "claude-code"}));
        record.insert("by".into(), by);
        outcomes.push(Outcome::Changed("artifacts/version/v1.session"));
    }
    digest(record, "sha256", "artifacts/version/v1.sha256", outcomes);
}

/// A seen record's effect, a path or a bare id, which becomes a digest.
fn seen(record: &mut Map<String, Value>, outcomes: &mut Vec<Outcome>) {
    let id = (record.get("effect").and_then(Value::as_str))
        .map(|effect| effect.rsplit('/').next().unwrap_or(effect).to_owned());
    let field = "board/seen/v1.effect";
    match (record.get("effect").and_then(Value::as_str), id) {
        (Some(effect), _) if effect.starts_with("sha256:") => {}
        (_, Some(id)) if hex(&id) => {
            record.insert("effect".into(), Value::String(format!("sha256:{id}")));
            outcomes.push(Outcome::Changed(field));
        }
        (Some(effect), _) => outcomes.push(Outcome::Unmapped(field, effect.to_owned())),
        (None, _) => {}
    }
}

/// A required actor stored as the string `old`, which becomes `by`.
fn renamed(
    record: &mut Map<String, Value>,
    old: &str,
    field: &'static str,
    context: &Context,
    outcomes: &mut Vec<Outcome>,
) {
    let Some(Value::String(text)) = record.get(old) else {
        return;
    };
    match actor(text, context) {
        Some(by) => {
            record.remove(old);
            record.insert("by".into(), by);
            outcomes.push(Outcome::Changed(field));
        }
        None => outcomes.push(Outcome::Unmapped(field, text.clone())),
    }
}

/// An optional actor stored as a string in `key`, which goes when it names
/// nobody.
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

fn subjects(
    record: &mut Map<String, Value>,
    field: &'static str,
    context: &Context,
    mount: &dyn Fn(&str) -> Option<String>,
    outcomes: &mut Vec<Outcome>,
) {
    let Some(Value::Array(subjects)) = record.get_mut("subjects") else {
        return;
    };
    for entry in subjects {
        let Value::String(text) = entry else {
            continue;
        };
        if subject(text) {
            continue;
        }
        let mapped = context
            .map
            .get(text.as_str())
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| mount(text));
        match mapped {
            Some(mapped) => {
                *entry = Value::String(mapped);
                outcomes.push(Outcome::Changed(field));
            }
            None => outcomes.push(Outcome::Unmapped(field, text.clone())),
        }
    }
}

/// A hash in `key` that is bare hex, which gains its `sha256:` prefix. A
/// value that is neither is reported.
fn digest(
    record: &mut Map<String, Value>,
    key: &str,
    field: &'static str,
    outcomes: &mut Vec<Outcome>,
) {
    let Some(text) = record.get(key).and_then(Value::as_str) else {
        return;
    };
    if text.starts_with("sha256:") {
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

/// The actor the old text names: its `local_` session, the program its
/// words name (Codex, or Claude and its subagents), and Luke. The map's
/// entry for the text wins.
pub(crate) fn actor(text: &str, context: &Context) -> Option<Value> {
    if let Some(mapped) = context.map.get(text).filter(|value| value.is_object()) {
        return Some(mapped.clone());
    }
    let mut actor = Map::new();
    let session = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .find(|word| {
            word.strip_prefix("local_").is_some_and(|id| {
                !id.is_empty() && id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f' | '-'))
            })
        });
    if let Some(session) = session {
        actor.insert("session".into(), json!(session.trim_end_matches('-')));
    }
    let words: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .collect();
    let named = |name: &str| words.iter().any(|word| word == name);
    let subagent = text.to_ascii_lowercase().contains("sub-agent");
    if named("codex") {
        actor.insert("program".into(), json!("codex"));
    } else if named("claude") || named("subagent") || subagent {
        actor.insert("program".into(), json!("claude-code"));
    }
    if named("luke") {
        actor.insert("person".into(), json!("luke"));
    }
    (!actor.is_empty()).then_some(Value::Object(actor))
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
