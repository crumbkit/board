//! `ci`: reads the check runs GitHub reports for a change's head and, once
//! every one has passed, writes them as a run of the change's suite and
//! attaches the run and the head to the change.

use board_model::LogicalPath;
use board_ops::{Host, PluginError};
use board_plugin::Target;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::merge::own;
use crate::send::output;
use crate::{gh, git, remote};

/// The conclusions that count as passed, and the one a result marks pass.
const PASSED: [&str; 3] = ["success", "skipped", "neutral"];
const SUCCESS: &str = "success";

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
struct Answer {
    total_count: usize,
    check_runs: Vec<CheckRun>,
}

/// One check run.
#[derive(Deserialize)]
struct CheckRun {
    name: String,
    status: String,
    conclusion: Option<String>,
    started_at: String,
    completed_at: Option<String>,
}

/// Writes the passing run of the change's suite and attaches it, or
/// refuses with `github::ci::pending`, `github::ci::failed` or
/// `github::ci::none` and writes nothing.
pub(crate) fn ci(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let CiInput { change } = PluginError::read_input("github", "ci", input)?;
    let top = git::top(host)?;
    let (owner, repo) = remote::repository(host, &top)?;
    let board = own(host)?;
    let (head, suite) = recorded(&*board, &change)?;
    let endpoint = format!("repos/{owner}/{repo}/commits/{head}/check-runs?per_page=100");
    let unreadable = |why: &str| gh::failed(&endpoint, why, "run it yourself to see the answer");
    let answer = gh::api(host, &endpoint)?
        .ok_or_else(|| unreadable("answered 404; push the head first, or check its repository"))?;
    let Answer {
        total_count,
        check_runs,
    } = serde_json::from_value(Value::Object(answer))
        .map_err(|error| unreadable(&format!("answered check runs this cannot read: {error}")))?;
    if total_count > check_runs.len() {
        return Err(unreadable("reported more check runs than one page of 100"));
    }
    judge(&change, &head, &check_runs)?;
    let finished = (check_runs.iter())
        .filter_map(|each| each.completed_at.clone())
        .max()
        .unwrap_or_default();
    let stamp: String = (finished.chars())
        .filter(char::is_ascii_digit)
        .take(12)
        .collect();
    if stamp.len() < 12 {
        return Err(unreadable("gave a check run no completed_at timestamp"));
    }
    let name = change.rsplit('/').next().unwrap_or_default();
    let path = format!(
        "{suite}/runs/{}T{}Z-{name}-{}",
        &stamp[..8],
        &stamp[8..],
        &head[..7]
    );
    let record = run(&owner, &repo, &head, &suite, &finished, &check_runs);
    let line = json!({"path": path, "record": record});
    (board.put(&[line]))
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
        checks: check_runs.len(),
    }))
}

/// The head and suite the change at `path` records, or
/// `github::ci::change` when it records either one not.
fn recorded(board: &dyn Host, path: &str) -> Result<(String, String), PluginError> {
    let refused =
        |refusal: &board_model::Refusal| PluginError::board_refused("github", "get", refusal);
    let logical: LogicalPath = path.parse().map_err(|_| {
        refuse(
            "change",
            format!("{path} is not a logical path"),
            "name the change as /changes/<name>".to_owned(),
            Map::new(),
        )
    })?;
    let report = (board.get_each(&[logical]))
        .map_err(|error| refused(&error.refusal("get")))?
        .into_iter()
        .next()
        .expect("get_each answers each path asked")
        .map_err(|error| refused(&error.refusal("get")))?;
    let record = report.record.map_err(|refusal| refused(&refusal))?;
    let text = |key| record.get(key).and_then(Value::as_str).map(str::to_owned);
    let head = text("head").filter(|head| {
        head.len() == 40 && (head.bytes()).all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    });
    match (head, text("suite")) {
        (Some(head), Some(suite)) => Ok((head, suite)),
        (head, _) => {
            let (missing, then) = if head.is_none() {
                ("full 40-hex head", "--head <sha>")
            } else {
                ("suite", "--suite /suites/<name>")
            };
            Err(refuse(
                "change",
                format!("the change at {path} records no {missing}"),
                format!("record it with `board changes attach --path {path} {then}`"),
                Map::new(),
            ))
        }
    }
}

