//! The `sync` and `drop` capabilities: a change ends when its pull request
//! closes, or when it is dropped by hand.
//!
//! `sync` reads each open change's pull request once with `gh api` and
//! writes every move in one commit, or, when gh fails, writes nothing.
//! `open` and `next` never run gh; they report the day of the last sync.

use std::fmt::Write as _;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use board_model::parse_json;
use board_ops::{Host, PluginError, Run, literal};
use board_plugin::child::ProcessError;
use board_plugin::shape::{Prose, Record};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::board::{self, readable, rows, text};
use crate::change::{OWN, SYNC_COMMAND, SYNC_PATH, parsed, read, referenced, today, write};
use crate::gates::open_task;
use crate::phases::final_phase;
use crate::records::{ChangeRecord, Phase, SyncRecord, TASK_TYPE};
use crate::wip::idle_action;
use crate::{PLUGIN, refusal, to_value};

/// A GitHub pull request's URL: its owner, repository and number.
static PULL_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/pull/([0-9]+)/?$")
        .expect("the pattern is a constant; the sync tests compile it")
});
/// How long one gh call may run.
const GH_SECONDS: Duration = Duration::from_secs(20);
/// How long all of a sync's gh calls may run together: less than the
/// capability's 600 seconds, so sync can say so.
const SYNC_SECONDS: Duration = Duration::from_secs(540);
/// What gh prints on stderr when GitHub answers 404, which it also answers
/// for a repository gh's account cannot see.
const NOT_FOUND: &str = "(HTTP 404)";
/// The default `limit` of `sync`'s entries (STANDARDS 23).
const SYNC_LIMIT: usize = 100;

/// Whether sync only reports, and how many entries it lists.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SyncInput")]
pub(crate) struct Input {
    #[serde(default)]
    #[schemars(description = "Report what sync would do, and write nothing.")]
    dry_run: bool,
    #[serde(default = "sync_limit")]
    #[schemars(
        range(min = 1, max = 10_000),
        description = "The most entries to list, 1 to 10,000; 100 when left out."
    )]
    limit: usize,
}

fn sync_limit() -> usize {
    SYNC_LIMIT
}

/// A change to end in dropped, and why.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DropInput {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(
        with = "Prose",
        length(min = 1),
        description = "Why the change is dropped, recorded on it."
    )]
    reason: String,
}

/// The phase the change left for dropped, and its task, which stays open
/// until it is closed.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct DropOutput {
    path: String,
    from: Phase,
    to: Phase,
    task: Option<String>,
}

/// What sync found for one change, and the phase it moved it `to`, or why
/// it left it in `code`.
#[derive(Debug, Serialize, JsonSchema)]
struct Entry {
    path: String,
    phase: Phase,
    to: Option<Phase>,
    status: &'static str,
    code: Option<&'static str>,
    pull_request: Option<String>,
    task: Option<String>,
    detail: String,
    command: Option<String>,
}

/// Each open change of the board's own and what sync did with it; `moved`
/// counts those it ended.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "SyncOutput")]
pub(crate) struct Output {
    dry_run: bool,
    changes: Vec<Entry>,
    total: usize,
    moved: usize,
    truncated: bool,
}

/// An entry with no move, code or command yet.
fn entry(path: &str, change: &ChangeRecord, status: &'static str, detail: String) -> Entry {
    Entry {
        path: path.to_owned(),
        phase: change.phase,
        to: None,
        status,
        code: None,
        pull_request: change.pull_request.clone(),
        task: change.task.clone(),
        detail,
        command: None,
    }
}

/// `changes::sync::gh_failed` for `gh api <endpoint>`, saying `why`;
/// nothing was written.
fn gh_failed(endpoint: &str, why: &str) -> PluginError {
    refusal(
        "changes::sync::gh_failed",
        format!(
            "`gh api {endpoint}` {why}, so sync wrote nothing; check gh on PATH and `gh auth \
             status`, then run `{SYNC_COMMAND}` again"
        ),
        [("endpoint", json!(endpoint))],
    )
}

