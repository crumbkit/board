//! `ci`: reads what GitHub reports for a change's head, its check runs,
//! check suites and commit statuses. Once each of them has passed, it
//! writes the check runs as a run of the change's suite and attaches the
//! run to the change.

use std::path::Path;
use std::time::{Duration, SystemTime};

use board_ops::{Host, PluginError};
use board_plugin::Target;
use schemars::JsonSchema;
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

use crate::merge::{is_full_commit, own, recorded_change};
use crate::send::output;
use crate::{gh, git, remote};

/// The conclusions that count as passed.
const PASSED: [&str; 3] = ["success", "skipped", "neutral"];

/// How long after GitHub last updated a check suite of an app other than
/// GitHub Actions, with no check runs, it stops counting. An app that runs
/// creates its first check run within seconds.
const IDLE: Duration = Duration::from_secs(600);

/// Record the check runs GitHub reports for a change's head as a run of
/// the change's suite, once every one has passed.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CiInput")]
pub(crate) struct CiInput {
    /// The change in the board this command runs in, such as
    /// /changes/lever. Its head is the commit whose check runs are read,
    /// and the run is written under its suite.
    #[schemars(regex(pattern = "^/changes/"))]
    change: String,
}

/// The run written, the head it ran on, the change it was attached to and
/// how many check runs it records, and the apps whose idle check suites
/// it left out.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CiOutput")]
pub(crate) struct CiOutput {
    run: String,
    head: String,
    change: String,
    checks: usize,
    ignored: Vec<String>,
}

/// What `gh api .../check-runs` answers, cut to what this reads.
#[derive(Deserialize)]
pub(crate) struct Runs {
    total_count: usize,
    check_runs: Vec<CheckRun>,
}

#[derive(Deserialize)]
pub(crate) struct CheckRun {
    name: String,
    status: String,
    conclusion: Option<String>,
    started_at: String,
    completed_at: Option<String>,
}

/// What `gh api .../check-suites` answers. A workflow's suite exists
/// before its jobs that wait on others have check runs.
#[derive(Deserialize)]
pub(crate) struct Suites {
    total_count: usize,
    check_suites: Vec<CheckSuite>,
}

#[derive(Deserialize)]
struct CheckSuite {
    status: String,
    conclusion: Option<String>,
    app: Option<App>,
    /// When GitHub last updated the suite: its creation, or a re-request
    /// that queued it again.
    #[serde(deserialize_with = "moment")]
    updated_at: SystemTime,
    latest_check_runs_count: usize,
}

#[derive(Deserialize)]
struct App {
    slug: String,
    name: String,
}

impl CheckSuite {
    /// The app's name when it is an app's suite other than GitHub Actions'
    /// that has not completed and has no check runs.
    fn idle(&self) -> Option<&str> {
        let app = self.app.as_ref()?;
        let idle = self.status != "completed"
            && self.latest_check_runs_count == 0
            && app.slug != "github-actions";
        idle.then_some(app.name.as_str())
    }
}

/// The moment GitHub's RFC 3339 text names.
fn moment<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
    let text = String::deserialize(deserializer)?;
    board_model::moment(&text)
        .ok_or_else(|| D::Error::custom(format!("updated_at {text} is not an RFC 3339 time")))
}

/// What `gh api .../status` answers: the commit statuses, which CI
/// outside GitHub's checks reports.
#[derive(Deserialize)]
pub(crate) struct Combined {
    total_count: usize,
    statuses: Vec<Status>,
}

#[derive(Deserialize)]
struct Status {
    context: String,
    state: String,
}

/// One check run, check suite or commit status: its name, its conclusion
/// or status, and whether it finished and passed.
struct Check {
    name: String,
    state: String,
    done: bool,
    passed: bool,
}

