//! `get`: read one prompt at its pin and render it with the input values
//! given.

use std::collections::BTreeMap;

use board_model::Source;
use board_ops::{Host, Limit, PluginError, literal};
use board_plugin::shape::{Record, TREE_CODES};
use board_repos::{Listed, Repo, root, shown};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::prompts::{MARKS, query, refused};
use crate::records::{PromptRecord, RunsIn};
use crate::refusal;
use crate::render::{Values, body, checked, placed, render, unused};

/// The prompt to render, and its input values.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "GetInput")]
pub(crate) struct Input {
    /// The prompt's path in the board, such as /prompts/review-pr.md.
    #[schemars(regex(pattern = r"^/.*\.md$"))]
    pub(crate) path: String,
    #[schemars(
        description = "The value of each input by its name. Board reads stdin only when no flag \
                       is given, so give the whole input as JSON on stdin: {\"path\": \
                       \"/prompts/review-pr.md\", \"inputs\": {\"pr\": \"2140\"}}."
    )]
    pub(crate) inputs: Option<Values>,
    #[schemars(
        description = "Where the work starts, in place of the prompt's own runs_in; rendered \
                       with the inputs and checked the same way."
    )]
    pub(crate) runs_in: Option<RunsIn>,
}

/// The rendered prompt and where it came from.
///
/// `repo` is the repo that holds it. `blob` is the git blob id of its file,
/// and `source` the commit it was read at, or `worktree` for the board's own
/// tree. `unused_inputs` lists the declared inputs its body never names.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "GetOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) path: String,
    pub(crate) repo: String,
    pub(crate) title: String,
    pub(crate) purpose: String,
    pub(crate) runs_in: BTreeMap<String, String>,
    pub(crate) text: String,
    pub(crate) blob: String,
    pub(crate) source: String,
    pub(crate) unused_inputs: Vec<String>,
}

/// A file at the pin, as `board get` reads it.
struct Held {
    /// `worktree`, or the commit it was read from.
    source: String,
    /// Its path in that source.
    inner: String,
    blob: String,
    /// Its frontmatter; none when Board refused it.
    record: Option<Value>,
    text: String,
}

/// The file at `path` at the pin; none when nothing is there. Extra
/// leading slashes are read as one, as the Python plugin read them.
fn read(host: &dyn Host, path: &str) -> Result<Option<Held>, PluginError> {
    let own = Repo {
        root: "/".to_owned(),
        clone: None,
    };
    let file = own.read(host, "prompts", path.trim_start_matches('/'))?;
    let (Some(blob), Some(location)) = (file.blob, file.location) else {
        return Ok(None);
    };
    let source = match &location.board.source {
        Source::WorkingTree => "worktree".to_owned(),
        Source::Commit(commit) => commit.to_string(),
    };
    Ok(Some(Held {
        source,
        inner: location.path,
        blob,
        record: file.record,
        text: file.text.unwrap_or_default(),
    }))
}

pub(crate) fn get(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input("prompts", "get", input)?;
    let path = given.path.as_str();
    let Some(Held {
        source,
        inner,
        blob,
        record,
        text,
    }) = read(host, path)?
    else {
        let why = format!("nothing is at {path}; run `board ls` to see what is there");
        return Err(refusal("prompts::prompt::not_found", why, []));
    };
    let condition = format!(
        "e.source = {} AND e.path = {}",
        literal(&source),
        literal(&inner)
    );
    let read = query(host, &condition)?;
    let file = read.files.first().cloned().unwrap_or_default();
    let Some(repo) = read.roots.get(&source).cloned() else {
        let held = root(&file.at);
        let why = format!(
            "{path} is in {held}, which the board does not mark, and prompts come only from the \
             board's own tree and the mounts /{MARKS} names; move the prompt, or add the mount's \
             path to /{MARKS} if its pointer is in the board's own tree"
        );
        let fields = [("path", json!(path)), ("repo", json!(held))];
        return Err(refusal("prompts::prompt::outside_scope", why, fields));
    };
    let is_prompt = file.kind.as_str() == Some(PromptRecord::TYPE);
    if is_prompt && file.code.as_str() == Some("board::record::unknown_type") {
        return Err(no_schema(host, &repo, &read.found));
    }
    let under_prompts = inner
        .split('/')
        .rev()
        .skip(1)
        .any(|segment| segment == "prompts");
    let tree_code = (file.code.as_str()).is_none_or(|code| TREE_CODES.contains(&code));
    let record = (under_prompts && is_prompt && tree_code)
        .then(|| record.and_then(|record| serde_json::from_value::<PromptRecord>(record).ok()))
        .flatten();
    let Some(record) = record else {
        let why = format!(
            "{path} is not a valid {} under a prompts/ directory (type {}, code {}); run `board \
             check {path}` to see what is wrong",
            PromptRecord::TYPE,
            shown(&file.kind),
            shown(&file.code)
        );
        let fields = [("path", json!(path)), ("code", file.code.clone())];
        return Err(refusal("prompts::prompt::not_prompt", why, fields));
    };
    let declared = record.inputs.unwrap_or_default();
    let values = checked(&declared, &given.inputs.unwrap_or_default())?;
    let text = body(&text);
    let placed = placed(given.runs_in.as_ref().unwrap_or(&record.runs_in), &values)?;
    let (key, value) = placed.place();
    Ok(json!(Output {
        path: path.to_owned(),
        repo,
        title: record.title,
        purpose: record.purpose,
        runs_in: BTreeMap::from([(key.to_owned(), value.to_owned())]),
        text: render(text, &values),
        blob,
        source,
        unused_inputs: unused(&declared, text),
    }))
}

/// The refusal of a prompt in a mount whose repo lacks the prompt schema,
/// with the commands that install it. Board itself refuses a call on a
/// board whose own tree lacks it, so `repo` is a mount.
fn no_schema(host: &dyn Host, repo: &str, found: &[Listed]) -> PluginError {
    let sql = "SELECT repository FROM sources WHERE source = 'worktree'";
    let report = match host.query(sql, Limit::ALL) {
        Ok(report) => report,
        Err(error) => return refused("query", &error.refusal()),
    };
    let board = (report.rows.first().and_then(|row| row.first()))
        .and_then(Value::as_str)
        .map_or_else(|| "board".to_owned(), |root| format!("board -C {root}"));
    let clone = (found.iter())
        .find(|listed| listed.root == repo)
        .and_then(|listed| listed.clone.clone())
        .unwrap_or_else(|| format!("<the clone mounted at {repo}>"));
    let why = format!(
        "{repo} holds no schema for {}, so Board cannot check its prompts; run `board -C {clone} \
         plugin enable prompts`, push, then `{board} mount update {repo}`",
        PromptRecord::TYPE
    );
    let fields = [("repo", json!(repo)), ("type", json!(PromptRecord::TYPE))];
    refusal("prompts::repo::no_schema", why, fields)
}
