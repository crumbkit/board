//! `ref`: write one Jira issue as a `jira/issue/v1` record, with the title
//! acli reads, unless a record is already there.

use std::io;
use std::time::Duration;

use board_model::{Code, LogicalPath};
use board_ops::{Host, PluginError, Run};
use board_plugin::child::{Exited, ProcessError};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::IssueRecord;

/// How long acli may take to read one work item.
const ACLI_SECONDS: u64 = 10;
const NO_SUMMARY: &str = "acli printed no JSON work item with a summary";

/// The issue to record.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RefInput")]
pub(crate) struct Input {
    /// The issue key, such as ACME-123.
    key: String,
    /// The site name before .atlassian.net, such as acme.
    site: String,
}

/// The issue's record, whether this call wrote it, and why it has no title
/// when it has none.
///
/// `title_note` is null when the record has a title or was already there.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RefOutput")]
pub(crate) struct Output {
    path: String,
    site: String,
    key: String,
    url: String,
    title: Option<String>,
    written: bool,
    title_note: Option<String>,
}

/// Writes the issue at `/jira/<site>/<key>` unless a record is there, and
/// reports the record.
pub(crate) fn reference(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { key, site } = PluginError::read_input("jira", "ref", input)?;
    if !is_key(&key) {
        return Err(refusal(
            "jira::key::invalid",
            format!(
                "'{key}' is not a Jira issue key, which is a project key in capitals, a hyphen \
                 and a number; pass a key such as ACME-123"
            ),
            "key",
            &key,
        ));
    }
    if !is_site(&site) {
        return Err(refusal(
            "jira::site::invalid",
            format!(
                "'{site}' is not a Jira site name, the lowercase name before .atlassian.net; \
                 pass a site such as acme for acme.atlassian.net"
            ),
            "site",
            &site,
        ));
    }
    let path = format!("/jira/{site}/{key}");
    if let Some(held) = read(host, &path)? {
        return Ok(reported(path, &held, false, None));
    }
    let (title, note) = title_of(&run_acli(host, &key), &site);
    let mut record = json!({
        "$type": IssueRecord::TYPE,
        "site": site,
        "key": key,
        "url": url_of(&site, &key),
    });
    if let Some(title) = title {
        record["title"] = json!(title);
    }
    let line = json!({"path": path, "expect": "none", "record": record});
    (host.put(&[line]))
        .map_err(|error| PluginError::board_refused("jira", "put --batch", &error.refusal()))?;
    Ok(reported(path, &record, true, note))
}

fn refusal(code: &str, message: String, field: &str, value: &str) -> PluginError {
    let mut fields = Map::new();
    fields.insert(field.to_owned(), Value::from(value));
    PluginError {
        code: code.to_owned(),
        message,
        fields,
    }
}

/// Whether `key` is a project key, a hyphen and an issue number, as Jira
/// writes keys: `[A-Z][A-Z0-9_]+-[1-9][0-9]*`, the record schema's pattern.
pub(crate) fn is_key(key: &str) -> bool {
    let Some((project, number)) = key.split_once('-') else {
        return false;
    };
    let (project, number) = (project.as_bytes(), number.as_bytes());
    project.len() >= 2
        && project[0].is_ascii_uppercase()
        && (project.iter())
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
        && number
            .first()
            .is_some_and(|byte| matches!(byte, b'1'..=b'9'))
        && number.iter().all(u8::is_ascii_digit)
}

/// Whether `site` is the name before .atlassian.net in a Jira Cloud site's
/// address: `[a-z0-9]([a-z0-9-]*[a-z0-9])?`, the record schema's pattern.
pub(crate) fn is_site(site: &str) -> bool {
    let end = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let bytes = site.as_bytes();
    bytes.first().is_some_and(end)
        && bytes.last().is_some_and(end)
        && bytes.iter().all(|byte| end(byte) || *byte == b'-')
}

/// The address of the issue's page.
fn url_of(site: &str, key: &str) -> String {
    format!("https://{site}.atlassian.net/browse/{key}")
}

