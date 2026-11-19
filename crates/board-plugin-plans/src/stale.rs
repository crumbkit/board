//! `plans.stale`: lists the work nobody tends and, with `apply`, asks the
//! owner about each item in one batch of `plans/question/v1` records.
//!
//! Only the board's own records are read; a mounted board runs its own
//! sweep, so a question about its work is filed in it.
//!
//! Stale work is a change in the plan phase whose route is `unowned` (its
//! repository names no code owner, which is how Board records a change with no owner), an
//! eval that is planned or running with no verdict, and an open task with no
//! owner. A change or eval is stale when its last activity is more than
//! `days` ago, and a task when its last activity is more than `task_days`
//! ago. Activity is the newest of the record's own day (a change's `touched`
//! and plan, an eval's runs, a task's `reviewed`) and the newest commit that
//! touched its file, or for a change the files under its folder, in the
//! repository that holds it. A project task and a deferred task are left out,
//! because their owner chose not to work on them now.
//!
//! Each item gets the question `Drop KIND PATH?` with the options `drop`
//! and `keep`, recommending `drop`. `plans.ask` files it, merges it into an
//! open question that asks the same words, or finds it answered; an item
//! whose question was answered is left out of the list and counted.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use board_model::utc_seconds;
use board_ops::{Host, Limit, PluginError, Run};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ask;
use crate::sweep::{answered, words};

/// The most items the output lists (STANDARDS 23).
const LISTED: usize = 100;
/// How long one `git log` may take.
const GIT_DEADLINE: Duration = Duration::from_secs(20);
const SECONDS_PER_DAY: u64 = 86_400;

/// Which work is stale, who asks, and whether to file the questions.
#[derive(Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "StaleInput")]
pub(crate) struct Input {
    /// A change or eval is stale after this many days without activity.
    #[schemars(range(min = 1, max = 3650))]
    days: u16,
    /// A task is stale after this many days without activity.
    #[schemars(range(min = 1, max = 3650))]
    task_days: u16,
    /// The project the questions are filed under, `board` when left out.
    #[schemars(regex(pattern = r"^[a-z0-9][a-z0-9._-]*$"))]
    project: String,
    /// The `local_` id of the asking session; required with `apply`.
    #[schemars(regex(pattern = r"^local_[0-9a-f-]+$"))]
    session: Option<String>,
    /// File the questions; without it nothing is written.
    apply: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            days: 7,
            task_days: 30,
            project: "board".to_owned(),
            session: None,
            apply: false,
        }
    }
}

/// The stale items, the questions filed for them, and how each fared.
///
/// `items` lists at most 100, with `total` and `truncated`. `counts` gives
/// the number of each kind, and `already_answered` how many stale items were
/// left out because the owner answered their question. `outcomes` counts
/// what `plans.ask` did with the questions: `filed`, `merged` or `answered`.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "StaleOutput")]
pub(crate) struct Output {
    applied: bool,
    items: Vec<Item>,
    total: usize,
    truncated: bool,
    counts: BTreeMap<&'static str, usize>,
    already_answered: usize,
    outcomes: BTreeMap<&'static str, usize>,
}

/// One stale item and what is proposed for it.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Item {
    path: String,
    /// `change`, `eval` or `task`.
    kind: &'static str,
    /// The newest day anything shows activity; absent when nothing does.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_activity: Option<String>,
    /// The one-line reason for the proposal.
    reason: String,
    /// `drop`; the owner may choose `keep` and assign an owner instead.
    recommendation: &'static str,
    /// The question filed or merged into, with `apply`.
    #[serde(skip_serializing_if = "Option::is_none")]
    question: Option<String>,
}

/// A record that may be stale, before its git activity is known.
struct Candidate {
    path: String,
    kind: &'static str,
    source: String,
    file: String,
    /// The newest day the record's own fields show.
    day: Option<String>,
    /// How many days without activity make it stale.
    window: u16,
}

