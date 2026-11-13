//! Pure parsers over the text that `board` and `git` print. Nothing here runs
//! a command or reads a file, so each one is tested against captured text.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use serde_json::Value;

use crate::time::parse_rfc3339;

/// Why a command's output could not be used.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    /// The text is not JSON of the expected shape.
    #[error("{what} is not the JSON board-top expects: {detail}")]
    Shape { what: &'static str, detail: String },
    /// Board printed a failure document.
    #[error("board refused with {code}: {message}")]
    Refused { code: String, message: String },
    /// The config file is not valid.
    #[error("{0}")]
    Config(String),
}

fn shape(what: &'static str, error: &impl std::fmt::Display) -> ParseError {
    ParseError::Shape {
        what,
        detail: error.to_string(),
    }
}

/// The payload of a `board` JSON report. A call report wraps its result in
/// `data.output`; a record or a plain result is the document itself; a failure
/// document becomes `ParseError::Refused`.
fn payload(text: &str, what: &'static str) -> Result<Value, ParseError> {
    let value: Value = serde_json::from_str(text).map_err(|error| shape(what, &error))?;
    let schema = value.get("schema").and_then(Value::as_str).unwrap_or("");
    if schema == "board/failure/v1" {
        let field = |name: &str| {
            value
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        return Err(ParseError::Refused {
            code: field("code"),
            message: field("message"),
        });
    }
    if schema == "board/call-report/v2"
        && let Some(output) = value.pointer("/data/output")
    {
        return Ok(output.clone());
    }
    Ok(value)
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value, what: &'static str) -> Result<T, ParseError> {
    serde_json::from_value(value).map_err(|error| shape(what, &error))
}

/// A change as `board changes open` lists it, or as its record reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct OpenChange {
    /// The change's logical path, such as `/changes/board-top`.
    #[serde(default)]
    pub path: String,
    /// The change's title.
    #[serde(default)]
    pub title: String,
    /// The repository mount the change is for.
    #[serde(default)]
    pub mount: String,
    /// The change's phase: plan, build, review, land, paused, done or dropped.
    #[serde(default)]
    pub phase: String,
    /// The task the change was started from.
    #[serde(default)]
    pub task: Option<String>,
}

/// The changes `board changes open` lists.
pub fn parse_changes_open(text: &str) -> Result<Vec<OpenChange>, ParseError> {
    #[derive(Deserialize)]
    struct Listing {
        changes: Vec<OpenChange>,
    }
    let listing: Listing = decode(
        payload(text, "`board changes open`")?,
        "`board changes open`",
    )?;
    Ok(listing.changes)
}

/// A change record read with `board get`, given the path it was read from.
pub fn parse_change_record(text: &str, path: &str) -> Result<OpenChange, ParseError> {
    let mut change: OpenChange = decode(payload(text, "a change record")?, "a change record")?;
    path.clone_into(&mut change.path);
    Ok(change)
}

/// An open question that blocks work, from `board plans open`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Question {
    /// The question's logical path.
    #[serde(default)]
    pub path: String,
    /// When it was asked, as an RFC 3339 stamp.
    #[serde(default)]
    pub asked: String,
    /// The work it blocks.
    #[serde(default)]
    pub blocks: Option<String>,
    /// The work it blocks, in the field newer questions use.
    #[serde(default)]
    pub blocked_work: Option<String>,
    /// The record that gives the question its context.
    #[serde(default)]
    pub context_path: Option<String>,
}

impl Question {
    /// Whether the question names the change at `path`.
    #[must_use]
    pub fn names(&self, path: &str) -> bool {
        let names = |field: &Option<String>| {
            field
                .as_deref()
                .is_some_and(|text| text == path || text.starts_with(&format!("{path}/")))
        };
        names(&self.blocks) || names(&self.blocked_work) || names(&self.context_path)
    }

    /// When the question was asked, in seconds since the epoch.
    #[must_use]
    pub fn asked_at(&self) -> Option<i64> {
        parse_rfc3339(&self.asked)
    }
}

/// The questions `board plans open` lists, and how many there are in all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Questions {
    /// The questions returned.
    pub list: Vec<Question>,
    /// Every open question, which may exceed `list`.
    pub total: usize,
}

