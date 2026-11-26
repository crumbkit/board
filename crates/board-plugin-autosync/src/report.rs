//! What a run leaves for the owner: the status file and log on this Mac, a
//! notification when a board's outcome changes, and one open task in the
//! personal board for each failing board. None of them holds a secret's
//! text: a finding keeps its rule, commit, file and line only.

use std::path::Path;
use std::time::Duration;

use board_model::Object;
use board_ops::{Host, Limit, Run, literal};
use board_plugin::Target;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::scan::Finding;

/// The status file and the log, in the state directory.
pub(crate) const STATUS: &str = "status.json";
const LOG: &str = "autosync.log";
/// The log is rotated once it holds more than this.
const LOG_LIMIT: u64 = 1024 * 1024;
/// The most findings the status keeps for one board.
pub(crate) const MAX_FINDINGS: usize = 50;
/// The most characters a notification shows.
const MAX_NOTICE: usize = 200;

/// How one board's round ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    /// Up to date with its remote, and pushed when it had commits to send.
    Synced,
    /// A dry run checked it and changed nothing.
    Checked,
    /// In the known boards file with no record, so never synced.
    NotSynced,
    Unsigned,
    Missing,
    Nested,
    OriginNotAllowed,
    /// Another branch, staged files, a git operation or another sync.
    Skipped,
    /// The remote could not be reached, fetched or found.
    Unreachable,
    /// A conflict, a failing record or a rewritten remote; nothing moved.
    Refused,
    BoardConfig,
    Findings,
    ScanError,
    ScannerMissing,
    FingerprintsTooLarge,
    PushFailed,
    /// The run's 30 minutes ran out before this board's turn.
    OutOfTime,
}

impl Outcome {
    /// Whether the board failed this round, so the run exits non-zero and
    /// the board's task is open.
    pub(crate) fn failed(self) -> bool {
        !matches!(self, Self::Synced | Self::Checked | Self::NotSynced)
    }

    /// The sentence a notification and a task give, which names no path,
    /// rule or commit.
    fn sentence(self) -> &'static str {
        match self {
            Self::Synced => "is in step with its remote",
            Self::Checked => "was checked and nothing changed",
            Self::NotSynced => "has no autosync record, so it is not synced",
            Self::Unsigned => "has a record without the owner's signature, so it was not synced",
            Self::Missing => "is not at the path its record names",
            Self::Nested => "is inside another checkout, so it was not synced",
            Self::OriginNotAllowed => "pushes to a URL its record does not allow",
            Self::Skipped => "was skipped: another branch, staged files or a git operation",
            Self::Unreachable => "could not reach its remote",
            Self::Refused => "needs a merge by hand, so nothing moved",
            Self::BoardConfig => "holds a gitleaks file, so it was not pushed",
            Self::Findings => "has commits the scan flagged, so they were not pushed",
            Self::ScanError => "could not be scanned, so it was not pushed",
            Self::ScannerMissing => "was not pushed: gitleaks 8.19.0 or later is not installed",
            Self::FingerprintsTooLarge => "was not pushed: the fingerprints file is over 1 MiB",
            Self::PushFailed => "could not be pushed",
            Self::OutOfTime => "was not reached before the run's 30 minutes ran out",
        }
    }
}

/// One board's last round.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct Entry {
    pub(crate) name: String,
    /// When the round ended, in UTC.
    pub(crate) time: String,
    pub(crate) outcome: Outcome,
    /// The refusal's code, when one stopped the round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) code: Option<String>,
    /// What happened, in one sentence.
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) merged: bool,
    #[serde(default)]
    pub(crate) pushed: bool,
    /// How many findings the scan made; at most [`MAX_FINDINGS`] are kept.
    #[serde(default)]
    pub(crate) found: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) findings: Vec<Finding>,
}

/// Every board's last round, as the status file holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct Status {
    pub(crate) boards: Vec<Entry>,
}