pub(crate) fn stale(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input("plans", "stale", input)?;
    let session = match (&given.session, given.apply) {
        (None, true) => {
            return Err(PluginError {
                code: "plans::stale::no_session".to_owned(),
                message: "plans.stale with apply files questions, and a question names the \
                          session that asks it; none was given"
                    .to_owned(),
                fields: serde_json::Map::new(),
                hint: Some("give --session with your local_ id".to_owned()),
            });
        }
        (session, _) => session.clone(),
    };
    let now = host.settings().now;
    let mut candidates = changes(host, &given)?;
    candidates.extend(evals(host, &given)?);
    candidates.extend(tasks(host, &given)?);
    let activity = commits(host, &candidates, now, &given);
    let folder = format!("/projects/{}", given.project);
    let answers = answered(host)?;
    let mut items = Vec::new();
    let mut already = 0;
    for each in candidates {
        let last = latest(each.day.clone(), activity.of(&each));
        if last
            .as_deref()
            .is_some_and(|day| day >= cutoff(now, each.window).as_str())
        {
            continue;
        }
        let question = question_of(each.kind, &each.path);
        if answers.contains_key(&(folder.clone(), words(&question))) {
            already += 1;
            continue;
        }
        items.push(item(&each, last, now));
    }
    items.sort_by(|a, b| (a.kind, &a.path).cmp(&(b.kind, &b.path)));
    let mut outcomes = BTreeMap::new();
    if let (true, Some(session)) = (given.apply, session) {
        for each in &mut items {
            let filed = ask::ask(host, &asking(&given.project, &session, each))?;
            let outcome = filed["outcome"].as_str().unwrap_or("filed");
            let outcome = ["filed", "merged", "answered"]
                .into_iter()
                .find(|known| *known == outcome)
                .unwrap_or("filed");
            *outcomes.entry(outcome).or_default() += 1;
            each.question = filed["path"].as_str().map(str::to_owned);
        }
    }
    let mut counts: BTreeMap<&'static str, usize> =
        [("changes", 0), ("evals", 0), ("tasks", 0)].into();
    for each in &items {
        *counts.entry(plural(each.kind)).or_default() += 1;
    }
    let total = items.len();
    items.truncate(LISTED);
    let output = Output {
        applied: given.apply,
        items,
        total,
        truncated: total > LISTED,
        counts,
        already_answered: already,
        outcomes,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

fn plural(kind: &str) -> &'static str {
    match kind {
        "change" => "changes",
        "eval" => "evals",
        _ => "tasks",
    }
}

/// The words of the question asked about `each`, which also key the dedupe.
fn question_of(kind: &str, path: &str) -> String {
    format!("Drop {kind} {path}?")
}

fn item(each: &Candidate, last: Option<String>, now: SystemTime) -> Item {
    let reason = match &last {
        Some(day) => format!(
            "{} and no activity since {day}, over {} days ago as of {}.",
            owner_clause(each.kind),
            each.window,
            &utc_seconds(now)[..10]
        ),
        None => format!(
            "{} and nothing in it or its history dates any activity.",
            owner_clause(each.kind)
        ),
    };
    Item {
        path: each.path.clone(),
        kind: each.kind,
        last_activity: last,
        reason,
        recommendation: "drop",
        question: None,
    }
}

fn owner_clause(kind: &str) -> &'static str {
    match kind {
        "change" => "Its route is unowned and it is still in the plan phase",
        "eval" => "No verdict yet",
        _ => "Never claimed",
    }
}

/// The `plans.ask` input for the question about `each`.
fn asking(project: &str, session: &str, each: &Item) -> Value {
    json!({
        "project": project,
        "question": {
            "question": question_of(each.kind, &each.path),
            "options": [
                {"id": "drop", "text": "Drop it."},
                {"id": "keep", "text": "Keep it, and say who owns it."},
            ],
            "recommendation": "drop",
            "why": each.reason,
            "by": {"session": session},
        },
    })
}

