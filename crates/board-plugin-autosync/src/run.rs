//! `run`: one round over every board record, one board at a time, under an
//! exclusive lock on this Mac; and `status`, which reads the last round.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use board_model::Code;
use board_ops::{BeforePush, Elsewhere, Guard, Host, Limit, PluginError, Pushing, literal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::origin::{self, Unfit, git};
use crate::records::{BOARDS, BoardRecord};
use crate::report::{self, Entry, MAX_FINDINGS, Outcome, Status};
use crate::scan::{self, Finding, SCRATCH, Scan, ScanFailure, Scanned, Scanner};
use crate::{PLUGIN, home, refusal, state};
use board_plugin::shape::Record as _;

/// The Mac's fingerprints file, under the home.
const FINGERPRINTS: &str = ".config/board/autosync/gitleaksignore";
/// How long a scan may run unless `--scan-timeout` says otherwise.
const SCAN_TIME: u64 = 600;
/// How long a whole run may take before the boards left are skipped.
const RUN_TIME: Duration = Duration::from_mins(30);
/// What fetch, merge and push share for one board, as `board sync`.
const SYNC_TIME: Duration = Duration::from_secs(60);

/// What `run` takes.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RunInput")]
pub(crate) struct Input {
    /// Checks each board's signature and origins and prints the range a
    /// push would scan; fetches, merges, pushes and writes nothing.
    #[serde(default, rename = "dry-run")]
    pub(crate) dry_run: bool,
    /// Seconds a scan may run, 600 when left out, such as for a first run
    /// with a long backlog.
    #[serde(rename = "scan-timeout")]
    #[schemars(range(min = 1, max = 86400))]
    pub(crate) scan_timeout: Option<u64>,
}

/// What `status` takes: nothing.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "StatusInput")]
pub(crate) struct StatusInput {}

/// Each board's round, and any task write that failed.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "RunOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) boards: Vec<Entry>,
    pub(crate) warnings: Vec<String>,
}

/// What every board's round shares.
struct Round<'a> {
    host: &'a dyn Host,
    home: PathBuf,
    scanner: Scanner,
    /// The scan, or the size of a fingerprints file too large to use.
    scan: Result<Scan, u64>,
    dry_run: bool,
    now: String,
}

pub(crate) fn status(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let _: StatusInput = PluginError::read_input(PLUGIN, "status", input)?;
    Ok(json!(Status::read(&state(&home(host)?))))
}

pub(crate) fn run(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "run", input)?;
    let home = home(host)?;
    let state = state(&home);
    let unwritable = |error: std::io::Error| {
        refusal(
            "autosync::state::unwritable",
            format!("{} cannot be written: {error}", state.display()),
            Map::new(),
            "make the directory writable by you, then run `board --personal autosync run` again",
        )
    };
    let Some(_only) = board_index::lock_state(&state.join("run.lock")).map_err(unwritable)? else {
        return Err(refusal(
            "autosync::run::busy",
            "another autosync run holds run.lock on this Mac".to_owned(),
            Map::new(),
            "wait for the other run to end; the agent runs again at its next interval",
        ));
    };
    board_index::remove_scratch(&state, SCRATCH).map_err(unwritable)?;
    let started = Instant::now();
    let fingerprints = scan::fingerprints(&home.join(FINGERPRINTS));
    let round = Round {
        host,
        scanner: scan::scanner(host),
        scan: fingerprints.map(|fingerprints| Scan {
            state: state.clone(),
            fingerprints,
            deadline: Duration::from_secs(given.scan_timeout.unwrap_or(SCAN_TIME)),
        }),
        home,
        dry_run: given.dry_run,
        now: board_model::utc(host.settings().now),
    };
    let mut boards = Vec::new();
    let mut synced = Vec::new();
    for (name, path, record) in records(host)? {
        if started.elapsed() > RUN_TIME {
            boards.push(round.entry(&name, Outcome::OutOfTime, None, String::new()));
            continue;
        }
        let (entry, directory) = round.board(&name, &path, &record);
        synced.extend(directory);
        boards.push(entry);
    }
    boards.extend(round.unlisted(&synced));
    let mut warnings = Vec::new();
    if !given.dry_run {
        let before = Status::read(&state);
        for entry in &boards {
            report::notify(host, entry, before.entry(&entry.name));
            warnings.extend(report::task(host, entry, before.entry(&entry.name)));
        }
        (Status {
            boards: boards.clone(),
        })
        .write(&state)
        .map_err(unwritable)?;
    }
    let failed: Vec<&str> = (boards.iter())
        .filter(|entry| entry.outcome.failed())
        .map(|entry| entry.name.as_str())
        .collect();
    if failed.is_empty() {
        return Ok(json!(Output { boards, warnings }));
    }
    let mut fields = Map::new();
    fields.insert("boards".to_owned(), json!(boards));
    fields.insert("warnings".to_owned(), json!(warnings));
    Err(refusal(
        "autosync::run::failed",
        format!(
            "{} of {} boards did not sync: {}",
            failed.len(),
            boards.len(),
            failed.join(", ")
        ),
        fields,
        "run `board --personal autosync status` to see why each one stopped",
    ))
}