/// Writes the passing run of the change's suite and attaches it, or
/// refuses with `github::ci::failed`, `github::ci::pending` or
/// `github::ci::none` and writes nothing.
pub(crate) fn ci(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let CiInput { change } = PluginError::read_input("github", "ci", input)?;
    let top = git::top(host)?;
    let (owner, repo) = remote::repository(host, &top)?;
    let board = own(host)?;
    let (recorded, _, _, suite) = recorded_change(&*board, &change)?;
    // A short head is read as the one commit it names in the checkout; the
    // change records the full commit only with the run, so a refusal
    // writes nothing.
    let expanded = (recorded.as_deref())
        .filter(|head| !is_full_commit(head))
        .and_then(|short| full_commit(host, &top, short));
    let short = recorded.clone();
    let head = expanded.clone().or(recorded);
    let (Some(head), Some(suite)) = (head.filter(|head| is_full_commit(head)), suite) else {
        return Err(PluginError {
            code: "github::ci::change".to_owned(),
            message: format!(
                "the change at {change} records no suite, or no head that is a full 40-hex \
                 commit or names one commit in this checkout"
            ),
            fields: Map::new(),
            hint: Some(format!(
                "attach what it lacks with `board changes attach --path {change} --head <sha>` \
                 or `--suite /suites/<name>`"
            )),
        });
    };
    let commit = format!("repos/{owner}/{repo}/commits/{head}");
    let (runs, suites, combined) = reported(host, &commit)?;
    let now = host.settings().now;
    let ignored = judge(&change, &head, &runs, &suites, &combined, now)?;
    let finished = (runs.iter())
        .filter_map(|each| each.completed_at.clone())
        .max()
        .unwrap_or_default();
    let stamp: String = (finished.chars())
        .filter(char::is_ascii_digit)
        .take(12)
        .collect();
    if stamp.len() < 12 {
        let why = "gave no completed_at timestamp";
        return Err(unusable(&commit, "check-runs", why));
    }
    let name = change.rsplit('/').next().unwrap_or_default();
    let path = format!(
        "{suite}/runs/{}T{}Z-{name}-{}",
        &stamp[..8],
        &stamp[8..],
        &head[..7]
    );
    let results: Vec<Value> = (runs.iter())
        .map(|each| match each.conclusion.as_deref() {
            Some("success") => json!({"case": each.name, "outcome": "pass"}),
            other => json!({
                "case": each.name,
                "outcome": "skipped",
                "note": format!("GitHub concluded {}.", other.unwrap_or_default()),
            }),
        })
        .collect();
    let record = json!({
        "$type": "runs/run/v1",
        "suite": suite,
        "by": {"program": "board"},
        "written_by": "github/ci",
        "started": (runs.iter()).map(|each| each.started_at.as_str()).min(),
        "finished": finished,
        "scope": "full",
        "dirty": false,
        "results": results,
        "versions": {repo: head},
        "summary": "Read by board github ci from the check runs GitHub reports.",
    });
    (board.put(&[json!({"path": path, "record": record})]))
        .map_err(|error| PluginError::board_refused("github", "put --batch", &error.refusal()))?;
    let target: Target = "changes.attach"
        .parse()
        .expect("changes.attach is <plugin>.<capability>");
    // Attach the run, and the full commit of a short head only when the
    // change still recorded that short head when it was read again here.
    // A head attached between that read and the attach is overwritten. When
    // the read fails, the run is attached without a head.
    let mut attach = json!({"path": change, "ref": path});
    if expanded.is_some()
        && recorded_change(&*board, &change).is_ok_and(|recorded| recorded.0 == short)
    {
        attach["head"] = json!(head);
    }
    board.call(&target, &attach).map_err(|refusal| {
        let mut error = PluginError::board_refused("github", "call", &refusal);
        error.message = format!(
            "the run {path} was written but not attached: {}; attach it with `board changes \
             attach --path {change} --ref {path}`",
            error.message
        );
        error
    })?;
    Ok(output(&CiOutput {
        run: path,
        head,
        change,
        checks: runs.len(),
        ignored,
    }))
}

/// The one commit the short head `short` names in the checkout at `top`,
/// or none when it names none or several, or is not 7 or more hex digits.
fn full_commit(host: &dyn Host, top: &Path, short: &str) -> Option<String> {
    let hex = short.len() >= 7 && short.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !hex {
        return None;
    }
    let named = format!("{short}^{{commit}}");
    let arguments = [
        "rev-parse",
        "--verify",
        "--quiet",
        "--end-of-options",
        &named,
    ];
    let full = git::git(host, top, &arguments, "").ok()?;
    Some(full.trim().to_owned()).filter(|full| is_full_commit(full))
}

/// `github::gh::failed` for the endpoint `what` under `commit`, which
/// answered something unusable for `why`.
fn unusable(commit: &str, what: &str, why: &str) -> PluginError {
    gh::failed(&format!("{commit}/{what}"), why, "record the run by hand")
}

/// The check runs, check suites and commit statuses GitHub reports for
/// `commit`, or `github::gh::failed` when one has more than a page of 100.
fn reported(
    host: &dyn Host,
    commit: &str,
) -> Result<(Vec<CheckRun>, Suites, Combined), PluginError> {
    let runs: Runs = read(host, &format!("{commit}/check-runs?per_page=100"))?;
    let suites: Suites = read(host, &format!("{commit}/check-suites?per_page=100"))?;
    let combined: Combined = read(host, &format!("{commit}/status?per_page=100"))?;
    for (what, total, got) in [
        ("check-runs", runs.total_count, runs.check_runs.len()),
        (
            "check-suites",
            suites.total_count,
            suites.check_suites.len(),
        ),
        ("status", combined.total_count, combined.statuses.len()),
    ] {
        if total > got {
            return Err(unusable(commit, what, "reported more than one page of 100"));
        }
    }
    Ok((runs.check_runs, suites, combined))
}

