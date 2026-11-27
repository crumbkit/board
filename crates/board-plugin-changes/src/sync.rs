//! The `sync` and `drop` capabilities: a change ends when its pull request
//! closes, or when it is dropped by hand. `advance` into done reads the
//! pull request here too, through [`require_merged`].
//!
//! `sync` reads each open change's pull request once with `gh api` and
//! writes every move in one commit, or, when gh fails, writes nothing.
//! `open` and `next` never run gh; they report the day of the last sync.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use board_model::parse_json;
use board_ops::{Host, PluginError, Run, literal};
use board_plugin::child::ProcessError;
use board_plugin::shape::Prose;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::board::{self, readable, rows, text};
use crate::change::{
    OWN, Pull, SYNC_COMMAND, attached, change_types, parsed, pull_of, read, record_sync, today,
    write,
};
use crate::phases::final_phase;
use crate::records::{ChangeRecord, DONE, DROPPED, is_final};
use crate::tasks::{at_merge, close_at_merge};
use crate::wip::idle_action;
use crate::{PLUGIN, refusal, to_value};

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
    from: String,
    to: String,
    task: Option<String>,
}

/// What sync found for one change, and the phase it moved it `to`, or why
/// it left it in `code`.
#[derive(Debug, Serialize, JsonSchema)]
struct Entry {
    path: String,
    phase: String,
    to: Option<String>,
    status: &'static str,
    code: Option<&'static str>,
    pull_request: Option<String>,
    task: Option<String>,
    detail: String,
    command: Option<String>,
}

/// Each open change of the board's own and what sync did with it; `moved`
/// counts those it ended. `unrecorded` says why Board's cache did not take
/// the day of the sync, when it did not.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "SyncOutput")]
pub(crate) struct Output {
    dry_run: bool,
    changes: Vec<Entry>,
    total: usize,
    moved: usize,
    truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    unrecorded: Option<String>,
}

/// An entry with no move, code or command yet, for the pull request at
/// `url`.
fn entry(
    path: &str,
    change: &ChangeRecord,
    url: Option<&str>,
    status: &'static str,
    detail: String,
) -> Entry {
    Entry {
        path: path.to_owned(),
        phase: change.phase.clone(),
        to: None,
        status,
        code: None,
        pull_request: url.map(str::to_owned),
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

/// Why `gh api <endpoint>` gave no answer.
struct GhFailed {
    endpoint: String,
    why: String,
}

/// What GitHub says of the pull request, or none when it answers 404; gh
/// runs at most until `deadline`.
fn pull_state(
    host: &dyn Host,
    pull: &Pull,
    deadline: Instant,
) -> Result<Option<Map<String, Value>>, GhFailed> {
    let endpoint = format!("repos/{}/{}/pulls/{}", pull.owner, pull.repo, pull.number);
    let failed = |why: String| GhFailed {
        endpoint: endpoint.clone(),
        why,
    };
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(failed(format!(
            "was not run because sync's gh calls took {} s in all",
            SYNC_SECONDS.as_secs()
        )));
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
            return Err(failed("ran past its time limit".to_owned()));
        }
        Err(error) => return Err(failed(format!("could not run: {error}"))),
    };
    let stderr = String::from_utf8_lossy(&exited.stderr).trim().to_owned();
    if exited.code != Some(0) {
        if stderr.contains(NOT_FOUND) {
            return Ok(None);
        }
        let code = exited
            .code
            .map_or_else(|| "on a signal".to_owned(), |code| code.to_string());
        return Err(failed(format!("exited {code}: {stderr}")));
    }
    match parse_json(&exited.stdout) {
        Ok(Value::Object(answer)) => Ok(Some(answer)),
        _ => Err(failed("printed no JSON object".to_owned())),
    }
}

/// The page of the commit GitHub's `answer` gives as the pull request's
/// merge, or none when it gives no full commit.
fn merge_commit_url(pull: &Pull, answer: &Map<String, Value>) -> Option<String> {
    let sha = answer.get("merge_commit_sha").and_then(Value::as_str)?;
    let full = sha.len() == 40
        && sha
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    full.then(|| {
        format!(
            "https://github.com/{}/{}/commit/{sha}",
            pull.owner, pull.repo
        )
    })
}