/// Every board record, by name, logical path and body, in name order.
fn records(host: &dyn Host) -> Result<Vec<(String, String, Value)>, PluginError> {
    let sql = format!(
        "SELECT logical, body FROM records WHERE type = {} AND logical LIKE {} ORDER BY logical",
        literal(BoardRecord::TYPE),
        literal(&format!("{BOARDS}%"))
    );
    let report = (host.query(&sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused(PLUGIN, "query", &error.refusal()))?;
    Ok((report.rows.into_iter())
        .filter_map(|row| {
            let path = row.first()?.as_str()?.to_owned();
            let name = path.strip_prefix(BOARDS)?.to_owned();
            let body = match row.get(1)? {
                Value::String(text) => board_model::parse_json(text.as_bytes()).ok()?,
                other => other.clone(),
            };
            Some((name, path, body))
        })
        .collect())
}

/// Why a push was held, as the guard saw it.
enum Held {
    BoardConfig(&'static str),
    Found(Vec<Finding>),
    Failed(ScanFailure),
}

impl Round<'_> {
    fn entry(&self, name: &str, outcome: Outcome, code: Option<&str>, message: String) -> Entry {
        Entry {
            name: name.to_owned(),
            time: self.now.clone(),
            outcome,
            code: code.map(str::to_owned),
            message,
            merged: false,
            pushed: false,
            found: 0,
            findings: Vec::new(),
        }
    }

    /// One board's round, and its directory once its record verified.
    fn board(&self, name: &str, path: &str, body: &Value) -> (Entry, Option<PathBuf>) {
        let unsigned = |why: &str| {
            let message = format!("the record at {path} {why}, so the board was not fetched");
            self.entry(
                name,
                Outcome::Unsigned,
                Some("autosync::board::unsigned"),
                message,
            )
        };
        let Ok(record) = serde_json::from_value::<BoardRecord>(body.clone()) else {
            return (unsigned("is not an autosync board record"), None);
        };
        let Some(signature) = &record.signature else {
            return (unsigned("has no signature"), None);
        };
        let mut fields = body.clone();
        if let Some(object) = fields.as_object_mut() {
            object.remove("signature");
        }
        if let Err(error) = self.host.verify_owner_write(path, &fields, signature) {
            return (unsigned(&format!("has no good signature: {error}")), None);
        }
        let directory = match record.path.strip_prefix("~/") {
            Some(rest) => self.home.join(rest),
            None => PathBuf::from(&record.path),
        };
        let entry = self.checked(name, &directory, &record);
        (entry, Some(directory))
    }

    /// The round of a board whose record verified.
    fn checked(&self, name: &str, directory: &Path, record: &BoardRecord) -> Entry {
        if !directory.is_dir() {
            let message = format!("{} is not a directory", directory.display());
            return self.entry(
                name,
                Outcome::Missing,
                Some("autosync::board::missing"),
                message,
            );
        }
        if origin::own_checkout(self.host, directory).is_err() {
            let message = format!(
                "{} is not the top of its own checkout, or lies inside another",
                directory.display()
            );
            return self.entry(
                name,
                Outcome::Nested,
                Some("autosync::board::nested"),
                message,
            );
        }
        if let Err(Unfit::NotAllowed { urls }) =
            origin::allowed(self.host, directory, record.branch(), &record.origins)
        {
            let message = if urls.is_empty() {
                format!("{} follows no remote with a push URL", record.branch())
            } else {
                format!(
                    "it would push to {}, which its record does not allow",
                    urls.join(", ")
                )
            };
            return self.entry(
                name,
                Outcome::OriginNotAllowed,
                Some("autosync::origin::not_allowed"),
                message,
            );
        }
        if self.dry_run {
            return self.dry(name, directory, record);
        }
        self.synced(name, directory, record)
    }

    /// What a dry run says of a board that passed its checks: the range a
    /// push would scan.
    fn dry(&self, name: &str, directory: &Path, record: &BoardRecord) -> Entry {
        let upstream = format!("{}@{{upstream}}", record.branch());
        let range = format!("{upstream}..{}", record.branch());
        let counted =
            git(self.host, directory, &["rev-list", "--count", &range]).filter(origin::Ran::ok);
        let message = match counted {
            Some(ran) => format!(
                "signature and origins check; a push would scan {} commits, {range}",
                ran.stdout.trim()
            ),
            None => "signature and origins check; the remote branch is not fetched yet, so a \
                     push would scan every commit"
                .to_owned(),
        };
        self.entry(name, Outcome::Checked, None, message)
    }

    /// Syncs the board through the guarded sync, scanning before each push.
    fn synced(&self, name: &str, directory: &Path, record: &BoardRecord) -> Entry {
        let opened = self
            .host
            .board(&Elsewhere::Directory(directory.to_path_buf()));
        let board = match opened {
            Ok(board) => board,
            Err(refused) => {
                return self.entry(
                    name,
                    Outcome::Missing,
                    Some(refused.code.as_str()),
                    refused.message,
                );
            }
        };
        let scan = self
            .scan
            .as_ref()
            .ok()
            .filter(|_| self.scanner == Scanner::Ready);
        let mut held = None;
        let mut before_push = |pushing: &Pushing| {
            let Some(scan) = scan else {
                return BeforePush::Hold;
            };
            if let Some(file) = scan::board_config(self.host, directory, &pushing.new.to_string()) {
                held = Some(Held::BoardConfig(file));
                return BeforePush::Hold;
            }
            match scan.run(self.host, directory, pushing) {
                Scanned::Clean => BeforePush::Push,
                Scanned::Found(found) => {
                    held = Some(Held::Found(found));
                    BeforePush::Hold
                }
                Scanned::Failed(failure) => {
                    held = Some(Held::Failed(failure));
                    BeforePush::Hold
                }
            }
        };
        let mut guard = Guard {
            branch: record.branch().to_owned(),
            before_push: &mut before_push,
        };
        let result = board.sync_guarded(scan.is_some(), &mut guard, SYNC_TIME);
        drop(guard);
        match result {
            Ok(synced) => {
                let mut entry = self.unpushable(name).unwrap_or_else(|| {
                    self.entry(
                        name,
                        Outcome::Synced,
                        None,
                        "in step with its remote".to_owned(),
                    )
                });
                entry.merged = synced.merged || synced.old != Some(synced.new);
                entry.pushed = synced.pushed;
                entry
            }
            Err(error) if error.code() == Code::SyncHeld => {
                let mut entry = self.held(name, held);
                entry.merged = true;
                entry
            }
            Err(error) => {
                let outcome = match error.code() {
                    Code::SyncOtherBranch
                    | Code::SyncStaged
                    | Code::SyncMerging
                    | Code::SyncBusy => Outcome::Skipped,
                    Code::SyncFailed
                    | Code::SyncTimeout
                    | Code::SyncNoRemote
                    | Code::SyncNoBranch => Outcome::Unreachable,
                    Code::SyncGaveUp => Outcome::PushFailed,
                    _ => Outcome::Refused,
                };
                self.entry(
                    name,
                    outcome,
                    Some(error.code().as_str()),
                    error.to_string(),
                )
            }
        }
    }

    /// The entry of a board that merged but could not be scanned, so
    /// nothing was pushed, if that is why.
    fn unpushable(&self, name: &str) -> Option<Entry> {
        if let Err(size) = self.scan {
            let message = format!(
                "~/{FINGERPRINTS} holds {size} bytes, over {} bytes, so no push runs",
                scan::MAX_FINGERPRINTS
            );
            let code = "autosync::scan::fingerprints_too_large";
            return Some(self.entry(name, Outcome::FingerprintsTooLarge, Some(code), message));
        }
        match &self.scanner {
            Scanner::Ready => None,
            Scanner::Missing => Some(
                self.entry(
                    name,
                    Outcome::ScannerMissing,
                    Some("autosync::scan::scanner_missing"),
                    "gitleaks is not on the PATH, so the board merged and nothing was pushed"
                        .to_owned(),
                ),
            ),
            Scanner::TooOld { version } => Some(self.entry(
                name,
                Outcome::ScannerMissing,
                Some("autosync::scan::scanner_missing"),
                format!("gitleaks {version} is older than 8.19.0, so nothing was pushed"),
            )),
        }
    }

    /// The entry of a board whose push the guard held.
    fn held(&self, name: &str, held: Option<Held>) -> Entry {
        match held {
            Some(Held::BoardConfig(file)) => self.entry(
                name,
                Outcome::BoardConfig,
                Some("autosync::scan::board_config"),
                format!(
                    "the board holds {file}, which gitleaks would read; remove {file} from the \
                     board, put any false-positive fingerprint in ~/{FINGERPRINTS} on this Mac, \
                     then run `board --personal autosync run`"
                ),
            ),
            Some(Held::Found(found)) => {
                let mut entry = self.entry(
                    name,
                    Outcome::Findings,
                    Some("autosync::scan::found"),
                    format!(
                        "the scan made {} findings in the commits to push, so none were pushed",
                        found.len()
                    ),
                );
                entry.found = found.len();
                entry.findings = found.into_iter().take(MAX_FINDINGS).collect();
                entry
            }
            Some(Held::Failed(failure)) => self.entry(
                name,
                Outcome::ScanError,
                Some("autosync::scan::failed"),
                format!("{}, so nothing was pushed", failure.describe()),
            ),
            None => self.entry(
                name,
                Outcome::ScanError,
                Some("autosync::scan::failed"),
                "the push was held before a scan ran".to_owned(),
            ),
        }
    }

    /// An entry for each known board no verified record names.
    fn unlisted(&self, synced: &[PathBuf]) -> Vec<Entry> {
        let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let synced: Vec<PathBuf> = synced.iter().map(|path| canonical(path)).collect();
        (self.host.known_boards().iter())
            .filter(|board| !synced.contains(&canonical(board)))
            .map(|board| {
                let name = board.file_name().map_or_else(
                    || board.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                self.entry(
                    &name,
                    Outcome::NotSynced,
                    None,
                    format!("{} has no autosync record", board.display()),
                )
            })
            .collect()
    }
}