/// What `gh api <endpoint>` answers, read as `T`, or `github::gh::failed`.
fn read<T: DeserializeOwned>(host: &dyn Host, endpoint: &str) -> Result<T, PluginError> {
    let answer = gh::api(host, endpoint)?
        .ok_or_else(|| gh::failed(endpoint, "answered 404", "push the head first"))?;
    serde_json::from_value(Value::Object(answer)).map_err(|error| {
        let why = format!("answered in a shape this cannot read: {error}");
        gh::failed(endpoint, &why, "run it yourself to see the answer")
    })
}

/// The check run or check suite `name` with `status` and `conclusion`: it
/// passed once completed with one of [`PASSED`].
fn checked(name: &str, status: &str, conclusion: Option<&str>) -> Check {
    let done = status == "completed";
    let state = conclusion.filter(|_| done).unwrap_or(status);
    let passed = done && PASSED.contains(&state);
    Check {
        name: name.to_owned(),
        state: state.to_owned(),
        done,
        passed,
    }
}

/// The app names of the idle check suites it leaves out, or refuses with
/// `github::ci::failed` naming each check that finished without passing,
/// else `github::ci::pending` naming each not finished, else
/// `github::ci::none` when no check run concluded success.
pub(crate) fn judge(
    change: &str,
    head: &str,
    runs: &[CheckRun],
    suites: &Suites,
    combined: &Combined,
    now: SystemTime,
) -> Result<Vec<String>, PluginError> {
    let (mut ignored, mut young) = (Vec::new(), Vec::new());
    let mut counted = Vec::new();
    for suite in &suites.check_suites {
        let old = (now.duration_since(suite.updated_at)).is_ok_and(|age| age > IDLE);
        match suite.idle() {
            Some(app) if old => ignored.push(app.to_owned()),
            Some(app) => {
                young.push(app);
                counted.push(suite);
            }
            None => counted.push(suite),
        }
    }
    let ran =
        (runs.iter()).map(|each| checked(&each.name, &each.status, each.conclusion.as_deref()));
    let suites = counted.into_iter().map(|each| {
        let name = (each.app.as_ref()).map_or_else(
            || "a check suite".to_owned(),
            |app| format!("the check suite of {}", app.name),
        );
        checked(&name, &each.status, each.conclusion.as_deref())
    });
    let statuses = (combined.statuses.iter()).map(|each| Check {
        name: each.context.clone(),
        state: each.state.clone(),
        done: each.state != "pending",
        passed: each.state == "success",
    });
    let checks: Vec<Check> = ran.chain(suites).chain(statuses).collect();
    let failed = checks.iter().any(|each| each.done && !each.passed);
    let involved: Vec<&Check> = (checks.iter())
        .filter(|each| {
            if failed {
                each.done && !each.passed
            } else {
                !each.done
            }
        })
        .collect();
    let again = format!("board github ci --change {change}");
    let (code, verb, hint) = if failed {
        let hint = format!(
            "fix the failures, push, attach the new head with `board changes attach --path \
             {change} --head <sha>`, and run `{again}` once its checks pass"
        );
        ("github::ci::failed", "did not pass", hint)
    } else if !involved.is_empty() {
        let verb = if involved.len() == 1 {
            "has not finished"
        } else {
            "have not finished"
        };
        let idle = if young.is_empty() {
            String::new()
        } else {
            format!(
                "; check suites with no check runs ({}) are ignored once idle for more than {} \
                 minutes; run this again then",
                young.join(", "),
                IDLE.as_secs() / 60
            )
        };
        let hint = format!("wait for them to finish, then run `{again}` again{idle}");
        ("github::ci::pending", verb, hint)
    } else if runs
        .iter()
        .any(|each| each.conclusion.as_deref() == Some("success"))
    {
        return Ok(ignored);
    } else {
        let names: Vec<&String> = runs.iter().map(|each| &each.name).collect();
        let mut fields = Map::new();
        fields.insert("checks".to_owned(), json!(names));
        return Err(PluginError {
            code: "github::ci::none".to_owned(),
            message: format!("GitHub reports no check run for {head} that concluded success"),
            fields,
            hint: Some(format!(
                "push the head with `board github push`, wait for CI to start, then run `{again}` \
                 again"
            )),
        });
    };
    let named: Vec<&String> = involved.iter().map(|each| &each.name).collect();
    let described: Vec<String> = (involved.iter())
        .map(|each| format!("{} ({})", each.name, each.state))
        .collect();
    let mut fields = Map::new();
    fields.insert("checks".to_owned(), json!(named));
    Err(PluginError {
        code: code.to_owned(),
        message: format!(
            "of the checks GitHub reports for {head}, {} {verb}; nothing was written",
            described.join(", ")
        ),
        fields,
        hint: Some(hint),
    })
}