/// The open questions `board plans open` lists.
pub fn parse_questions(text: &str) -> Result<Questions, ParseError> {
    #[derive(Deserialize)]
    struct Listing {
        questions: Vec<Question>,
        #[serde(default)]
        total: Option<usize>,
    }
    let listing: Listing = decode(payload(text, "`board plans open`")?, "`board plans open`")?;
    let total = listing.total.unwrap_or(listing.questions.len());
    Ok(Questions {
        list: listing.questions,
        total,
    })
}

/// A realm of the work map.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Realm {
    /// The realm's id.
    #[serde(default)]
    pub id: String,
    /// The realm's display name.
    #[serde(default)]
    pub title: String,
}

/// A project of the work map.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Project {
    /// The project's id.
    #[serde(default)]
    pub id: String,
    /// The id of the realm it belongs to.
    #[serde(default)]
    pub realm: String,
    /// The project's title.
    #[serde(default)]
    pub title: String,
    /// The project's state, such as active, queued or done.
    #[serde(default)]
    pub state: String,
    /// The changes that belong to it.
    #[serde(default)]
    pub changes: Vec<String>,
}

/// A finding of the work map: a kind and the records it names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Finding {
    /// The finding's kind, such as landed or stale.
    #[serde(default)]
    pub kind: String,
    /// The records it names.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// The newest `keeper/map/v1` record, with only the fields board-top reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct WorkMap {
    /// Realms, in display order.
    #[serde(default)]
    pub realms: Vec<Realm>,
    /// Projects, in display order.
    #[serde(default)]
    pub projects: Vec<Project>,
    /// Findings about stale, landed or otherwise misplaced work.
    #[serde(default)]
    pub findings: Vec<Finding>,
    /// Questions waiting on the owner, by realm.
    #[serde(default)]
    pub questions_waiting: BTreeMap<String, usize>,
}

/// The work map record. Unknown fields are ignored.
pub fn parse_map(text: &str) -> Result<WorkMap, ParseError> {
    decode(payload(text, "the work map")?, "the work map")
}

/// The paths of the open tasks in `board tasks tree`, once each.
pub fn parse_open_tasks(text: &str) -> Result<Vec<String>, ParseError> {
    fn collect(nodes: &[Value], found: &mut Vec<String>) {
        for node in nodes {
            let open = node.get("status").and_then(Value::as_str) == Some("open");
            if let (true, Some(path)) = (open, node.get("path").and_then(Value::as_str)) {
                found.push(path.to_owned());
            }
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                collect(children, found);
            }
        }
    }
    let value = payload(text, "`board tasks tree`")?;
    let roots = value
        .get("roots")
        .and_then(Value::as_array)
        .ok_or_else(|| shape("`board tasks tree`", &"no roots list"))?;
    let mut found = Vec::new();
    collect(roots, &mut found);
    found.sort();
    found.dedup();
    Ok(found)
}

/// One move of a change between phases, read from git history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseEvent {
    /// When the commit that moved it was made.
    pub at: i64,
    /// The phase before the move; `None` when the commit created the change.
    pub from: Option<String>,
    /// The phase after the move.
    pub to: String,
}

fn quoted_phase(line: &str) -> Option<String> {
    let value = line.trim_start().strip_prefix("\"phase\":")?;
    let value = value.trim_start().strip_prefix('"')?;
    Some(value[..value.find('"')?].to_owned())
}

/// The change name in a `changes/<name>.json` path, when it is one.
fn change_name(file: &str) -> Option<&str> {
    let name = file.strip_prefix("changes/")?.strip_suffix(".json")?;
    (!name.is_empty() && !name.contains('/')).then_some(name)
}