impl Status {
    /// The status file in `state`, or an empty one when it is missing or
    /// unreadable, since the next run rewrites it.
    pub(crate) fn read(state: &Path) -> Self {
        (std::fs::read(state.join(STATUS)).ok())
            .and_then(|bytes| board_model::parse_json(&bytes).ok())
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default()
    }

    pub(crate) fn entry(&self, name: &str) -> Option<&Entry> {
        self.boards.iter().find(|entry| entry.name == name)
    }

    /// Writes the status file, and one log line per board.
    pub(crate) fn write(&self, state: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        board_index::write_state(&state.join(STATUS), &bytes)?;
        for entry in &self.boards {
            let code = entry.code.as_deref().unwrap_or("-");
            let outcome = serde_json::to_value(entry.outcome).unwrap_or_default();
            let line = format!(
                "{} {} {} {code} pushed={} found={}",
                entry.time,
                entry.name,
                outcome.as_str().unwrap_or("-"),
                entry.pushed,
                entry.found
            );
            board_index::append_state(&state.join(LOG), &line, LOG_LIMIT)?;
        }
        Ok(())
    }
}

/// Shows a notification of `entry` when its outcome differs from
/// `before`'s. The text reaches `osascript` as an argument, so no board
/// text is read as `AppleScript`. A notification that fails is dropped.
pub(crate) fn notify(host: &dyn Host, entry: &Entry, before: Option<&Entry>) {
    if before.map(|before| before.outcome) == Some(entry.outcome)
        || (before.is_none() && !entry.outcome.failed())
    {
        return;
    }
    let mut text = format!("The {} board {}.", entry.name, entry.outcome.sentence());
    if text.chars().count() > MAX_NOTICE {
        text = text.chars().take(MAX_NOTICE - 1).collect::<String>() + "…";
    }
    let arguments = [
        "-e",
        "on run argv",
        "-e",
        "display notification (item 1 of argv) with title \"Board sync\"",
        "-e",
        "end run",
        &text,
    ]
    .map(str::to_owned);
    let _ = host.run(&Run {
        program: "osascript",
        arguments: &arguments,
        directory: None,
        input: &[],
        deadline: Duration::from_secs(10),
        added: &[],
    });
}

/// The title of a failing board's task.
pub(crate) fn title(name: &str) -> String {
    format!("Board sync stopped for the {name} board")
}

/// The body of a failing board's task: what failed and how often, with no
/// path, rule or commit.
fn body(entry: &Entry) -> String {
    let mut body = format!(
        "autosync stopped syncing the {} board: it {}.",
        entry.name,
        entry.outcome.sentence()
    );
    if entry.found > 0 {
        body += &[" The scan made ", &entry.found.to_string(), " findings."].concat();
    }
    body.push_str(
        "\n\nRun `board --personal autosync status` on this Mac for the details. This task \
         closes itself when the board next syncs cleanly.",
    );
    body
}

/// Opens, rewrites or closes `entry`'s task in the personal board, which
/// `host` is: one open task per failing board, found by its exact title,
/// rewritten only when the outcome or the number of findings changes, and
/// closed with `tasks.done` once the board syncs cleanly. A task write that
/// fails is given back as a warning.
pub(crate) fn task(host: &dyn Host, entry: &Entry, before: Option<&Entry>) -> Option<String> {
    let open = match open_task(host, &entry.name) {
        Ok(open) => open,
        Err(why) => return Some(why),
    };
    let called = |capability: &str, input: Value| -> Option<String> {
        let target: Target = format!("tasks.{capability}").parse().ok()?;
        (host.call(&target, &input).err()).map(|refusal| refusal.message)
    };
    match (entry.outcome.failed(), open) {
        (true, None) => called(
            "add",
            json!({
                "title": title(&entry.name),
                "text": body(entry),
                "repo": "/",
                "reason": "a failing sync of a board on this Mac belongs with that Mac's board",
            }),
        ),
        (true, Some(path)) => {
            let changed = before.is_none_or(|before| {
                before.outcome != entry.outcome || before.found != entry.found
            });
            changed
                .then(|| rewrite(host, &path, &body(entry)))
                .flatten()
        }
        (false, Some(path)) => called("done", json!({"path": path, "push": false})),
        (false, None) => None,
    }
}

