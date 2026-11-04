//! Running gh, the GitHub CLI, which every capability that reaches GitHub
//! goes through.

use std::path::Path;
use std::time::Duration;

use board_ops::{Host, PluginError, Run};
use board_plugin::child::ProcessError;
use serde_json::{Map, Value};

use crate::git::{MODEL_VARIABLES, tail, unfinished};

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
        added: &[],
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
        hint: None,
    }
}

/// What a failure to send says, since gh may have sent the content before
/// it failed.
const UNSURE: &str = "it may have been sent, so look on GitHub before you run it again";

/// What `gh <arguments>` prints in `at` after reading `stdin`, or
/// `github::gh::failed` naming `named`. gh never sees the model
/// credentials.
pub(crate) fn send(
    host: &dyn Host,
    arguments: &[String],
    stdin: &str,
    named: &str,
    at: Option<&Path>,
) -> Result<String, PluginError> {
    let run = Run {
        program: "gh",
        arguments,
        directory: at,
        input: stdin.as_bytes(),
        deadline: TIMEOUT,
        added: &[],
    };
    let exited = (host.run_hiding(&run, &MODEL_VARIABLES))
        .map_err(|error| unsent(named, &unfinished(&error)))?;
    if exited.code != Some(0) {
        let stderr = String::from_utf8_lossy(&exited.stderr);
        let code = exited
            .code
            .map_or_else(|| "on a signal".to_owned(), |code| code.to_string());
        return Err(unsent(
            named,
            &format!("exited {code}: {}", tail(stderr.trim())),
        ));
    }
    Ok(String::from_utf8_lossy(&exited.stdout).into_owned())
}

/// The object GitHub answers `gh api --method <method> <endpoint>` with
/// `body`, which holds an https `html_url`.
pub(crate) fn send_json(
    host: &dyn Host,
    method: &str,
    endpoint: &str,
    body: &Value,
) -> Result<(Map<String, Value>, String), PluginError> {
    let named = format!("gh api --method {method} {endpoint}");
    let arguments = ["api", "--method", method, endpoint, "--input", "-"].map(str::to_owned);
    let printed = send(host, &arguments, &body.to_string(), &named, None)?;
    match serde_json::from_str(&printed) {
        Ok(Value::Object(answer)) => {
            let url = (answer.get("html_url").and_then(Value::as_str))
                .filter(|url| url.starts_with("https://"))
                .map(str::to_owned);
            match url {
                Some(url) => Ok((answer, url)),
                None => Err(unsent(&named, "answered without an https html_url")),
            }
        }
        _ => Err(unsent(&named, "answered without an https html_url")),
    }
}

/// `github::gh::failed` for a command that sends content, which may have
/// reached GitHub before it failed.
pub(crate) fn unsent(named: &str, why: &str) -> PluginError {
    let mut fields = Map::new();
    fields.insert("command".to_owned(), Value::from(named));
    PluginError {
        code: "github::gh::failed".to_owned(),
        message: format!("`{named}` {why}; {UNSURE}, and {CHECK_GH}"),
        fields,
        hint: None,
    }
}
