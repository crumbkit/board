//! Running gh, the GitHub CLI, which every capability that reaches GitHub
//! goes through.

use std::time::Duration;

use board_ops::{Host, PluginError, Run};
use board_plugin::child::ProcessError;
use serde_json::{Map, Value};

/// How long gh may run.
const TIMEOUT: Duration = Duration::from_secs(20);
/// How much of gh's stderr a failure's message carries, in characters.
const TAIL: usize = 2000;
/// What gh prints on stderr when GitHub answers 404.
const NOT_FOUND: &str = "(HTTP 404)";
/// What a failure says to do when gh itself failed.
const CHECK_GH: &str = "check gh on PATH and `gh auth status`";

/// The variables gh reads for its account and configuration, which Board
/// passes a capability that runs gh.
pub(crate) const ENV: [&str; 3] = ["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"];

/// The JSON object `gh api <endpoint>` prints, or none when GitHub answers
/// 404. Anything else that is not an object is `github::gh::failed`, whose
/// message carries the end of gh's stderr.
pub(crate) fn api(
    host: &dyn Host,
    endpoint: &str,
) -> Result<Option<Map<String, Value>>, PluginError> {
    let arguments = ["api".to_owned(), endpoint.to_owned()];
    let ran = host.run(&Run {
        program: "gh",
        arguments: &arguments,
        directory: None,
        input: b"",
        deadline: TIMEOUT,
    });
    let exited = ran.map_err(|error| {
        let why = match error {
            ProcessError::NotStarted { source } => format!("did not start: {source}"),
            ProcessError::Timeout { .. } => {
                format!("did not finish: it ran past {} s", TIMEOUT.as_secs())
            }
            ProcessError::TooLarge { limit, .. } => {
                format!("did not finish: it printed more than {limit} bytes")
            }
        };
        failed(endpoint, &why, CHECK_GH)
    })?;
    let stderr = String::from_utf8_lossy(&exited.stderr);
    let stderr = stderr.trim();
    if exited.code != Some(0) {
        if stderr.contains(NOT_FOUND) {
            return Ok(None);
        }
        let code = exited
            .code
            .map_or_else(|| "on a signal".to_owned(), |code| code.to_string());
        let tail = stderr.chars().count().saturating_sub(TAIL);
        let tail: String = stderr.chars().skip(tail).collect();
        return Err(failed(
            endpoint,
            &format!("exited {code}: {tail}"),
            CHECK_GH,
        ));
    }
    match serde_json::from_slice(&exited.stdout) {
        Ok(Value::Object(answer)) => Ok(Some(answer)),
        _ => Err(failed(endpoint, "printed no JSON object", CHECK_GH)),
    }
}

/// `github::gh::failed` for `gh api <endpoint>`, saying `why` and what to
/// do `then`.
pub(crate) fn failed(endpoint: &str, why: &str, then: &str) -> PluginError {
    let mut fields = Map::new();
    fields.insert("endpoint".to_owned(), Value::from(endpoint));
    PluginError {
        code: "github::gh::failed".to_owned(),
        message: format!("`gh api {endpoint}` {why}; {then}"),
        fields,
    }
}