/// The first 10 characters, a UTC day, of the day `window` days before `now`.
fn cutoff(now: SystemTime, window: u16) -> String {
    let then = now - Duration::from_secs(u64::from(window) * SECONDS_PER_DAY);
    utc_seconds(then)[..10].to_owned()
}

/// The later of two days; days are `YYYY-MM-DD`, so text order is day order.
fn latest(a: Option<String>, b: Option<String>) -> Option<String> {
    a.max(b)
}

fn rows(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    let report = (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("plans", "query", &error.refusal()))?;
    Ok(report.rows)
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// The day of a timestamp or day text: its first 10 characters.
fn day_of(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    let day = text.get(..10)?;
    let shaped = day.bytes().enumerate().all(|(at, byte)| match at {
        4 | 7 => byte == b'-',
        _ => byte.is_ascii_digit(),
    });
    shaped.then(|| day.to_owned())
}

/// The day a plan's id names, such as `/changes/x/plans/20261104T193513907334Z`.
fn plan_day(plan: &Value) -> Option<String> {
    let id = plan.as_str()?.rsplit('/').next()?;
    let id = id.strip_suffix(".md").unwrap_or(id);
    let digits = id
        .get(..8)
        .filter(|day| day.bytes().all(|byte| byte.is_ascii_digit()))?;
    (id.as_bytes().get(8) == Some(&b'T'))
        .then(|| format!("{}-{}-{}", &digits[..4], &digits[4..6], &digits[6..]))
}

fn changes(host: &dyn Host, given: &Input) -> Result<Vec<Candidate>, PluginError> {
    let sql = "SELECT logical, source, path, json_extract(body, '$.touched'), \
               json_extract(body, '$.plan') FROM records \
               WHERE type = 'changes/change/v2' AND '/' || path IN (logical, logical || '.json') \
               AND json_extract(body, '$.phase') = 'plan' \
               AND json_extract(body, '$.route') = 'unowned'";
    let mut found = Vec::new();
    for row in rows(host, sql)? {
        let (Some(path), Some(source), Some(file)) = (text(&row[0]), text(&row[1]), text(&row[2]))
        else {
            continue;
        };
        found.push(Candidate {
            path,
            kind: "change",
            source,
            file,
            day: latest(day_of(&row[3]), plan_day(&row[4])),
            window: given.days,
        });
    }
    Ok(found)
}

fn evals(host: &dyn Host, given: &Input) -> Result<Vec<Candidate>, PluginError> {
    let sql = "SELECT logical, source, path, json_extract(body, '$.runs') FROM records \
               WHERE type = 'evals/eval/v1' AND '/' || path IN (logical, logical || '.json') \
               AND json_extract(body, '$.status') IN ('planned', 'running') \
               AND json_extract(body, '$.verdict') IS NULL";
    let mut run_days = BTreeMap::new();
    let runs = "SELECT logical, COALESCE(json_extract(body, '$.finished'), \
                json_extract(body, '$.started')) FROM records WHERE type = 'runs/run/v1'";
    for row in rows(host, runs)? {
        if let (Some(path), Some(day)) = (text(&row[0]), day_of(&row[1])) {
            run_days.insert(path, day);
        }
    }
    let mut found = Vec::new();
    for row in rows(host, sql)? {
        let (Some(path), Some(source), Some(file)) = (text(&row[0]), text(&row[1]), text(&row[2]))
        else {
            continue;
        };
        let listed: Vec<String> = row[3]
            .as_str()
            .and_then(|runs| serde_json::from_str(runs).ok())
            .unwrap_or_default();
        let day = listed
            .iter()
            .filter_map(|run| run_days.get(run))
            .max()
            .cloned();
        found.push(Candidate {
            path,
            kind: "eval",
            source,
            file,
            day,
            window: given.days,
        });
    }
    Ok(found)
}

fn tasks(host: &dyn Host, given: &Input) -> Result<Vec<Candidate>, PluginError> {
    let sql = "SELECT logical, source, path, json_extract(body, '$.reviewed') FROM records \
               WHERE type = 'tasks/task/v2' AND '/' || path IN (logical, logical || '.json') \
               AND json_extract(body, '$.status') = 'open' \
               AND json_extract(body, '$.owner') IS NULL \
               AND json_extract(body, '$.defer') IS NULL \
               AND COALESCE(json_extract(body, '$.kind'), 'task') = 'task'";
    let mut found = Vec::new();
    for row in rows(host, sql)? {
        let (Some(path), Some(source), Some(file)) = (text(&row[0]), text(&row[1]), text(&row[2]))
        else {
            continue;
        };
        found.push(Candidate {
            path,
            kind: "task",
            source,
            file,
            day: day_of(&row[3]),
            window: given.task_days,
        });
    }
    Ok(found)
}

/// The newest commit day of each file, by the source that holds it.
struct Commits(BTreeMap<String, BTreeMap<String, String>>);

impl Commits {
    /// The newest commit day of the candidate's file or, for a change, of a
    /// file in its folder.
    fn of(&self, each: &Candidate) -> Option<String> {
        let files = self.0.get(&each.source)?;
        let mut newest = files.get(&each.file).cloned();
        if let Some(folder) = each
            .file
            .strip_suffix(".json")
            .map(|stem| format!("{stem}/"))
        {
            for (_, day) in files
                .range(folder.clone()..)
                .take_while(|(file, _)| file.starts_with(&folder))
            {
                newest = newest.max(Some(day.clone()));
            }
        }
        newest
    }
}

/// For each source the candidates come from, the files its repository
/// changed within the longest window, each with its newest commit day. A
/// repository that git cannot read gives no activity.
fn commits(host: &dyn Host, candidates: &[Candidate], now: SystemTime, given: &Input) -> Commits {
    let mut found = BTreeMap::new();
    let since = cutoff(now, given.days.max(given.task_days));
    let Ok(sources) = rows(host, "SELECT source, repository FROM sources") else {
        return Commits(found);
    };
    for row in sources {
        let (Some(source), Some(repository)) = (text(&row[0]), text(&row[1])) else {
            continue;
        };
        if !candidates.iter().any(|each| each.source == source) {
            continue;
        }
        if let Some(files) = log(host, Path::new(&repository), &since) {
            found.insert(source, files);
        }
    }
    Commits(found)
}

/// Each file changed since `since` in `repository`, with its newest commit
/// day, or none when git failed.
fn log(host: &dyn Host, repository: &Path, since: &str) -> Option<BTreeMap<String, String>> {
    let arguments: Vec<String> = [
        "log",
        &format!("--since={since}T00:00:00Z"),
        "--name-only",
        "--no-renames",
        "--invert-grep",
        "--grep=^Board-Bulk:",
        "--format=%cs",
    ]
    .map(str::to_owned)
    .into();
    let run = Run {
        program: "git",
        arguments: &arguments,
        directory: Some(repository),
        input: b"",
        deadline: GIT_DEADLINE,
        added: &[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
        ],
    };
    let exited = host
        .run(&run)
        .ok()
        .filter(|exited| exited.code == Some(0))?;
    let mut files = BTreeMap::new();
    let mut day = String::new();
    for line in String::from_utf8_lossy(&exited.stdout).lines() {
        if line.is_empty() {
            continue;
        }
        if day_of(&Value::String(line.to_owned())).is_some() && line.len() == 10 {
            line.clone_into(&mut day);
        } else {
            // Newest first, so the first day a file shows is its newest.
            files.entry(line.to_owned()).or_insert_with(|| day.clone());
        }
    }
    Some(files)
}

#[cfg(test)]
mod tests;