/// Refuses unless every check run completed with a passing conclusion:
/// `github::ci::failed` names each that did not, else
/// `github::ci::pending` each still queued or running, and
/// `github::ci::none` says GitHub reports none.
fn judge(change: &str, head: &str, runs: &[CheckRun]) -> Result<(), PluginError> {
    let again = format!("board github ci --change {change}");
    if runs.is_empty() {
        return Err(refuse(
            "none",
            format!("GitHub reports no check runs for {head}"),
            format!(
                "push the head with `board github push`, wait for CI to start, then run `{again}` \
                 again"
            ),
            checks(&[]),
        ));
    }
    let failed: Vec<&CheckRun> = (runs.iter())
        .filter(|each| each.status == "completed")
        .filter(|each| !PASSED.contains(&each.conclusion.as_deref().unwrap_or_default()))
        .collect();
    let pending: Vec<&CheckRun> = (runs.iter())
        .filter(|each| each.status != "completed")
        .collect();
    let (code, involved, why, then) = if !failed.is_empty() {
        let named = (failed.iter()).map(|each| {
            let conclusion = each.conclusion.as_deref().unwrap_or("no conclusion");
            format!("{} ({conclusion})", each.name)
        });
        (
            "failed",
            &failed,
            format!("{} did not pass", named.collect::<Vec<_>>().join(", ")),
            format!(
                "fix the failures, push, attach the new head with `board changes attach --path \
                 {change} --head <sha>`, and run `{again}` once its checks pass"
            ),
        )
    } else if !pending.is_empty() {
        let named = (pending.iter()).map(|each| format!("{} ({})", each.name, each.status));
        (
            "pending",
            &pending,
            format!("{} has not finished", named.collect::<Vec<_>>().join(", ")),
            format!("wait for the checks to finish, then run `{again}` again"),
        )
    } else {
        return Ok(());
    };
    let names: Vec<&str> = involved.iter().map(|each| each.name.as_str()).collect();
    Err(refuse(
        code,
        format!("of the check runs GitHub reports for {head}, {why}; nothing was written"),
        then,
        checks(&names),
    ))
}

/// `fields.checks` naming `names`.
fn checks(names: &[&str]) -> Map<String, Value> {
    let mut fields = Map::new();
    fields.insert("checks".to_owned(), json!(names));
    fields
}

/// `github::ci::<code>`, saying `message`, with `hint`.
fn refuse(code: &str, message: String, hint: String, fields: Map<String, Value>) -> PluginError {
    PluginError {
        code: format!("github::ci::{code}"),
        message,
        fields,
        hint: Some(hint),
    }
}

/// The runs/run/v1 of `suite` that the passing check `runs` of
/// `owner`/`repo` at `head` make, finished at `finished`.
fn run(
    owner: &str,
    repo: &str,
    head: &str,
    suite: &str,
    finished: &str,
    runs: &[CheckRun],
) -> Value {
    let started = (runs.iter()).map(|each| each.started_at.as_str()).min();
    let results: Vec<Value> = (runs.iter())
        .map(|each| match each.conclusion.as_deref() {
            Some(SUCCESS) => json!({"case": each.name, "outcome": "pass"}),
            other => json!({
                "case": each.name,
                "outcome": "skipped",
                "note": format!("GitHub concluded {}.", other.unwrap_or_default()),
            }),
        })
        .collect();
    let skipped = runs.len()
        - (runs.iter())
            .filter(|each| each.conclusion.as_deref() == Some(SUCCESS))
            .count();
    json!({
        "$type": "runs/run/v1",
        "suite": suite,
        "by": {"program": "board"},
        "written_by": "github/ci",
        "started": started,
        "finished": finished,
        "scope": "full",
        "dirty": false,
        "results": results,
        "versions": {repo: head},
        "measures": {"checks": runs.len(), "skipped": skipped},
        "setup": format!(
            "Read by board github ci from the check runs GitHub reports for {owner}/{repo} at \
             {head}. Each result is one check run, named as GitHub names it."
        ),
        "summary": format!(
            "All {} check runs completed with success, skipped or neutral.",
            runs.len()
        ),
    })
}