/// What GitHub says of the pull request, or none when it answers 404; gh
/// runs at most until `deadline`.
fn pull_state(
    host: &dyn Host,
    (owner, repo, number): (&str, &str, &str),
    deadline: Instant,
) -> Result<Option<Map<String, Value>>, PluginError> {
    let endpoint = format!("repos/{owner}/{repo}/pulls/{number}");
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(gh_failed(
            &endpoint,
            &format!(
                "was not run because sync's gh calls took {} s in all",
                SYNC_SECONDS.as_secs()
            ),
        ));
    }
    let arguments = ["api".to_owned(), endpoint.clone()];
    let run = Run {
        program: "gh",
        arguments: &arguments,
        directory: None,
        input: b"",
        deadline: left.min(GH_SECONDS),
        added: &[],
    };
    let exited = match host.run(&run) {
        Ok(exited) => exited,
        Err(ProcessError::Timeout { .. }) => {
            return Err(gh_failed(&endpoint, "ran past its time limit"));
        }
        Err(error) => return Err(gh_failed(&endpoint, &format!("could not run: {error}"))),
    };
    let stderr = String::from_utf8_lossy(&exited.stderr).trim().to_owned();
    if exited.code != Some(0) {
        if stderr.contains(NOT_FOUND) {
            return Ok(None);
        }
        let code = exited
            .code
            .map_or_else(|| "on a signal".to_owned(), |code| code.to_string());
        return Err(gh_failed(&endpoint, &format!("exited {code}: {stderr}")));
    }
    match parse_json(&exited.stdout) {
        Ok(Value::Object(answer)) => Ok(Some(answer)),
        _ => Err(gh_failed(&endpoint, "printed no JSON object")),
    }
}

/// Each change of the board's own that a closing pull request can end, with
/// its blob, by path.
fn open_changes(host: &dyn Host) -> Result<Vec<(String, ChangeRecord, String)>, PluginError> {
    let sql = format!(
        "SELECT logical, blob, code, body FROM records WHERE type = {} AND json_extract(body, \
         '$.phase') IN ('build', 'review', 'land', 'paused') AND substr(logical, 1, {}) = {} \
         ORDER BY logical",
        literal(ChangeRecord::TYPE),
        OWN.len(),
        literal(OWN)
    );
    Ok((rows(host, &sql)?.iter())
        .filter(|row| readable(row.get(2)))
        .filter_map(|row| {
            let change = parsed(row.get(3))?;
            Some((
                text(row.first())?.to_owned(),
                change,
                text(row.get(1))?.to_owned(),
            ))
        })
        .collect())
}

/// The tasks the change references in `records`, through any link, that are
/// neither done nor dropped.
fn open_tasks(host: &dyn Host, change: &ChangeRecord) -> Result<Vec<String>, PluginError> {
    let records = referenced(host, change.records())?;
    Ok((records.of_type(TASK_TYPE))
        .filter(|(_, body)| open_task(body))
        .map(|(path, _)| path.to_owned())
        .collect())
}

/// The entry of a change that records no pull request: what to do with it.
fn without_pull(path: &str, change: &ChangeRecord) -> Entry {
    let action = idle_action(path, change);
    let detail = format!(
        "it records no pull request; {} it, or record its pull request with `board call \
         changes.attach --path {path} --pull_request <url>`",
        action.action.as_str()
    );
    Entry {
        code: Some("changes::sync::no_pull_request"),
        command: Some(action.command),
        ..entry(path, change, "no_pull_request", detail)
    }
}

/// A merged pull request's entry, and the change ended in done, or none
/// while a task is still open.
fn merged(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    url: &str,
    answer: &Map<String, Value>,
) -> Result<(Entry, Option<ChangeRecord>), PluginError> {
    let owed = open_tasks(host, change)?;
    if !owed.is_empty() {
        let detail = format!(
            "its pull request merged, but these tasks are not done or dropped: {}; close them \
             with `board call tasks.done`, then run `{SYNC_COMMAND}` again",
            owed.join(", ")
        );
        let owing = Entry {
            code: Some("changes::sync::owed"),
            command: Some(SYNC_COMMAND.to_owned()),
            ..entry(path, change, "owed", detail)
        };
        return Ok((owing, None));
    }
    let mut ended = change.clone();
    ended.phase = Phase::Done;
    let mut detail = format!("its pull request {url} merged");
    let sha = answer.get("merge_commit_sha").and_then(Value::as_str);
    match sha.filter(|sha| {
        sha.len() == 40
            && sha
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    }) {
        Some(sha) => {
            let repository = url.split("/pull/").next().unwrap_or(url);
            ended.merge_commit_url = Some(format!("{repository}/commit/{sha}"));
        }
        None => detail.push_str(", and GitHub gave no merge commit, so none is recorded"),
    }
    let skipped = match change.phase {
        Phase::Build => Some("build and review"),
        Phase::Review => Some("review"),
        _ => None,
    };
    if let Some(skipped) = skipped {
        let _ = write!(
            detail,
            "; it left {} without passing the {skipped} gates",
            change.phase.as_str()
        );
    }
    let done = Entry {
        to: Some(Phase::Done),
        ..entry(path, change, "merged", detail)
    };
    Ok((done, Some(ended)))
}

