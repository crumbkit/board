//! `post`: comment a report on a pull request with gh, once the owner
//! approves the exact comment.

use std::time::Duration;

use board_model::LogicalPath;
use board_ops::{Host, PluginError, Run, Shown};
use board_plugin::child::ProcessError;
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::records::ReportRecord;
use crate::{PLUGIN, read_record, refusal, refused};

/// What `post` passes gh: its token, or the directory that holds its login.
pub(crate) const ENV: [&str; 3] = ["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"];
/// How long `post` may take, in seconds: one decide and one `gh pr
/// comment`.
pub(crate) const TIMEOUT: u64 = 60;
/// How long gh may run: less than the capability's timeout, so the plugin
/// can report a stopped gh.
const GH_TIMEOUT: Duration = Duration::from_secs(TIMEOUT - 20);
/// How much of gh's stderr a failure's message carries.
const TAIL: usize = 1000;

/// Post a report as a comment on a pull request, once the owner approves
/// the comment.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PostInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = r"^/reviews/[^/]+/report$"),
        extend("format" = "board-path"),
        description = "The report to post, by its logical path, such as /reviews/fix-sync/report."
    )]
    pub(crate) report: String,
    #[schemars(
        regex(pattern = r"^https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/pull/[0-9]+$"),
        description = "The pull request to comment on, by its URL, such as \
                       https://github.com/o/r/pull/7."
    )]
    pub(crate) pr: String,
}

/// The comment's URL, from the report's `source` when it was already
/// posted, and whether this call posted it.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "PostOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) comment: String,
    pub(crate) posted: bool,
}

/// Ask the owner with the whole comment, then post it with `gh pr comment`
/// and set the report's `source`.
///
/// A report whose `source` is set has been posted, or came from GitHub, so
/// it is skipped without asking. The comment names the pull request, so an
/// approval admits only that text on that pull request.
pub(crate) fn post(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "post", input)?;
    let (path, pr) = (given.report, given.pr);
    let (mut body, blob) = read_record(host, &path, ReportRecord::TYPE, "report")?;
    let report: ReportRecord = serde_json::from_value(body.clone()).map_err(|_| {
        refusal(
            "review::report::not_found",
            format!(
                "the report at {path} does not read as {}",
                ReportRecord::TYPE
            ),
            [("path", json!(path))],
        )
    })?;
    let output = |comment: String, posted: bool| {
        serde_json::to_value(Output { comment, posted }).expect("the output is a string and a bool")
    };
    if let Some(source) = &report.source {
        return Ok(output(source.clone(), false));
    }
    let text = comment(&path, &pr, &report);
    let logical: LogicalPath = (path.parse()).expect("the input schema admits only a logical path");
    let shown = Shown::Text(text.clone().into_bytes());
    (host.decide("review.post", &logical, Some(&shown)))
        .map_err(|refusal| refused("decide", &refusal))?;
    let url = gh(host, &pr, &text)?;
    body["source"] = json!(url);
    let line = json!({"path": path, "expect": blob, "record": body});
    (host.put(&[line])).map_err(|error| refused("put --batch", &error.refusal()))?;
    Ok(output(url, true))
}

/// The comment for `report`: verdict, author, range or subject, findings
/// and the report it follows, then its target.
///
/// An agent wrote the report from an untrusted diff, so every @ is followed
/// by a zero-width joiner and mentions no one.
pub(crate) fn comment(path: &str, pr: &str, report: &ReportRecord) -> String {
    let reviewed = match (&report.subject, &report.range) {
        (Some(subject), _) => format!("Record `{}` at `{}`.", subject.record, subject.blob),
        (None, range) => format!("Commits `{}`.", range.as_deref().unwrap_or_default()),
    };
    let mut lines = vec![
        match &report.by {
            Some(by) => format!("**Review: {}** by {by}", report.verdict.as_str()),
            None => format!("**Review: {}**", report.verdict.as_str()),
        },
        String::new(),
        reviewed,
        String::new(),
    ];
    for finding in &report.findings {
        let line = finding
            .line
            .map(|line| format!(":{line}"))
            .unwrap_or_default();
        let place = format!("{}{line}", finding.file.as_deref().unwrap_or_default());
        let place = if place.is_empty() {
            place
        } else {
            format!("{} ", code(&place))
        };
        lines.push(format!(
            "- **{}** {place}{}",
            finding.severity.as_str(),
            finding.text
        ));
    }
    if report.findings.is_empty() {
        lines.push("No findings.".to_owned());
    }
    if let Some(follows) = &report.follows {
        lines.extend([String::new(), format!("Follows {follows}.")]);
    }
    lines.extend([
        String::new(),
        "---".to_owned(),
        format!("Report {path}, for {pr}."),
        String::new(),
    ]);
    lines.join("\n").replace('@', "@\u{200d}")
}

/// `text` as a Markdown code span whose fence is longer than any run of
/// backticks in it.
pub(crate) fn code(text: &str) -> String {
    let longest = (text.split(|character| character != '`'))
        .map(str::len)
        .max()
        .unwrap_or(0);
    if longest == 0 {
        return format!("`{text}`");
    }
    let fence = "`".repeat(longest + 1);
    format!("{fence} {text} {fence}")
}

/// The URL `gh pr comment` prints after posting `text` on `pr`, or
/// `review::gh::failed`.
///
/// Board sets an error's `stderr` field to the plugin's own, so gh's goes
/// in the message.
fn gh(host: &dyn Host, pr: &str, text: &str) -> Result<String, PluginError> {
    let arguments = ["pr", "comment", pr, "--body-file", "-"].map(str::to_owned);
    let run = Run {
        program: "gh",
        arguments: &arguments,
        directory: None,
        input: text.as_bytes(),
        deadline: GH_TIMEOUT,
        added: &[],
    };
    let (exit, why) = match host.run(&run) {
        Ok(exited) => {
            let url = String::from_utf8_lossy(&exited.stdout).trim().to_owned();
            if exited.code == Some(0) && url.starts_with(&format!("{pr}#issuecomment-")) {
                return Ok(url);
            }
            let why = if exited.code == Some(0) {
                let shown: String = url.chars().take(200).collect();
                format!("printed {shown:?}, not the comment's URL, and may have posted the comment")
            } else {
                let stderr = String::from_utf8_lossy(&exited.stderr);
                let exit =
                    (exited.code).map_or_else(|| "on a signal".to_owned(), |code| code.to_string());
                format!(
                    "exited {exit}: {}",
                    board_ops::agent::tail(stderr.trim(), TAIL)
                )
            };
            (exited.code, why)
        }
        Err(ProcessError::NotStarted { source }) => (
            None,
            format!("did not finish, and may have posted the comment: {source}"),
        ),
        Err(ProcessError::Timeout { .. }) => (
            None,
            format!(
                "did not finish, and may have posted the comment: it ran past {} seconds",
                GH_TIMEOUT.as_secs()
            ),
        ),
        Err(ProcessError::TooLarge { limit, .. }) => (
            None,
            format!(
                "did not finish, and may have posted the comment: it printed more than {limit} bytes"
            ),
        ),
    };
    Err(refusal(
        "review::gh::failed",
        format!(
            "`gh pr comment {pr}` {why}; check gh on PATH and `gh auth status`, and look for the \
             comment on {pr} before posting again, since the approval admits the same comment \
             again without asking"
        ),
        [("pull_request", json!(pr)), ("exit", json!(exit))],
    ))
}
