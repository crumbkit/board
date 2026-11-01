//! `ref`: record one pull request or issue, asking GitHub only when it is
//! not yet recorded.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::gh;
use crate::objects::{Held, Kind, Object, held};

/// Record one pull request or issue.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RefInput")]
pub(crate) struct Input {
    /// The pull request or issue as owner/repo#number, such as octo/widgets#12.
    #[schemars(regex(pattern = crate::objects::REF_PATTERN))]
    #[serde(rename = "ref")]
    reference: String,
}

/// The record for the reference, and whether this call wrote it.
///
/// `written` is false when the record was already there; nothing was asked of
/// GitHub and nothing was written. `title` is null when the record holds none.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RefOutput")]
pub(crate) struct Output {
    path: String,
    #[serde(rename = "type")]
    kind: Kind,
    written: bool,
    url: String,
    title: Option<String>,
}

/// Writes the record for `owner/repo#number` with an expect-none put, or
/// reports the one already there.
pub(crate) fn reference(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { reference } = PluginError::read_input("github", "ref", input)?;
    let Some(object) = Object::parse(&reference) else {
        return Err(PluginError {
            code: "github::call::bad_input".to_owned(),
            message: format!("{reference} is not owner/repo#number"),
            fields: Map::new(),
            hint: None,
        });
    };
    if let Some(Held { path, kind, record }) = held(host, &object)? {
        return Ok(answer(path, kind, &record, false));
    }
    let endpoint = object.endpoint();
    let Some(issue) = gh::api(host, &endpoint)? else {
        let Object {
            owner,
            repo,
            number,
        } = &object;
        let mut fields = Map::new();
        fields.insert("ref".to_owned(), Value::from(reference.as_str()));
        return Err(PluginError {
            code: "github::ref::not_found".to_owned(),
            message: format!(
                "GitHub has no pull request or issue {reference}, or gh's account cannot see it; \
                 check the number with `gh issue view {number} --repo {owner}/{repo}`"
            ),
            fields,
            hint: None,
        });
    };
    let Some(url) =
        (issue.get("html_url").and_then(Value::as_str)).filter(|url| url.starts_with("https://"))
    else {
        return Err(gh::failed(
            &endpoint,
            "answered without an https html_url",
            &format!("run `gh api {endpoint}` to see the answer"),
        ));
    };
    let kind = if issue.contains_key("pull_request") {
        Kind::Pull
    } else {
        Kind::Issue
    };
    let mut record = Map::new();
    record.insert("$type".to_owned(), Value::from(kind.record_type()));
    record.insert("owner".to_owned(), Value::from(object.owner.as_str()));
    record.insert("repo".to_owned(), Value::from(object.repo.as_str()));
    record.insert("number".to_owned(), Value::from(object.number));
    record.insert("url".to_owned(), Value::from(url));
    if let Some(title) =
        (issue.get("title").and_then(Value::as_str)).filter(|title| !title.is_empty())
    {
        record.insert("title".to_owned(), Value::from(title));
    }
    let path = object.path(kind);
    let line = json!({"path": path, "expect": "none", "record": record});
    (host.put(&[line]))
        .map_err(|error| PluginError::board_refused("github", "put --batch", &error.refusal()))?;
    Ok(answer(path, kind, &record, true))
}

/// What `ref` reports about the record at `path`.
fn answer(path: String, kind: Kind, record: &Map<String, Value>, written: bool) -> Value {
    let text = |name: &str| record.get(name).and_then(Value::as_str).map(str::to_owned);
    let output = Output {
        path,
        kind,
        written,
        url: text("url").unwrap_or_default(),
        title: text("title"),
    };
    serde_json::to_value(output).expect("the output is plain strings and booleans")
}