/// Phase moves, by change name, oldest first, from the output of
/// `git log --reverse -p -U0 --format=commit@%aI -- 'changes/*.json'`.
#[must_use]
pub fn parse_phase_log(text: &str) -> HashMap<String, Vec<PhaseEvent>> {
    let mut events: HashMap<String, Vec<PhaseEvent>> = HashMap::new();
    let (mut at, mut file, mut before, mut created) = (None, None::<String>, None::<String>, false);
    for line in text.lines() {
        if let Some(stamp) = line.strip_prefix("commit@") {
            at = parse_rfc3339(stamp.trim());
            file = None;
            before = None;
            created = false;
        } else if line.starts_with("diff --git ") {
            file = None;
            before = None;
            created = false;
        } else if let Some(path) = line.strip_prefix("+++ b/") {
            file = change_name(path).map(str::to_owned);
        } else if line == "--- /dev/null" {
            created = true;
        } else if line.starts_with("+++ ") || line.starts_with("--- ") {
            // A deleted file, or the old path of a changed one.
        } else if let Some(old) = line.strip_prefix('-')
            && let Some(phase) = quoted_phase(old)
        {
            before = Some(phase);
        } else if let Some(new) = line.strip_prefix('+')
            && let (Some(phase), Some(name), Some(at)) = (quoted_phase(new), &file, at)
        {
            let from = before.take();
            // A rewrite that leaves the phase as it was, or one that does not
            // show the old phase of a file that already existed, is not a move.
            if from.as_deref() != Some(phase.as_str()) && (created || from.is_some()) {
                events.entry(name.clone()).or_default().push(PhaseEvent {
                    at,
                    from,
                    to: phase,
                });
            }
        }
    }
    events
}

/// When each file was first added, by path, from the output of
/// `git log --reverse --diff-filter=A --format=commit@%aI --name-only`.
#[must_use]
pub fn parse_added_log(text: &str) -> HashMap<String, i64> {
    let mut added = HashMap::new();
    let mut at = None;
    for line in text.lines() {
        if let Some(stamp) = line.strip_prefix("commit@") {
            at = parse_rfc3339(stamp.trim());
        } else if let (false, Some(at)) = (line.is_empty(), at) {
            added.entry(line.to_owned()).or_insert(at);
        }
    }
    added
}

/// Board-top's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// How many open changes are allowed at once.
    pub limit: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { limit: 12 }
    }
}