/// The path of the open task titled for `name`'s board, if there is one.
fn open_task(host: &dyn Host, name: &str) -> Result<Option<String>, String> {
    let sql = format!(
        "SELECT logical FROM records WHERE type IN ('tasks/task/v2', 'tasks/task/v3') \
         AND logical LIKE '/tasks/%' \
         AND json_extract(body, '$.title') = {} AND json_extract(body, '$.status') = 'open' \
         ORDER BY logical LIMIT 1",
        literal(&title(name))
    );
    let report = host
        .query(&sql, Limit::ALL)
        .map_err(|error| error.refusal().message)?;
    Ok((report.rows.first())
        .and_then(|row| row.first())
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// Puts `body` after the task's frontmatter, keeping the frontmatter, only
/// if the task is still the one read.
fn rewrite(host: &dyn Host, path: &str, body: &str) -> Option<String> {
    let logical = path.parse().ok()?;
    let read = host.get_each(&[logical]).ok()?.into_iter().next()?.ok()?;
    let Object::Blob(blob) = read.location.object else {
        return Some(format!(
            "{path} is not a file, so its task text cannot be rewritten"
        ));
    };
    let text = String::from_utf8(read.bytes).ok()?;
    let end = text.get(4..)?.find("\n---\n")? + 4 + 5;
    let new = format!("{}\n{body}\n", &text[..end]);
    let line = json!({"path": path, "expect": blob.to_string(), "text": new});
    (host.put(&[line]).err()).map(|error| error.refusal().message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(outcome: Outcome, found: usize) -> Entry {
        Entry {
            name: "team".to_owned(),
            time: "2026-11-23T09:00:00Z".to_owned(),
            outcome,
            code: None,
            message: String::new(),
            merged: false,
            pushed: false,
            found,
            findings: Vec::new(),
        }
    }

    #[test]
    fn a_task_body_names_the_failure_and_count_but_no_path_rule_or_commit() {
        let text = body(&entry(Outcome::Findings, 2));
        assert!(text.contains("team board"), "{text}");
        assert!(text.contains("2 findings"), "{text}");
        assert!(text.lines().count() <= 20);
    }

    /// The blob the task holds when `rewrite` reads it.
    const READ: &str = "1111111111111111111111111111111111111111";

    /// A personal board holding one task, as `READ`, whose puts apply each
    /// line's expect as Board does under its put lock. Another writer may
    /// change the task just before the next put.
    struct Holding {
        settings: board_ops::Settings,
        blob: std::cell::RefCell<String>,
        racing: std::cell::RefCell<bool>,
        landed: std::cell::RefCell<Vec<Vec<Value>>>,
    }

    const TASK: &str = "---\n$type: tasks/task/v3\ntitle: t\nstatus: open\n---\n\nOld.\n";

    impl Holding {
        fn new(racing: bool) -> Self {
            Self {
                settings: board_ops::Settings {
                    now: std::time::SystemTime::UNIX_EPOCH,
                    day_start: None,
                    utc_offset: 0,
                    caller: None,
                    environment: board_ops::Variables::default(),
                    cache_home: None,
                },
                blob: std::cell::RefCell::new(READ.to_owned()),
                racing: std::cell::RefCell::new(racing),
                landed: std::cell::RefCell::default(),
            }
        }
    }

    impl Host for Holding {
        fn query(
            &self,
            _: &str,
            _: Limit,
        ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
            unreachable!("rewrite runs no query")
        }

        fn refs(
            &self,
            _: &[board_model::LogicalPath],
            _: board_ops::Absent,
            _: &[String],
        ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
        {
            unreachable!("rewrite resolves no paths")
        }

        fn get_each(
            &self,
            paths: &[board_model::LogicalPath],
        ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError>
        {
            let object = board_model::ObjectId::from_hex(&self.blob.borrow())
                .map_or(board_model::Object::Directory, board_model::Object::Blob);
            Ok(vec![Ok(board_ops::GetReport {
                location: board_model::Location {
                    board: board_model::Board {
                        repository: std::path::PathBuf::from("/personal"),
                        source: board_model::Source::WorkingTree,
                    },
                    path: "tasks/t.md".to_owned(),
                    object,
                    pointers: Vec::new(),
                    follow: None,
                },
                display: paths[0].clone(),
                bytes: TASK.as_bytes().to_vec(),
                record: Ok(json!({})),
            })])
        }

        fn put(&self, lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
            if self.racing.replace(false) {
                *self.blob.borrow_mut() = "3333333333333333333333333333333333333333".to_owned();
            }
            let held = self.blob.borrow().clone();
            for line in lines {
                if line["expect"].as_str().is_some_and(|expect| expect != held) {
                    return Err(board_ops::PutError::Stale {
                        path: line["path"].as_str().unwrap().parse().unwrap(),
                        found: board_model::ObjectId::from_hex(&held),
                    });
                }
            }
            self.landed.borrow_mut().push(lines.to_vec());
            Ok(board_ops::PutReport {
                writes: Vec::new(),
                removed: Vec::new(),
                absent: Vec::new(),
                commit: None,
                mended: Vec::new(),
            })
        }

        fn call(
            &self,
            _: &Target,
            _: &Value,
        ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
            unreachable!("rewrite calls no plugin")
        }

        fn board(
            &self,
            _: &board_ops::Elsewhere,
        ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
            unreachable!("rewrite opens no other board")
        }

        fn decide(
            &self,
            _: &str,
            _: &board_model::LogicalPath,
            _: Option<&board_ops::Shown>,
        ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
            unreachable!("rewrite asks the owner nothing")
        }

        fn sync(&self, _: bool, _: Duration) -> Result<board_ops::Synced, board_ops::SyncError> {
            unreachable!("rewrite syncs nothing")
        }

        fn mount_update(
            &self,
            _: &board_model::LogicalPath,
            _: Duration,
        ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
            unreachable!("rewrite updates no mount")
        }

        fn settings(&self) -> &board_ops::Settings {
            &self.settings
        }
    }

    #[test]
    fn a_rewrite_expects_the_blob_it_read_and_keeps_the_frontmatter() {
        let host = Holding::new(false);
        assert_eq!(rewrite(&host, "/tasks/t.md", "New."), None);
        let landed = host.landed.borrow();
        assert_eq!(landed[0][0]["expect"], READ, "{}", landed[0][0]);
        let text = landed[0][0]["text"].as_str().unwrap();
        assert!(text.starts_with("---\n$type: tasks/task/v3\n"), "{text}");
        assert!(text.ends_with("---\n\nNew.\n"), "{text}");
    }

    #[test]
    fn a_task_another_writer_changes_first_makes_the_rewrite_stale_and_writes_nothing() {
        let host = Holding::new(true);
        let refused = rewrite(&host, "/tasks/t.md", "New.").expect("the rewrite is refused");
        assert!(refused.contains("is not what --expect says"), "{refused}");
        assert!(host.landed.borrow().is_empty(), "nothing is written");
    }

    #[test]
    fn a_task_path_that_is_no_file_is_refused_with_a_message_and_nothing_is_written() {
        let host = Holding::new(false);
        *host.blob.borrow_mut() = String::new();
        let refused = rewrite(&host, "/tasks/t.md", "New.").expect("the rewrite is refused");
        assert!(refused.contains("/tasks/t.md"), "{refused}");
        assert!(host.landed.borrow().is_empty(), "nothing is written");
    }

    #[test]
    fn every_failing_outcome_is_failed_and_the_clean_ones_are_not() {
        assert!(!Outcome::Synced.failed());
        assert!(!Outcome::Checked.failed());
        assert!(!Outcome::NotSynced.failed());
        assert!(Outcome::Findings.failed());
        assert!(Outcome::ScannerMissing.failed());
    }
}