/// Each change of the board's own that a closing pull request can end, with
/// its blob, by path.
fn open_changes(host: &dyn Host) -> Result<Vec<(String, ChangeRecord, String)>, PluginError> {
    let sql = format!(
        "SELECT logical, blob, code, body FROM records WHERE type IN ({}) AND json_extract(body, \
         '$.phase') NOT IN ('plan', 'done', 'dropped') AND substr(logical, 1, {}) = {} \
         ORDER BY logical",
        change_types(),
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

/// The entry of a change Board could not read what it depends on for, such
/// as a task beneath a mount that cannot be read. It is left as it is, and
/// the other changes sync. It has no command, because no one command fixes
/// a mount: `detail` gives the fix.
fn unread(path: &str, change: &ChangeRecord, error: &PluginError) -> Entry {
    let hint = error.hint.as_deref().unwrap_or("no next step is known");
    let detail = format!("{}; {hint}", error.message);
    Entry {
        code: Some("changes::sync::unread"),
        ..entry(path, change, None, "unread", detail)
    }
}

/// What sync does with a change whose read `error` refused: the change's
/// [`unread`] entry when what the tree holds along one of its references
/// stopped that reference, such as a mount that cannot be read, or else
/// `error`, which refuses the whole sync, as git or the index failing does.
fn unread_or_refused(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    error: PluginError,
) -> Result<Entry, PluginError> {
    let mut paths = attached(host, path)?;
    paths.extend(change.pulls().iter().cloned());
    match board::stopped(host, &paths) {
        Ok(stopped) => match stopped.first() {
            Some(refusal) => Ok(unread(path, change, &board::refused("refs", refusal))),
            None => Err(error),
        },
        Err(_) => Err(error),
    }
}

/// The entry of a change that records no pull request: what to do with it.
fn without_pull(path: &str, change: &ChangeRecord) -> Entry {
    let action = idle_action(path, change);
    let detail = format!(
        "it records no pull request; {} it, or record its pull request with `board \
         github ref --ref <owner>/<repo>#<number>` and attach it with `board \
         changes attach --path {path} --pull <record>`",
        action.action.as_str()
    );
    Entry {
        code: Some("changes::sync::no_pull_request"),
        command: Some(action.command),
        ..entry(path, change, None, "no_pull_request", detail)
    }
}

/// The `owed` entry of a change whose pull request at `url` merged but
/// that cannot end yet, for the reason `detail`.
fn owing(path: &str, change: &ChangeRecord, url: &str, detail: String) -> Entry {
    Entry {
        code: Some("changes::sync::owed"),
        command: Some(SYNC_COMMAND.to_owned()),
        ..entry(path, change, Some(url), "owed", detail)
    }
}

/// A merged pull request's entry, and the change ended in done, or none
/// while a task its plan does not close at merge is still open.
///
/// Each task the plan closes at merge closes first, through `tasks.done`,
/// unless `dry_run`; when one is refused, the change is owed and stays.
fn merged(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    pull: &Pull,
    answer: &Map<String, Value>,
    dry_run: bool,
) -> Result<(Entry, Option<ChangeRecord>), PluginError> {
    let url = pull.url.as_str();
    let tasks = at_merge(host, path, change)?;
    if !tasks.owed.is_empty() {
        let detail = format!(
            "its pull request merged, but these tasks are not done or dropped: {}; close them \
             with `board tasks done`, or name them in the plan's Closes at merge section, then \
             run `{SYNC_COMMAND}` again",
            tasks.owed.join(", ")
        );
        return Ok((owing(path, change, url, detail), None));
    }
    let closed = if dry_run {
        Vec::new()
    } else {
        match close_at_merge(host, path, &change.phase, &tasks.closes, SYNC_COMMAND) {
            Ok(closed) => closed,
            Err(refused) => return Ok((owing(path, change, url, refused.message), None)),
        }
    };
    let mut ended = change.clone();
    DONE.clone_into(&mut ended.phase);
    let mut detail = format!("its pull request {url} merged");
    if dry_run && !tasks.closes.is_empty() {
        let _ = write!(
            detail,
            ", and sync would close these tasks: {}",
            tasks.closes.join(", ")
        );
    } else if !closed.is_empty() {
        let _ = write!(detail, ", and it closed these tasks: {}", closed.join(", "));
    }
    ended.merge_commit_url = merge_commit_url(pull, answer);
    if ended.merge_commit_url.is_none() {
        detail.push_str(", and GitHub gave no merge commit, so none is recorded");
    }
    match change.phase.as_str() {
        "build" => detail.push_str("; it left build without passing the build and review gates"),
        "review" => detail.push_str("; it left review without passing the review gates"),
        "land" => {
            detail.push_str("; sync moved it from land to done without evaluating the land gates");
        }
        _ => {}
    }
    let done = Entry {
        to: Some(DONE.to_owned()),
        ..entry(path, change, Some(url), "merged", detail)
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
    dry_run: bool,
) -> Result<(Entry, Option<ChangeRecord>), PluginError> {
    let reference = change.pulls().last().cloned().unwrap_or_default();
    let Some(pull) = pull_of(host, &reference)? else {
        let detail = format!(
            "{reference} is not a github/pull/v1 record Board reads, so the pull request's state \
             cannot be read; record it with `board github ref` and attach it with --pull"
        );
        let attach = format!("board changes attach --path {path} --pull <record>");
        let unread = Entry {
            code: Some("changes::sync::pull_unreadable"),
            command: Some(attach),
            ..entry(path, change, None, "pull_unreadable", detail)
        };
        return Ok((unread, None));
    };
    let url = pull.url.clone();
    let answer = pull_state(host, &pull, deadline)
        .map_err(|failed| gh_failed(&failed.endpoint, &failed.why))?;
    let Some(answer) = answer else {
        let detail = format!(
            "GitHub answers 404 for {url}: no such pull request, or gh's account cannot see the \
             repository; check `gh auth status`, then drop the change or record the right URL"
        );
        let missing = Entry {
            code: Some("changes::sync::pull_not_found"),
            command: Some(format!("board changes drop --path {path} --reason '<why>'")),
            ..entry(path, change, Some(&url), "not_found", detail)
        };
        return Ok((missing, None));
    };
    if answer.get("merged") == Some(&Value::Bool(true)) {
        return merged(host, path, change, &pull, &answer, dry_run);
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
        DROPPED.clone_into(&mut ended.phase);
        ended.dropped_reason = Some(reason.clone());
        let dropped = Entry {
            to: Some(DROPPED.to_owned()),
            ..entry(path, change, Some(&url), "closed", reason)
        };
        return Ok((dropped, Some(ended)));
    }
    let open = entry(
        path,
        change,
        Some(&url),
        "open",
        format!("its pull request {url} is open"),
    );
    Ok((open, None))
}

/// Refuses to move the change at `path` into done until GitHub reports
/// its newest pull request merged, and then records the merge commit on
/// `change`, as sync does, and says true. A change that records no pull
/// request moves as before, and the answer is false: no merge happened.
pub(crate) fn require_merged(
    host: &dyn Host,
    path: &str,
    change: &mut ChangeRecord,
) -> Result<bool, PluginError> {
    let Some(reference) = change.pulls().last().cloned() else {
        return Ok(false);
    };
    let Some(pull) = pull_of(host, &reference)? else {
        return Err(not_merged(
            path,
            &reference,
            &format!(
                "{reference} is not a github/pull/v1 record Board reads, so whether it merged \
                 cannot be read; record it with `board github ref` and attach it with `board \
                 changes attach --path {path} --pull <record>`"
            ),
        ));
    };
    let url = pull.url.as_str();
    let answer = pull_state(host, &pull, Instant::now() + GH_SECONDS).map_err(|failed| {
        refusal(
            "changes::land::gh_failed",
            format!(
                "the change at {path} stays in {}: `gh api {}` {}, so whether {url} merged is \
                 not known; check gh on PATH and `gh auth status`, then advance again",
                change.phase, failed.endpoint, failed.why
            ),
            [("path", json!(path)), ("endpoint", json!(failed.endpoint))],
        )
    })?;
    let Some(answer) = answer else {
        return Err(not_merged(
            path,
            url,
            &format!(
                "GitHub answers 404 for {url}: no such pull request, or gh's account cannot see \
                 the repository; check `gh auth status` and the pull request the change records"
            ),
        ));
    };
    if answer.get("merged") == Some(&Value::Bool(true)) {
        change.merge_commit_url = merge_commit_url(&pull, &answer);
        return Ok(true);
    }
    let why = if answer.get("state").and_then(Value::as_str) == Some("closed") {
        format!(
            "GitHub reports {url} closed without merging; drop the change with `board changes \
             drop --path {path} --reason '<why>'`"
        )
    } else {
        format!(
            "GitHub reports {url} not merged; merge it with `board github merge --change \
             {path}`, which moves the change to done once GitHub reports the merge"
        )
    };
    Err(not_merged(path, url, &why))
}

/// `changes::land::not_merged`: the change at `path` stays where it is
/// because GitHub has not reported its pull request at `pull` merged, for
/// the reason `why`.
fn not_merged(path: &str, pull: &str, why: &str) -> PluginError {
    refusal(
        "changes::land::not_merged",
        format!("the change at {path} reaches done only after its pull request merges: {why}"),
        [("path", json!(path)), ("pull_request", json!(pull))],
    )
}

const BOARD_REFUSED: &str = "changes::board::refused";

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
        if change.pulls().is_empty() {
            entries.push(without_pull(&path, &change));
            continue;
        }
        let (found, ended) = match synced(host, &path, &change, deadline, dry_run) {
            Err(error) if error.code == BOARD_REFUSED => {
                (unread_or_refused(host, &path, &change, error)?, None)
            }
            answer => answer?,
        };
        entries.push(found);
        if let Some(ended) = ended {
            lines.push(json!({"path": path, "expect": blob, "record": ended}));
        }
    }
    let mut unrecorded = None;
    if !dry_run {
        if !lines.is_empty() {
            board::put(host, &lines)?;
        }
        unrecorded = record_sync(host, &today(host).to_string())?;
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
        unrecorded,
    }))
}

/// Ends a change that is not done or dropped in `dropped`, recording why.
pub(crate) fn drop(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let DropInput { path, reason } = PluginError::read_input(PLUGIN, "drop", input)?;
    let (mut change, blob) = read(host, &path)?;
    let phase = change.phase.clone();
    if is_final(&phase) {
        return Err(final_phase(
            &path,
            &phase,
            format!(
                "the change at {path} is already {phase}, so it cannot be dropped; list the open \
                 changes with `board changes open`"
            ),
        ));
    }
    DROPPED.clone_into(&mut change.phase);
    change.dropped_reason = Some(reason);
    write(host, &path, &blob, &change)?;
    Ok(to_value(DropOutput {
        path,
        from: phase,
        to: DROPPED.to_owned(),
        task: change.task,
    }))
}

#[cfg(test)]
mod tests;
