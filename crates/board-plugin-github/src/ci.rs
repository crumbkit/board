//! `ci`: reads what GitHub reports for a change's head, its check runs,
//! check suites and commit statuses, and once every one has passed writes
//! the check runs as a run of the change's suite and attaches the run and
//! the head to the change.

use board_ops::{Host, PluginError};
use board_plugin::Target;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::merge::{is_full_commit, own, recorded_change};
use crate::send::output;
use crate::{gh, git, remote};

/// The conclusions that count as passed.
const PASSED: [&str; 3] = ["success", "skipped", "neutral"];

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
/// how many check runs it records.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CiOutput")]
pub(crate) struct CiOutput {
    run: String,
    head: String,
    change: String,
    checks: usize,
}

/// What `gh api .../check-runs` answers, cut to what this reads.
#[derive(Deserialize)]
struct Runs {
    total_count: usize,
    check_runs: Vec<CheckRun>,
}

#[derive(Deserialize)]
struct CheckRun {
    name: String,
    status: String,
    conclusion: Option<String>,
    started_at: String,
    completed_at: Option<String>,
}

/// What `gh api .../check-suites` answers. A workflow's suite exists
/// before its jobs that wait on others have check runs.
#[derive(Deserialize)]
struct Suites {
    check_suites: Vec<CheckSuite>,
}

#[derive(Deserialize)]
struct CheckSuite {
    status: String,
    app: Option<App>,
}

#[derive(Deserialize)]
struct App {
    name: String,
}

/// What `gh api .../status` answers: the commit statuses, which CI
/// outside GitHub's checks reports.
#[derive(Deserialize)]
struct Combined {
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
    let (head, _, _, suite) = recorded_change(&*board, &change)?;
    let (Some(head), Some(suite)) = (head.filter(|head| is_full_commit(head)), suite) else {
        return Err(PluginError {
            code: "github::ci::change".to_owned(),
            message: format!("the change at {change} records no full 40-hex head or no suite"),
            fields: Map::new(),
            hint: Some(format!(
                "attach what it lacks with `board changes attach --path {change} --head <sha>` \
                 or `--suite /suites/<name>`"
            )),
        });
    };
    let commit = format!("repos/{owner}/{repo}/commits/{head}");
    let runs: Runs = read(host, &format!("{commit}/check-runs?per_page=100"))?;
    let suites: Suites = read(host, &format!("{commit}/check-suites?per_page=100"))?;
    let combined: Combined = read(host, &format!("{commit}/status"))?;
    let unusable = |why: &str| {
        gh::failed(
            &format!("{commit}/check-runs"),
            why,
            "record the run by hand",
        )
    };
    if runs.total_count > runs.check_runs.len() {
        return Err(unusable("reported more than 100 check runs"));
    }
    let runs = runs.check_runs;
    judge(&change, &head, &runs, &suites, &combined)?;
    let finished = (runs.iter())
        .filter_map(|each| each.completed_at.clone())
        .max();
    let finished = finished.unwrap_or_default();
    let stamp: String = (finished.chars())
        .filter(char::is_ascii_digit)
        .take(12)
        .collect();
    if stamp.len() < 12 {
        return Err(unusable("gave no completed_at timestamp"));
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
    let attach = json!({"path": change, "ref": path, "head": head});
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
    }))
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

/// Refuses with `github::ci::failed` naming each check that finished
/// without passing, else `github::ci::pending` naming each not finished,
/// else `github::ci::none` when no check run concluded success.
fn judge(
    change: &str,
    head: &str,
    runs: &[CheckRun],
    suites: &Suites,
    combined: &Combined,
) -> Result<(), PluginError> {
    let ran = (runs.iter()).map(|each| {
        let done = each.status == "completed";
        let state =
            (each.conclusion.clone().filter(|_| done)).unwrap_or_else(|| each.status.clone());
        let passed = done && PASSED.contains(&state.as_str());
        Check {
            name: each.name.clone(),
            state,
            done,
            passed,
        }
    });
    let suites = (suites.check_suites.iter()).map(|each| Check {
        name: (each.app.as_ref()).map_or_else(
            || "a check suite".to_owned(),
            |app| format!("the check suite of {}", app.name),
        ),
        state: each.status.clone(),
        done: each.status == "completed",
        passed: true,
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
        let hint = format!("wait for them to finish, then run `{again}` again");
        ("github::ci::pending", verb, hint)
    } else if runs
        .iter()
        .any(|each| each.conclusion.as_deref() == Some("success"))
    {
        return Ok(());
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
