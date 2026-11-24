//! `list`: each entry in rank order, with what it covers, and whether the
//! owner must confirm it. A confirmed entry is stale from [`STALE_DAYS`]
//! days after the day it was confirmed, or sooner when the board has more
//! commits since that day than the value `priorities-reconfirm` allows.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use board_ops::{Host, Limit, PluginError, Run, literal};
use board_plugin::shape::{Date, Slug};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::records::{Horizon, PrioritiesRecord};
use crate::{held, refused};

/// The params value that says how many commits an entry may see before
/// it is stale.
pub(crate) const RECONFIRM: &str = "priorities-reconfirm";

/// The commits an entry may see when no heuristic of [`RECONFIRM`] is
/// found.
const RECONFIRM_DEFAULT: f64 = 300.0;

/// The whole UTC days after its confirmation at which an entry is stale,
/// however few commits the board has.
const STALE_DAYS: i64 = 7;

/// How long git may take to count a day's commits.
const GIT_DEADLINE: Duration = Duration::from_secs(10);

/// One horizon, or both when left out.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ListInput")]
pub(crate) struct Input {
    /// short or long; left out, both.
    horizon: Option<Horizon>,
}

/// Each horizon's entries, highest first, and the commits an entry may
/// see before it is stale.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ListOutput")]
pub(crate) struct Output {
    short: Vec<Row>,
    long: Vec<Row>,
    stale_after: f64,
}

/// One entry: its rank in its horizon, from 1; `unconfirmed` when the
/// owner has not confirmed it since it last changed; `stale` when
/// [`STALE_DAYS`] days have passed since `confirmed`, or the board has
/// more commits since `confirmed` than `stale_after`.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Row {
    rank: usize,
    horizon: Horizon,
    #[schemars(with = "Slug")]
    id: String,
    title: String,
    #[schemars(with = "Option<Date>")]
    confirmed: Option<String>,
    unconfirmed: bool,
    stale: bool,
    covers: Vec<Covered>,
}

/// A covered record with its type, title and status (a task's status or
/// a change's phase), or `missing` when nothing is at its path.
#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
enum Covered {
    Found {
        path: String,
        #[serde(rename = "type")]
        record_type: String,
        title: Option<String>,
        status: Option<String>,
    },
    Missing {
        path: String,
        missing: bool,
    },
}

pub(crate) fn list(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: Input = PluginError::read_input("priorities", "list", input)?;
    let (record, _) = held(host)?;
    let stale_after = reconfirm(host);
    let today = board_repos::today(host.settings().now, 0, Duration::ZERO);
    let titles = covered(host, &record)?;
    let mut counted: BTreeMap<String, Option<u32>> = BTreeMap::new();
    let mut rows = |horizon: Horizon| -> Vec<Row> {
        if asked.horizon.is_some_and(|only| only != horizon) {
            return Vec::new();
        }
        (record.entries(horizon).iter().enumerate())
            .map(|(index, entry)| {
                let stale = entry.confirmed.as_ref().is_some_and(|day| {
                    let since =
                        (counted.entry(day.clone())).or_insert_with(|| commits_since(host, day));
                    is_stale(board_repos::Date::parse(day), today, *since, stale_after)
                });
                Row {
                    rank: index + 1,
                    horizon,
                    id: entry.id.clone(),
                    title: entry.title.clone(),
                    confirmed: entry.confirmed.clone(),
                    unconfirmed: entry.confirmed.is_none(),
                    stale,
                    covers: (entry.covers.iter())
                        .map(|path| match titles.get(path) {
                            Some((record_type, title, status)) => Covered::Found {
                                path: path.clone(),
                                record_type: record_type.clone(),
                                title: title.clone(),
                                status: status.clone(),
                            },
                            None => Covered::Missing {
                                path: path.clone(),
                                missing: true,
                            },
                        })
                        .collect(),
                }
            })
            .collect()
    };
    let output = Output {
        short: rows(Horizon::Short),
        long: rows(Horizon::Long),
        stale_after,
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings and numbers"))
}

/// Whether an entry confirmed on `confirmed`, or on a day that did not
/// parse when none, is stale on `today`: [`STALE_DAYS`] or more whole
/// days have passed, or more than `stale_after` commits are known since.
pub(crate) fn is_stale(
    confirmed: Option<board_repos::Date>,
    today: board_repos::Date,
    commits: Option<u32>,
    stale_after: f64,
) -> bool {
    let aged = confirmed.is_some_and(|day| today.days() - day.days() >= STALE_DAYS);
    aged || commits.is_some_and(|since| f64::from(since) > stale_after)
}

/// The commits an entry may see, from params, or the built-in default
/// when no heuristic is found.
fn reconfirm(host: &dyn Host) -> f64 {
    board_plugin_params::get(host, RECONFIRM, None).map_or(RECONFIRM_DEFAULT, |got| got.value)
}

/// The type, title and status of each covered record in this board.
type Titles = BTreeMap<String, (String, Option<String>, Option<String>)>;

fn covered(host: &dyn Host, record: &PrioritiesRecord) -> Result<Titles, PluginError> {
    let paths: Vec<String> = (Horizon::BOTH.iter())
        .flat_map(|horizon| record.entries(*horizon))
        .flat_map(|entry| entry.covers.iter().map(|path| literal(path)))
        .collect();
    if paths.is_empty() {
        return Ok(Titles::new());
    }
    let sql = format!(
        "SELECT logical, type, json_extract(body, '$.title'), \
         coalesce(json_extract(body, '$.status'), json_extract(body, '$.phase')) \
         FROM records WHERE source = 'worktree' AND logical IN ({})",
        paths.join(", ")
    );
    let rows = (host.query(&sql, Limit::ALL))
        .map(|report| report.rows)
        .map_err(|error| refused("query", &error.refusal()))?;
    let text = |value: &Value| value.as_str().map(str::to_owned);
    Ok((rows.iter())
        .filter_map(|row| match row.as_slice() {
            [
                Value::String(path),
                Value::String(record_type),
                title,
                status,
            ] => Some((
                path.clone(),
                (record_type.clone(), text(title), text(status)),
            )),
            _ => None,
        })
        .collect())
}

/// The commits on the board's branch since the start of `day` in UTC,
/// leaving out bulk commits, or none when git cannot count them.
fn commits_since(host: &dyn Host, day: &str) -> Option<u32> {
    let rows = (host.query(
        "SELECT repository FROM sources WHERE source = 'worktree'",
        Limit::ALL,
    ))
    .ok()?
    .rows;
    let repository = rows.first()?.first()?.as_str()?.to_owned();
    let arguments: Vec<String> = [
        "rev-list",
        "--count",
        &format!("--since={day}T00:00:00Z"),
        "--invert-grep",
        "--grep=^Board-Bulk:",
        "HEAD",
    ]
    .map(str::to_owned)
    .into();
    let run = Run {
        program: "git",
        arguments: &arguments,
        directory: Some(Path::new(&repository)),
        input: b"",
        deadline: GIT_DEADLINE,
        added: &[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
        ],
    };
    let exited = (host.run(&run).ok()).filter(|exited| exited.code == Some(0))?;
    String::from_utf8_lossy(&exited.stdout).trim().parse().ok()
}