/// What the change's pull request says, and the change to write when it
/// closed, or none to leave it.
fn synced(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    deadline: Instant,
) -> Result<(Entry, Option<ChangeRecord>), PluginError> {
    let url = change.pull_request.clone().unwrap_or_default();
    let matched = PULL_URL.captures(&url).and_then(|captures| {
        Some((
            captures.get(1)?.as_str(),
            captures.get(2)?.as_str(),
            captures.get(3)?.as_str(),
        ))
    });
    let Some(pull) = matched else {
        let detail = format!(
            "{url} is not a GitHub pull request URL, so its state cannot be read; record the \
             right URL"
        );
        let attach = format!("board call changes.attach --path {path} --pull_request <url>");
        let unread = Entry {
            code: Some("changes::sync::not_github"),
            command: Some(attach),
            ..entry(path, change, "not_github", detail)
        };
        return Ok((unread, None));
    };
    let Some(answer) = pull_state(host, pull, deadline)? else {
        let detail = format!(
            "GitHub answers 404 for {url}: no such pull request, or gh's account cannot see the \
             repository; check `gh auth status`, then drop the change or record the right URL"
        );
        let missing = Entry {
            code: Some("changes::sync::pull_not_found"),
            command: Some(format!(
                "board call changes.drop --path {path} --reason '<why>'"
            )),
            ..entry(path, change, "not_found", detail)
        };
        return Ok((missing, None));
    };
    if answer.get("merged") == Some(&Value::Bool(true)) {
        return merged(host, path, change, &url, &answer);
    }
    if answer.get("state").and_then(Value::as_str) == Some("closed") {
        let closed: String = (answer.get("closed_at").and_then(Value::as_str))
            .unwrap_or_default()
            .chars()
            .take(10)
            .collect();
        let on = if closed.is_empty() {
            String::new()
        } else {
            format!(" on {closed}")
        };
        let reason = format!("its pull request {url} was closed without merging{on}");
        let mut ended = change.clone();
        ended.phase = Phase::Dropped;
        ended.dropped_reason = Some(reason.clone());
        let dropped = Entry {
            to: Some(Phase::Dropped),
            ..entry(path, change, "closed", reason)
        };
        return Ok((dropped, Some(ended)));
    }
    let open = entry(
        path,
        change,
        "open",
        format!("its pull request {url} is open"),
    );
    Ok((open, None))
}

/// Reads each open change's pull request and ends the change when it
/// closed: done when merged, else dropped. With `dry_run`, reports the same
/// and writes nothing. Otherwise every move and the day of the sync are
/// written in one commit.
pub(crate) fn sync(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { dry_run, limit } = PluginError::read_input(PLUGIN, "sync", input)?;
    let deadline = Instant::now() + SYNC_SECONDS;
    let mut entries = Vec::new();
    let mut lines = Vec::new();
    for (path, change, blob) in open_changes(host)? {
        if change.pull_request.is_none() {
            entries.push(without_pull(&path, &change));
            continue;
        }
        let (found, ended) = synced(host, &path, &change, deadline)?;
        entries.push(found);
        if let Some(ended) = ended {
            lines.push(json!({"path": path, "expect": blob, "record": ended}));
        }
    }
    if !dry_run {
        let sql = format!(
            "SELECT blob FROM entries WHERE logical = {}",
            literal(SYNC_PATH)
        );
        let found = rows(host, &sql)?;
        let expect = (found.first().and_then(|row| text(row.first()))).unwrap_or("none");
        let record = SyncRecord {
            tag: board_plugin::shape::Tag::default(),
            synced: today(host).to_string(),
        };
        lines.push(json!({"path": SYNC_PATH, "expect": expect, "record": record}));
        board::put(host, &lines)?;
    }
    let total = entries.len();
    let moved = entries.iter().filter(|found| found.to.is_some()).count();
    entries.truncate(limit);
    Ok(to_value(Output {
        dry_run,
        changes: entries,
        total,
        moved,
        truncated: total > limit,
    }))
}

/// Ends a change that is not done or dropped in `dropped`, recording why.
pub(crate) fn drop(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let DropInput { path, reason } = PluginError::read_input(PLUGIN, "drop", input)?;
    let (mut change, blob) = read(host, &path)?;
    let phase = change.phase;
    if phase.is_final() {
        return Err(final_phase(
            &path,
            phase,
            format!(
                "the change at {path} is already {}, so it cannot be dropped; list the open \
                 changes with `board call changes.open`",
                phase.as_str()
            ),
        ));
    }
    change.phase = Phase::Dropped;
    change.dropped_reason = Some(reason);
    write(host, &path, &blob, &change)?;
    Ok(to_value(DropOutput {
        path,
        from: phase,
        to: Phase::Dropped,
        task: change.task,
    }))
}

#[cfg(test)]
mod tests {
    use super::PULL_URL;

    #[test]
    fn a_pull_request_url_gives_its_owner_repository_and_number() {
        let captures = PULL_URL
            .captures("https://github.com/crumbkit/board/pull/12/")
            .unwrap();
        assert_eq!(&captures[1], "crumbkit");
        assert_eq!(&captures[2], "board");
        assert_eq!(&captures[3], "12");
        assert!(!PULL_URL.is_match("https://github.com/crumbkit/board/issues/12"));
    }
}