/// The config file's text. A `limit` of at least one is the only setting.
pub fn parse_config(text: &str) -> Result<Config, ParseError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct File {
        limit: Option<usize>,
    }
    let file: File = toml::from_str(text).map_err(|error| ParseError::Config(error.to_string()))?;
    match file.limit {
        None => Ok(Config::default()),
        Some(0) => Err(ParseError::Config("limit must be at least 1".to_owned())),
        Some(limit) => Ok(Config { limit }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: &str = r#"{"changes":[{"flow":{"name":"development"},"kind":"build","mount":"/repos/board","path":"/changes/a","phase":"plan","prototype":null,"route":"unowned","task":"/repos/notes-personal/tasks/a.md","title":"A"},{"mount":"/repos/x","path":"/repos/other/changes/b","phase":"paused","task":null,"title":"B"}],"sync":{},"total":2,"truncated":false,"unreadable":0}"#;

    #[test]
    fn reads_the_open_changes_whether_wrapped_or_bare() {
        let bare = parse_changes_open(OPEN).unwrap();
        assert_eq!(bare.len(), 2);
        assert_eq!(bare[0].path, "/changes/a");
        assert_eq!(
            bare[0].task.as_deref(),
            Some("/repos/notes-personal/tasks/a.md")
        );
        assert_eq!(bare[1].task, None);
        let wrapped = format!(r#"{{"schema":"board/call-report/v2","data":{{"output":{OPEN}}}}}"#);
        assert_eq!(parse_changes_open(&wrapped).unwrap(), bare);
    }

    #[test]
    fn a_failure_document_carries_its_code() {
        let text =
            r#"{"schema":"board/failure/v1","code":"plans::board::refused","message":"timed out"}"#;
        assert_eq!(
            parse_questions(text),
            Err(ParseError::Refused {
                code: "plans::board::refused".to_owned(),
                message: "timed out".to_owned()
            })
        );
        assert!(matches!(
            parse_changes_open("not json"),
            Err(ParseError::Shape { .. })
        ));
    }

    #[test]
    fn a_question_names_a_change_by_any_field() {
        let text = r#"{"questions":[{"path":"/q/1","asked":"2026-11-13T10:00:00Z","blocks":"/changes/a"},{"path":"/q/2","asked":"2026-11-13T10:00:00Z","context_path":"/changes/b/plans/p1"}],"total":7}"#;
        let questions = parse_questions(text).unwrap();
        assert_eq!(questions.total, 7);
        assert!(questions.list[0].names("/changes/a"));
        assert!(!questions.list[0].names("/changes/ab"));
        assert!(questions.list[1].names("/changes/b"));
        assert_eq!(questions.list[0].asked_at(), Some(1_794_564_000));
    }

    #[test]
    fn reads_the_map_and_ignores_unknown_fields() {
        let text = r#"{"$type":"keeper/map/v1","future":1,"realms":[{"id":"board","title":"Board"}],"projects":[{"id":"p","realm":"board","title":"P","state":"active","changes":["/changes/a"],"extra":true}],"findings":[{"kind":"landed","paths":["/changes/a"]}],"questions_waiting":{"board":9,"other":35}}"#;
        let map = parse_map(text).unwrap();
        assert_eq!(map.projects[0].changes, ["/changes/a"]);
        assert_eq!(map.findings[0].kind, "landed");
        assert_eq!(map.questions_waiting.values().sum::<usize>(), 44);
    }

    #[test]
    fn lists_open_tasks_once_each_through_children() {
        let text = r#"{"schema":"board/call-report/v2","data":{"output":{"roots":[{"path":"/t/a.md","status":"open","children":[{"path":"/t/b.md","status":"open","children":[]},{"path":"/t/c.md","status":"done","children":[]}]},{"path":"/t/a.md","status":"open","children":[]}]}}}"#;
        assert_eq!(parse_open_tasks(text).unwrap(), ["/t/a.md", "/t/b.md"]);
    }

    const LOG: &str = "commit@2026-11-10T09:00:00Z
diff --git a/changes/a.json b/changes/a.json
new file mode 100644
--- /dev/null
+++ b/changes/a.json
@@ -0,0 +1,3 @@
+{
+  \"phase\": \"plan\",
+  \"flow\": \"x\"
commit@2026-11-11T09:00:00Z
diff --git a/changes/a.json b/changes/a.json
--- a/changes/a.json
+++ b/changes/a.json
@@ -5 +5 @@
-  \"phase\": \"plan\",
+  \"phase\": \"build\",
diff --git a/changes/a/source.json b/changes/a/source.json
--- a/changes/a/source.json
+++ b/changes/a/source.json
@@ -5 +5 @@
-  \"phase\": \"plan\",
+  \"phase\": \"build\",
commit@2026-11-12T09:00:00Z
diff --git a/changes/rewritten.json b/changes/rewritten.json
--- a/changes/rewritten.json
+++ b/changes/rewritten.json
@@ -5 +5 @@
-  \"phase\": \"done\",
+  \"phase\": \"done\",
diff --git a/changes/unknown.json b/changes/unknown.json
--- a/changes/unknown.json
+++ b/changes/unknown.json
@@ -5,0 +6 @@
+  \"phase\": \"done\",
";

    #[test]
    fn reads_phase_moves_oldest_first_and_skips_nested_files() {
        let events = parse_phase_log(LOG);
        assert_eq!(
            events.len(),
            1,
            "a rewrite that keeps the phase is not a move"
        );
        let a = &events["a"];
        assert_eq!(a.len(), 2);
        assert_eq!((a[0].from.clone(), a[0].to.as_str()), (None, "plan"));
        assert_eq!(
            (a[1].from.as_deref(), a[1].to.as_str()),
            (Some("plan"), "build")
        );
        assert!(a[0].at < a[1].at);
    }

    #[test]
    fn reads_when_files_were_added() {
        let text = "commit@2026-11-01T00:00:00Z\ntasks/a.md\n\ncommit@2026-11-02T00:00:00Z\ntasks/a.md\ntasks/b.md\n";
        let added = parse_added_log(text);
        assert_eq!(added["tasks/a.md"], 1_793_491_200);
        assert_eq!(added["tasks/b.md"], 1_793_577_600);
    }

    #[test]
    fn config_defaults_to_twelve_and_refuses_what_it_cannot_use() {
        assert_eq!(parse_config("").unwrap().limit, 12);
        assert_eq!(parse_config("limit = 5").unwrap().limit, 5);
        assert!(matches!(
            parse_config("limit = 0"),
            Err(ParseError::Config(_))
        ));
        assert!(matches!(
            parse_config("limit = \"x\""),
            Err(ParseError::Config(_))
        ));
        assert!(matches!(
            parse_config("limt = 5"),
            Err(ParseError::Config(_))
        ));
    }
}
