//! The soft limit on work in progress: three active changes of a kind in
//! build, per repository.
//!
//! A change's repository is the clone Board says its mount reads, keyed by
//! that clone's origin in its stored form, so two mounts of one repository
//! count together. A clone whose origin git cannot read is keyed by its
//! path, and a mount Board cannot resolve by the mount itself.
//!
//! Build changes and prototypes are counted apart. A change counts while it
//! is active: within the last `BOARD_CHANGES_ACTIVE_DAYS` days, 3 by
//! default, a changes capability touched it, its head commit was
//! committed, or a run it references finished. An idle change and a paused
//! change count toward nothing. When the limit is reached, a move into
//! build is refused unless the caller gives a reason, which the change
//! records and the output carries as a warning.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use board_ops::{Host, Origin, PluginError, literal};
use board_repos::Date;
use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::board::{self, rows, text};
use crate::change::{
    OWN, attached, change_types, kind_in, parsed, prototypes, referenced_in_walk, today,
};
use crate::diff::git;
use crate::gates::RUN_TYPE;
use crate::records::{BUILD, ChangeRecord, Kind};
use crate::{refusal, to_value};

/// The most active changes of a kind in build per repository before a move
/// into build needs a reason.
const WIP_LIMIT: usize = 3;
/// How many days a change stays active after its last touch, commit or
/// run, unless the variable below says otherwise.
const ACTIVE_DAYS: u32 = 3;
pub(crate) const ACTIVE_DAYS_VARIABLE: &str = "BOARD_CHANGES_ACTIVE_DAYS";
/// How many local clones an origin is followed through to the origin they
/// follow.
const HOPS: usize = 3;
/// How long each git read the limit makes may take, and how long they may
/// take in all, well inside the 30 s Board gives the capability.
const GIT_READ: Duration = Duration::from_secs(10);
const GIT_READS: Duration = Duration::from_secs(20);

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("each pattern is a constant; the WIP tests compile it")
}

/// A GitHub origin in a form `shown_remote` does not reduce, with any
/// scheme, case, www or port; its path.
static GITHUB_URL: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"(?i)^[a-z][a-z0-9+.-]*://(?:[^@/]+@)?(?:www\.)?github\.com(?::\d+)?/(.+)$")
});
static GITHUB_SCP: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)^[^@/:]+@(?:www\.)?github\.com:(.+)$"));

/// An idle change in build and what to do with it: sync, drop or resume,
/// and the command.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub(crate) struct Idle {
    pub(crate) path: String,
    pub(crate) action: Action,
    pub(crate) command: String,
}

#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Action {
    Sync,
    Drop,
    Resume,
}

impl Action {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Drop => "drop",
            Self::Resume => "resume",
        }
    }
}

/// The WIP limit a move into build went past: the active changes holding
/// places, the idle ones, the override.
///
/// `repository` is the repository counted, by its origin's stored form, or
/// by its clone or mount when there is none; `unread` maps each mount whose
/// origin git could not read to why.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[expect(
    clippy::struct_field_names,
    reason = "the refusal's fields name the WIP limit `limit`, as the Python plugin did"
)]
pub(crate) struct Limit {
    code: &'static str,
    mount: String,
    repository: String,
    unread: BTreeMap<String, String>,
    kind: Kind,
    limit: usize,
    active_days: u32,
    active: Vec<String>,
    idle: Vec<Idle>,
    #[serde(rename = "override")]
    #[schemars(rename = "override")]
    over: String,
}

/// Where a change's mount reads: the repository's key, and why git gave no
/// origin for its clone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Repository {
    pub(crate) key: String,
    pub(crate) unread: Option<String>,
}

/// `origin` as a repository is keyed by, in lower case: `owner/name` for
/// GitHub, else as written.
pub(crate) fn stored(origin: &str) -> String {
    let matched = (GITHUB_URL.captures(origin))
        .or_else(|| GITHUB_SCP.captures(origin))
        .and_then(|captures| captures.get(1));
    let origin = matched.map_or_else(
        || origin.to_owned(),
        |path| format!("https://github.com/{}", path.as_str()),
    );
    board_model::shown_remote(&origin).to_lowercase()
}

/// The clone a local origin names, as an absolute path or a file:// URL;
/// else none.
fn local(origin: &str) -> Option<&str> {
    let path = origin.strip_prefix("file://").unwrap_or(origin);
    path.starts_with('/').then_some(path)
}

