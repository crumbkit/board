//! The compliance suites whose `subjects` name the repository a report
//! reads, each with its newest qualifying run and the commits since it.

use std::collections::BTreeSet;

use board_model::{LogicalPath, parse_json};
use board_ops::{Host, PluginError};
use board_plugin::Target;
use serde_json::{Value, json};

use super::place::{Place, Ran, git};
use super::{FailedCase, Refused, RepositoryRead, SuiteRow, SuiteRun, Verdict};
use crate::{check, index};

/// Each suite, wherever it is.
const SUITES: &str = "SELECT logical, body FROM records WHERE type = 'runs/suite/v1' \
    AND logical IS NOT NULL ORDER BY logical";

/// Each compliance suite whose `subjects` name the repository, by its name
/// or its mount, and its newest qualifying run. A suite whose body does not
/// parse is a `rules::report::suite_unreadable` refusal.
pub(crate) fn suites(
    host: &dyn Host,
    place: &Place,
    repository: &RepositoryRead,
    refused: &mut Vec<Refused>,
) -> Result<Vec<SuiteRow>, PluginError> {
    let names = |subject: &str| {
        subject.eq_ignore_ascii_case(&repository.name)
            || (place.mount.as_ref()).is_some_and(|(mount, _)| subject == mount.as_str())
    };
    let mut rows = Vec::new();
    for (logical, body) in index::string_pairs::<Vec<(String, String)>>(host, SUITES)? {
        let Ok(suite) = parse_json(body.as_bytes()) else {
            refused.push(Refused {
                code: "rules::report::suite_unreadable".to_owned(),
                message: format!("the suite {logical}'s body is not JSON"),
                rules: vec![logical],
            });
            continue;
        };
        let compliance = suite["kind"] == "compliance";
        let subjects = (suite["subjects"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .any(names);
        if !(compliance && subjects) {
            continue;
        }
        let cases = (suite["cases"].as_array())
            .map(|cases| u64::try_from(cases.len()).expect("a count fits in u64"));
        let (run, code) = match latest(host, &logical) {
            Ok(Some(latest)) => (Some(suite_run(host, place, repository, &latest)), None),
            Ok(None) => (None, None),
            Err(code) => (None, Some(code)),
        };
        rows.push(SuiteRow {
            id: logical,
            cases,
            run,
            code,
        });
    }
    Ok(rows)
}

/// The latest qualifying run of `suite` as `runs.latest` gives it, none
/// when none qualifies, or the code of its refusal.
fn latest(host: &dyn Host, suite: &str) -> Result<Option<Value>, String> {
    let target: Target = "runs.latest"
        .parse()
        .expect("runs.latest is <plugin>.<capability>");
    let output = (host.call(&target, &json!({"suite": suite})))
        .map_err(|refusal| refusal.code.as_str().to_owned())?
        .output;
    Ok(Some(output["run"].clone()).filter(|run| !run.is_null()))
}

/// The run `latest` names, with its commit and the notes on its failed
/// cases read from the run itself, and the commits since.
fn suite_run(
    host: &dyn Host,
    place: &Place,
    repository: &RepositoryRead,
    latest: &Value,
) -> SuiteRun {
    let id = latest["path"].as_str().unwrap_or_default().to_owned();
    let record = (id.parse::<LogicalPath>().ok())
        .and_then(|path| check::read(host, BTreeSet::from([&path])).ok())
        .and_then(|mut read| read.pop_first())
        .and_then(|(_, read)| read.ok())
        .and_then(|bytes| parse_json(&bytes).ok())
        .unwrap_or(Value::Null);
    let commit = run_commit(&record);
    let failed = (latest["failed"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(|case| FailedCase {
            case: case.to_owned(),
            note: (record["results"].as_array().into_iter().flatten())
                .find(|result| result["case"] == case && result["note"].is_string())
                .and_then(|result| result["note"].as_str())
                .map(str::to_owned),
        })
        .collect();
    let finished = finished(latest);
    let (commits_since, commits_since_code) =
        match commits_since(host, place, repository, commit.as_deref()) {
            Ok(count) => (Some(count), None),
            Err(code) => (None, Some(code.to_owned())),
        };
    SuiteRun {
        id,
        finished,
        commit,
        outcome: if latest["outcome"] == "fail" {
            Verdict::Fail
        } else {
            Verdict::Pass
        },
        failed,
        commits_since,
        commits_since_code,
    }
}

/// When `latest` finished, or started when it records no finish, in UTC;
/// none when neither is a moment.
pub(crate) fn finished(latest: &Value) -> Option<String> {
    let moment = |name: &str| latest[name].as_str().and_then(board_model::moment);
    moment("finished")
        .or_else(|| moment("started"))
        .map(board_model::utc_seconds)
}

/// The commit a run's `versions.commit` names, when it is 7 to 40
/// lowercase hex digits.
pub(crate) fn run_commit(run: &Value) -> Option<String> {
    let text = run["versions"]["commit"].as_str()?;
    let hex = (7..=40).contains(&text.len())
        && (text.bytes()).all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    hex.then(|| text.to_owned())
}

/// The commits reachable from the report's commit and not from `run`'s,
/// counted by git in the repository's clone after checking that the run's
/// commit is there and is an ancestor; or why not.
fn commits_since(
    host: &dyn Host,
    place: &Place,
    repository: &RepositoryRead,
    run: Option<&str>,
) -> Result<u64, &'static str> {
    const MISSING: &str = "rules::report::run_commit_missing";
    const UNNAMED: &str = "rules::report::run_commit_unnamed";
    const NOT_ANCESTOR: &str = "rules::report::run_commit_not_ancestor";
    const NO_GIT: &str = "rules::report::no_git";
    let run = run.ok_or(UNNAMED)?;
    let (Some(clone), Some(head)) = (&place.clone, &repository.commit) else {
        return Err(NO_GIT);
    };
    let present = git(
        host,
        clone,
        &["cat-file", "-e", &format!("{run}^{{commit}}")],
    )
    .ok_or(NO_GIT)?;
    if present.code != Some(0) {
        return Err(MISSING);
    }
    let ancestor = git(host, clone, &["merge-base", "--is-ancestor", run, head]).ok_or(NO_GIT)?;
    match ancestor.code {
        Some(0) => {}
        Some(1) => return Err(NOT_ANCESTOR),
        _ => return Err(NO_GIT),
    }
    let counted = git(
        host,
        clone,
        &["rev-list", "--count", &format!("{run}..{head}")],
    )
    .and_then(Ran::ok)
    .ok_or(NO_GIT)?;
    counted.trim().parse().map_err(|_| NO_GIT)
}