/// The record at `path`, or none when nothing is there.
fn read(host: &dyn Host, path: &str) -> Result<Option<Value>, PluginError> {
    let path: LogicalPath = path
        .parse()
        .expect("a valid key and site make a logical path");
    let answers = (host.get_each(&[path]))
        .map_err(|error| PluginError::board_refused("jira", "get", &error.refusal("get")))?;
    match answers.into_iter().next() {
        Some(Ok(report)) => (report.record)
            .map(Some)
            .map_err(|refusal| PluginError::board_refused("jira", "get", &refusal)),
        Some(Err(error)) if error.code() == Code::PathNotFound => Ok(None),
        Some(Err(error)) => Err(PluginError::board_refused(
            "jira",
            "get",
            &error.refusal("get"),
        )),
        None => Ok(None),
    }
}

/// The output: the record's fields, whether this call wrote it, and why it
/// has no title.
fn reported(path: String, record: &Value, written: bool, note: Option<String>) -> Value {
    let text = |name: &str| record[name].as_str().unwrap_or_default().to_owned();
    let output = Output {
        site: text("site"),
        key: text("key"),
        url: text("url"),
        title: record["title"].as_str().map(str::to_owned),
        path,
        written,
        title_note: note,
    };
    serde_json::to_value(output).expect("the output is plain strings and booleans")
}

/// Runs `acli jira workitem view <key> --fields summary --json`, found on
/// the PATH a process plugin would see, so a test controls which acli runs,
/// or that none does, through that PATH.
fn run_acli(host: &dyn Host, key: &str) -> Result<Exited, ProcessError> {
    let arguments = [
        "jira", "workitem", "view", key, "--fields", "summary", "--json",
    ]
    .map(str::to_owned);
    host.run(&Run {
        program: "acli",
        arguments: &arguments,
        directory: None,
        input: b"",
        deadline: Duration::from_secs(ACLI_SECONDS),
    })
}

/// The issue's title from acli's run, or none and why there is none.
pub(crate) fn title_of(
    ran: &Result<Exited, ProcessError>,
    site: &str,
) -> (Option<String>, Option<String>) {
    let why = |note: String| (None, Some(note));
    match ran {
        Err(ProcessError::NotStarted { source }) if source.kind() == io::ErrorKind::NotFound => {
            why("acli is not on PATH, so the record has no title".to_owned())
        }
        Err(ProcessError::NotStarted { source }) => why(format!(
            "acli did not start ({source}), so the record has no title"
        )),
        Err(ProcessError::Timeout { .. }) => why(format!(
            "acli ran past {ACLI_SECONDS} s, so the record has no title"
        )),
        Err(ProcessError::TooLarge { limit, .. }) => why(format!(
            "acli printed more than {limit} bytes, so the record has no title"
        )),
        Ok(exited) if exited.code != Some(0) => {
            let stderr = String::from_utf8_lossy(&exited.stderr);
            let last: String = (stderr.trim().lines().last().unwrap_or_default())
                .chars()
                .take(300)
                .collect();
            let code = exited
                .code
                .map_or_else(|| "on a signal".to_owned(), |code| code.to_string());
            why(format!("acli exited {code}: {last}"))
        }
        Ok(exited) => title_from(&String::from_utf8_lossy(&exited.stdout), site),
    }
}

/// The summary in acli's JSON, or none and why. A work item read from
/// another site gives no title.
pub(crate) fn title_from(stdout: &str, site: &str) -> (Option<String>, Option<String>) {
    let no_summary = || (None, Some(NO_SUMMARY.to_owned()));
    let Ok(item) = serde_json::from_str::<Value>(stdout) else {
        return no_summary();
    };
    let Some(summary) = (item["fields"]["summary"].as_str()).filter(|summary| !summary.is_empty())
    else {
        return no_summary();
    };
    if let Some(source) = item["self"].as_str()
        && !source.starts_with(&format!("https://{site}.atlassian.net/"))
    {
        return (
            None,
            Some(format!("acli is signed in to another site ({source})")),
        );
    }
    (Some(summary.to_owned()), None)
}