/// The repository `clone` follows: its origin's stored form, through up to
/// `HOPS` local clones. Each origin is read at the clone itself, so a
/// directory that is not a repository never gives the origin of one around
/// it.
pub(crate) fn followed(host: &dyn Host, clone: &Path) -> Repository {
    let mut clone = clone.to_path_buf();
    for hop in 0..=HOPS {
        let shown = clone.display().to_string();
        let unread = |why: String| Repository {
            key: shown.clone(),
            unread: Some(why),
        };
        let url = match host.clone_origin(&clone) {
            Origin::Url(url) if !url.trim().is_empty() => url.trim().to_owned(),
            Origin::Url(_) => return unread(format!("git gave {shown} an empty origin")),
            Origin::Outside => {
                return unread(format!(
                    "git gave no origin for {shown}: it is not a git repository"
                ));
            }
            Origin::Unread(why) => return unread(format!("git gave no origin for {shown}: {why}")),
        };
        let Some(nearer) = local(&url) else {
            return Repository {
                key: stored(&url),
                unread: None,
            };
        };
        if hop == HOPS {
            return Repository {
                key: nearer.to_owned(),
                unread: Some(format!(
                    "followed {HOPS} local clones to {nearer}, whose origin was not read"
                )),
            };
        }
        clone = PathBuf::from(nearer);
    }
    unreachable!("the loop returns on its last hop")
}

/// The clone each of `mounts` reads, by mount, or none for a mount Board
/// cannot resolve.
fn clones(host: &dyn Host, mounts: &[String]) -> Result<BTreeMap<String, PathBuf>, PluginError> {
    let answers = board::refs_in_walk(host, mounts)?;
    Ok((mounts.iter().zip(answers))
        .filter_map(|(mount, answer)| {
            Some((mount.clone(), answer.ok()??.location.board.repository))
        })
        .collect())
}

/// The repository each of `mounts` reads, by mount.
fn repositories(
    host: &dyn Host,
    mounts: &[String],
) -> Result<BTreeMap<String, Repository>, PluginError> {
    let found = clones(host, mounts)?;
    Ok((mounts.iter())
        .map(|mount| {
            let read = found.get(mount).map_or_else(
                || Repository {
                    key: mount.clone(),
                    unread: None,
                },
                |clone| followed(host, clone),
            );
            (mount.clone(), read)
        })
        .collect())
}

/// Every change of the board's own in build that Board does not mark
/// degraded, by path, in path order.
fn in_build(host: &dyn Host) -> Result<Vec<(String, ChangeRecord)>, PluginError> {
    let sql = format!(
        "SELECT logical, code, body FROM records WHERE type IN ({}) AND json_extract(body, \
         '$.phase') = 'build' AND substr(logical, 1, {}) = {} ORDER BY logical",
        change_types(),
        OWN.len(),
        literal(OWN)
    );
    Ok((rows(host, &sql)?.iter())
        .filter(|row| board::readable(row.get(1)))
        .filter_map(|row| Some((text(row.first())?.to_owned(), parsed(row.get(2))?)))
        .collect())
}

/// The days a change stays active, from `BOARD_CHANGES_ACTIVE_DAYS`, or
/// `ACTIVE_DAYS` when it is unset or empty.
pub(crate) fn active_days(host: &dyn Host) -> Result<u32, PluginError> {
    let given = (host.settings().environment.0.iter())
        .find(|(name, _)| name == ACTIVE_DAYS_VARIABLE)
        .map(|(_, value)| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    if given.is_empty() {
        return Ok(ACTIVE_DAYS);
    }
    let days = (given.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| given.parse::<u32>().ok())
        .flatten()
        .filter(|days| *days >= 1);
    days.ok_or_else(|| {
        refusal(
            "changes::wip::bad_active_days",
            format!(
                "{ACTIVE_DAYS_VARIABLE} is '{given}', which is not a whole number of days of at \
                 least 1; set it to one, such as 3, or unset it for {ACTIVE_DAYS}"
            ),
            [
                ("variable", json!(ACTIVE_DAYS_VARIABLE)),
                ("value", json!(given)),
            ],
        )
    })
}

/// The UTC day of an RFC 3339 time or a `YYYY-MM-DD` day, or none when it
/// is neither. A time without an offset is read as UTC.
pub(crate) fn day_of(stamp: Option<&Value>) -> Option<Date> {
    let stamp = stamp?.as_str()?;
    let date = Date::parse(stamp.get(..10)?)?;
    let rest = &stamp[10..];
    if rest.is_empty() {
        return Some(date);
    }
    let rest = rest.strip_prefix(['T', 't', ' '])?;
    let number = |text: &str| -> Option<i64> {
        (text.len() == 2 && text.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| text.parse().ok())
            .flatten()
    };
    let (hours, rest) = (number(rest.get(..2)?)?, &rest[2..]);
    let rest = rest.strip_prefix(':')?;
    let (minutes, mut rest) = (number(rest.get(..2)?)?, &rest[2..]);
    let mut seconds = 0;
    if let Some(after) = rest.strip_prefix(':') {
        seconds = number(after.get(..2)?)?;
        rest = &after[2..];
        if let Some(fraction) = rest.strip_prefix(['.', ',']) {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                return None;
            }
            rest = &fraction[digits..];
        }
    }
    if hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let (offset_hours, offset_minutes) = rest[1..].split_once(':')?;
            let (offset_hours, offset_minutes) = (number(offset_hours)?, number(offset_minutes)?);
            if offset_hours > 23 || offset_minutes > 59 {
                return None;
            }
            sign * (offset_hours * 3600 + offset_minutes * 60)
        }
    };
    let utc = hours * 3600 + minutes * 60 + seconds - offset;
    Some(date.plus(utc.div_euclid(86_400)))
}

