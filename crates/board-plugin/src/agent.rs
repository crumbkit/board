//! A clean-context model agent: the argv a person names in a variable, given
//! a prompt on stdin. A built-in plugin starts one through this module, so
//! the process module stays the one owner of child processes and signals.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::process::{self, Finished, Limits, Stopped};

/// How much of the agent's stdout or stderr an error carries.
pub const TAIL: usize = 4096;

/// The agent `variable` names for `plugin`, as its argv.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub plugin: String,
    pub variable: String,
    pub argv: Vec<String>,
}

/// A refusal in the plugin's own namespace, such as
/// `controls::agent::timeout`, with its message and fields.
#[derive(Debug, PartialEq)]
pub struct Refused {
    pub code: String,
    pub message: String,
    pub fields: Map<String, Value>,
}

impl Agent {
    /// The agent whose argv `value`, the value of `variable`, holds as a
    /// JSON array of strings, or `<plugin>::agent::not_set` or `invalid`.
    pub fn named(plugin: &str, variable: &str, value: Option<&OsString>) -> Result<Self, Refused> {
        let Some(value) = value else {
            return Err(refused(
                plugin,
                "not_set",
                format!(
                    "{variable} is not set; set it to the agent's argv as a JSON array of strings, \
                     as README.md shows"
                ),
                [("variable", Value::from(variable))],
            ));
        };
        let text = value.to_string_lossy();
        let parsed: Option<Vec<String>> = serde_json::from_str(&text).ok();
        // An argument may be empty, as in `--tools ""`; the program may not.
        match parsed {
            Some(argv) if argv.first().is_some_and(|program| !program.is_empty()) => Ok(Self {
                plugin: plugin.to_owned(),
                variable: variable.to_owned(),
                argv,
            }),
            _ => {
                let shown: String = text.chars().take(200).collect();
                Err(refused(
                    plugin,
                    "invalid",
                    format!(
                        "{variable} is {shown:?}, which is not a JSON array of strings whose first \
                         names a program; set it to one, such as [\"claude\", \"-p\"]"
                    ),
                    [("variable", Value::from(variable))],
                ))
            }
        }
    }

    /// What the agent printed on stdout, given `prompt` on stdin in a new
    /// empty directory with only `environment`.
    ///
    /// `hint` is what to try first when the agent runs past `timeout`, such
    /// as "shorten the part or the rubric". The empty directory holds no
    /// project settings or MCP servers. The agent leads its own process
    /// group, which is killed when the agent exits, after `timeout`, or when
    /// Board gets SIGINT, SIGTERM or SIGHUP, so neither the agent nor a
    /// process it started outlives the call.
    pub fn ask(
        &self,
        environment: &[(String, OsString)],
        prompt: &str,
        timeout: Duration,
        hint: &str,
    ) -> Result<String, Refused> {
        let program = &self.argv[0];
        let not_started = |error: &dyn std::fmt::Display| {
            refused(
                &self.plugin,
                "not_started",
                format!(
                    "the agent {program:?} cannot be started: {error}; change {}",
                    self.variable
                ),
                [("command", Value::from(self.argv.clone()))],
            )
        };
        let empty = tempfile::Builder::new()
            .prefix(&format!("board-{}-", self.plugin))
            .tempdir()
            .map_err(|error| not_started(&error))?;
        let environment: Vec<(&str, OsString)> = (environment.iter())
            .map(|(name, value)| (name.as_str(), value.clone()))
            .collect();
        let limits = Limits {
            time: timeout,
            ..Limits::default()
        };
        let (_, finished) = process::run_alone(
            Path::new(program),
            &self.argv[1..],
            empty.path(),
            &environment,
            prompt.as_bytes().to_vec(),
            limits,
        );
        let Finished {
            code,
            stdout,
            stderr,
        } = match finished {
            Ok(finished) => finished,
            Err(Stopped::Io(error)) => return Err(not_started(&error)),
            // Board keeps no stdout of a stopped child, so unlike the Python
            // runner this refusal carries no stdout tail; changing `Stopped`
            // would touch the signing module, whose changes need the owner.
            Err(Stopped::Timeout(_)) => {
                let seconds = timeout.as_secs_f64();
                return Err(refused(
                    &self.plugin,
                    "timeout",
                    format!(
                        "the agent {program:?} ran past {seconds} seconds and was stopped; {hint}, \
                         or make the agent {} names answer sooner",
                        self.variable
                    ),
                    [("seconds", Value::from(seconds))],
                ));
            }
            Err(Stopped::TooLarge(stderr)) => {
                return Err(refused(
                    &self.plugin,
                    "failed",
                    format!(
                        "the agent {program:?} printed more than {} bytes and was stopped; change {}",
                        limits.stdout, self.variable
                    ),
                    [
                        ("exit", Value::Null),
                        ("stderr", Value::from(tail(&stderr))),
                    ],
                ));
            }
        };
        let stdout = String::from_utf8_lossy(&stdout).into_owned();
        if code != Some(0) {
            let stderr = tail(&stderr);
            let short = |text: &str| -> String {
                let text = text.trim();
                let start = text.len().saturating_sub(200);
                text[floor(text, start)..].to_owned()
            };
            let exit = code.map_or_else(|| "on a signal".to_owned(), |code| code.to_string());
            return Err(refused(
                &self.plugin,
                "failed",
                format!(
                    "the agent {program:?} exited {exit}; its stdout ends: {}; its stderr ends: {}",
                    short(&stdout),
                    short(&stderr)
                ),
                [
                    ("exit", Value::from(code)),
                    ("stderr", Value::from(stderr)),
                    ("stdout", Value::from(tail(stdout.as_bytes()))),
                ],
            ));
        }
        Ok(stdout)
    }
}

/// The agent's answer, from `claude -p --output-format json`'s envelope when
/// stdout is one, else stdout itself.
pub fn answer(stdout: &str) -> String {
    match serde_json::from_str::<Value>(stdout) {
        Ok(Value::Object(envelope)) => match envelope.get("result") {
            Some(Value::String(result)) => result.clone(),
            _ => stdout.to_owned(),
        },
        _ => stdout.to_owned(),
    }
}

/// The last JSON object in `answer` that holds `key`.
pub fn last_object(answer: &str, key: &str) -> Option<Map<String, Value>> {
    let starts = answer.match_indices('{').map(|(at, _)| at).rev();
    starts.into_iter().find_map(|start| {
        let mut objects = serde_json::Deserializer::from_str(&answer[start..]).into_iter::<Value>();
        match objects.next() {
            Some(Ok(Value::Object(found))) if found.contains_key(key) => Some(found),
            _ => None,
        }
    })
}

/// The last `TAIL` bytes of `bytes` as text.
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let start = text.len().saturating_sub(TAIL);
    text[floor(&text, start)..].to_owned()
}

/// The first character boundary of `text` at or after `at`.
fn floor(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

fn refused<const N: usize>(
    plugin: &str,
    reason: &str,
    message: String,
    fields: [(&str, Value); N],
) -> Refused {
    Refused {
        code: format!("{plugin}::agent::{reason}"),
        message,
        fields: (fields.into_iter())
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests;