/// The shared deadline of the limit's git reads, and the refusal once it
/// has passed.
struct Reads {
    until: Instant,
}

impl Reads {
    fn start() -> Self {
        Self {
            until: Instant::now() + GIT_READS,
        }
    }

    /// How long the next read may take, or `changes::wip::timeout` when no
    /// time is left.
    fn left(&self, path: &str) -> Result<Duration, PluginError> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(refusal(
                "changes::wip::timeout",
                format!(
                    "the WIP limit's git reads of the changes in build took {} s in all, so \
                     whether {path} may move into build is not known; run the command again once \
                     git is not held up",
                    GIT_READS.as_secs()
                ),
                [("path", json!(path))],
            ));
        }
        Ok(left.min(GIT_READ))
    }
}

/// The UTC day each change's head was committed, read with git from its
/// mount's repository, when git knows it.
///
/// git resolves each head on its own, so an abbreviated head names exactly
/// one commit or none.
fn committed(
    host: &dyn Host,
    moving: &str,
    changes: &[(String, ChangeRecord)],
) -> Result<BTreeMap<String, Date>, PluginError> {
    let mut mounts: Vec<String> = (changes.iter())
        .filter(|(_, change)| change.head.is_some())
        .map(|(_, change)| change.mount.clone())
        .collect();
    mounts.sort();
    mounts.dedup();
    let repositories = clones(host, &mounts)?;
    let reads = Reads::start();
    let mut found = BTreeMap::new();
    for (path, change) in changes {
        let (Some(head), Some(repository)) = (&change.head, repositories.get(&change.mount)) else {
            continue;
        };
        let deadline = reads.left(moving)?;
        let arguments = [
            "log",
            "-1",
            "--format=%ct",
            "--end-of-options",
            head.as_str(),
        ];
        match git(host, repository, &arguments, deadline) {
            Ok(out) => {
                let out = out.trim();
                if let Some(seconds) = (!out.is_empty()
                    && out.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| out.parse::<i64>().ok())
                .flatten()
                {
                    found.insert(path.clone(), Date::from_days(seconds.div_euclid(86_400)));
                }
            }
            Err(failed) if failed.timed_out => {
                reads.left(moving)?;
            }
            Err(_) => {}
        }
    }
    Ok(found)
}

/// The UTC day the newest run each change references, through any link,
/// finished.
fn finished(
    host: &dyn Host,
    changes: &[(String, ChangeRecord)],
) -> Result<BTreeMap<String, Date>, PluginError> {
    let mut found = BTreeMap::new();
    for (path, _) in changes {
        let records = referenced_in_walk(host, &attached(host, path)?)?;
        let newest = (records.of_type(RUN_TYPE))
            .filter_map(|(_, body)| day_of(body.get("finished")))
            .max();
        if let Some(day) = newest {
            found.insert(path.clone(), day);
        }
    }
    Ok(found)
}

/// The paths active on one of the `days` days up to `today`, today
/// included, and the idle ones, each in order.
pub(crate) fn split(
    newest: &[(String, Option<Date>)],
    today: Date,
    days: u32,
) -> (Vec<String>, Vec<String>) {
    let since = today.plus(1 - i64::from(days));
    let (active, idle): (Vec<_>, Vec<_>) =
        (newest.iter()).partition(|(_, day)| day.is_some_and(|day| day >= since));
    let paths = |listed: Vec<&(String, Option<Date>)>| {
        listed.into_iter().map(|(path, _)| path.clone()).collect()
    };
    (paths(active), paths(idle))
}

/// What to do with an idle change: sync one with a pull request, resume one
/// with commits, else drop it.
pub(crate) fn idle_action(path: &str, change: &ChangeRecord) -> Idle {
    let (action, command) = if !change.pulls().is_empty() {
        (Action::Sync, "board changes sync".to_owned())
    } else if change.head.is_some() {
        (Action::Resume, format!("board changes next --path {path}"))
    } else {
        (
            Action::Drop,
            format!("board changes drop --path {path} --reason '<why>'"),
        )
    };
    Idle {
        path: path.to_owned(),
        action,
        command,
    }
}

/// The change at `path` among `changes`.
fn held<'a>(changes: &'a [(String, ChangeRecord)], path: &str) -> Option<&'a ChangeRecord> {
    (changes.iter())
        .find(|(each, _)| each == path)
        .map(|(_, change)| change)
}

/// A move into build as the limit reads it: the command that passes the
/// limit, the reason given, and how many days a change stays active.
pub(crate) struct Move<'a> {
    pub(crate) over: String,
    pub(crate) reason: Option<&'a str>,
    pub(crate) days: u32,
}

/// Whether `change` may move into build at `path`: none below the limit,
/// else the limit it goes past.
///
/// At the limit, without a reason the move is refused with
/// `changes::wip::full`; with one, the change records it in `over_limit`
/// and the returned limit is the warning to show.
pub(crate) fn admit(
    host: &dyn Host,
    path: &str,
    change: &mut ChangeRecord,
    kind: Kind,
    moving: &Move<'_>,
) -> Result<Option<Limit>, PluginError> {
    let prototypes = prototypes(host)?;
    let candidates: Vec<(String, ChangeRecord)> = (in_build(host)?.into_iter())
        .filter(|(other, held)| {
            other != path && held.phase == BUILD && kind_in(&prototypes, other) == kind
        })
        .collect();
    if candidates.len() < WIP_LIMIT {
        return Ok(None);
    }
    let mut mounts: Vec<String> = (candidates.iter())
        .map(|(_, held)| held.mount.clone())
        .chain([change.mount.clone()])
        .collect();
    mounts.sort();
    mounts.dedup();
    let read = repositories(host, &mounts)?;
    let key = |mount: &str| -> String {
        read.get(mount)
            .map_or_else(|| mount.to_owned(), |repository| repository.key.clone())
    };
    let repository = key(&change.mount);
    let others: Vec<(String, ChangeRecord)> = (candidates.into_iter())
        .filter(|(_, held)| key(&held.mount) == repository)
        .collect();
    if others.len() < WIP_LIMIT {
        return Ok(None);
    }
    let (commits, runs) = (committed(host, path, &others)?, finished(host, &others)?);
    let newest: Vec<(String, Option<Date>)> = (others.iter())
        .map(|(other, held)| {
            let touched = day_of(
                held.touched
                    .as_ref()
                    .map(|day| Value::from(day.as_str()))
                    .as_ref(),
            );
            let day = [
                touched,
                commits.get(other).copied(),
                runs.get(other).copied(),
            ]
            .into_iter()
            .flatten()
            .max();
            (other.clone(), day)
        })
        .collect();
    let (active, idle) = split(&newest, today(host), moving.days);
    if active.len() < WIP_LIMIT {
        return Ok(None);
    }
    let limit = Limit {
        code: "changes::wip::over_limit",
        mount: change.mount.clone(),
        repository: repository.clone(),
        unread: (read.iter())
            .filter_map(|(mount, each)| Some((mount.clone(), each.unread.clone()?)))
            .collect(),
        kind,
        limit: WIP_LIMIT,
        active_days: moving.days,
        active: active.clone(),
        idle: (idle.iter())
            .filter_map(|other| Some(idle_action(other, held(&others, other)?)))
            .collect(),
        over: moving.over.clone(),
    };
    if let Some(reason) = moving.reason {
        change.over_limit = Some(reason.to_owned());
        return Ok(Some(limit));
    }
    let what = if kind == Kind::Build {
        "build changes"
    } else {
        "prototypes"
    };
    let Value::Object(mut fields) = to_value(&limit) else {
        unreachable!("a limit is a JSON object");
    };
    fields.remove("code");
    fields.insert("path".to_owned(), json!(path));
    Err(board_ops::PluginError {
        code: "changes::wip::full".to_owned(),
        message: format!(
            "{} {what} active in the last {} days are in build for {repository}, the limit of \
             {WIP_LIMIT} ({}), so {path} waits; finish, pause or drop one, deal with the idle \
             ones in `idle`, or go ahead and record why with `{}`",
            active.len(),
            moving.days,
            active.join(", "),
            moving.over
        ),
        fields,
        hint: None,
    })
}

#[cfg(test)]
mod tests;
