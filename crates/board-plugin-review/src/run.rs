//! `run`: review the range or record a brief names with the agent
//! `BOARD_REVIEW_AGENT` names, and record the report.

use std::path::Path;
use std::time::{Duration, Instant};

use board_model::{Code, LogicalPath};
use board_ops::agent::{self, Agent};
use board_ops::{Absent, Host, PluginError, literal};
use board_plugin::shape::{Actor, Pinned, Record, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::{BriefRecord, Cost, Finding, ReportRecord, Severity, Verdict};
use crate::{PLUGIN, git, read_record, refusal, refused, root, rows};

/// The variable that holds the agent `run` starts, as a JSON array of
/// strings.
const AGENT_VARIABLE: &str = "BOARD_REVIEW_AGENT";
/// What `run` reads: the agent to start, and what that agent may read,
/// besides what Board passes every plugin.
pub(crate) const ENV: [&str; 3] = [
    AGENT_VARIABLE,
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
];
/// How long `run` may take, in seconds: an agent reading a large change
/// takes minutes.
pub(crate) const TIMEOUT: u64 = 1800;
/// How long the agent may run: a minute less than the capability's
/// timeout, so the plugin can stop it and report.
const AGENT_TIMEOUT: Duration = Duration::from_secs(TIMEOUT - 60);

const INSTRUCTIONS: &str = r#"You review {what}. You have not seen it before.
Read the brief, then the {shown}. You have no tools and no copy of the repository, so the {shown} is all you have.

Answer with your review, and end your answer with one JSON object and nothing after it:
{"verdict": "approve" | "changes" | "comment",
 "findings": [{"severity": "high" | "medium" | "low", "file": "<path>", "line": <number>, "text": "<what is wrong>"}]}
Leave out "file" and "line" for a finding about the whole {shown}. Use "changes" when any finding must be fixed.
"#;

/// Review the range or record a brief names with the agent
/// `BOARD_REVIEW_AGENT` names.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RunInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = r"^/reviews/[^/]+/brief$"),
        extend("format" = "board-path"),
        description = "The brief to run, by its logical path, such as /reviews/fix-sync/brief."
    )]
    pub(crate) brief: String,
}

/// The report written, the commits or record it covers, the verdict and
/// how many findings.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(
    rename = "RunOutput",
    deny_unknown_fields,
    extend("oneOf" = [{"required": ["range"]}, {"required": ["subject"]}])
)]
pub(crate) struct Output {
    pub(crate) report: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) subject: Option<Pinned>,
    #[schemars(schema_with = "crate::records::verdict")]
    pub(crate) verdict: Verdict,
    pub(crate) findings: usize,
}

/// Review the brief's range or subject with the agent `BOARD_REVIEW_AGENT`
/// names, then write the report beside it.
///
/// The brief comes only from the board's own tree, and the agent only from
/// the user's environment, since a board's tree may come from a pull
/// request. A range's base is replaced by the merge base of the base and
/// the head. A subject is read through Board, and must still be at its
/// pinned blob. The report is written at `/reviews/<id>/report`, beside the
/// brief at `/reviews/<id>/brief`, and a review that has one is refused.
pub(crate) fn run(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "run", input)?;
    let path = given.brief;
    let (body, _) = read_record(host, &path, BriefRecord::TYPE, "brief")?;
    let brief: BriefRecord = serde_json::from_value(body).map_err(|_| {
        refusal(
            "review::brief::not_found",
            format!("the brief at {path} does not read as {}", BriefRecord::TYPE),
            [("path", json!(path))],
        )
    })?;
    let report_path = format!("{}report", path.strip_suffix("brief").unwrap_or(&path));
    let exists = format!(
        "SELECT 1 FROM entries WHERE logical = {}",
        literal(&report_path)
    );
    if !rows(host, &exists)?.is_empty() {
        return Err(refusal(
            "review::report::exists",
            format!(
                "{report_path} already holds a report; write a brief under a new /reviews/<id> whose \
                 `follows` is {report_path}, and run that"
            ),
            [("report", json!(report_path))],
        ));
    }
    let value = (host.settings().environment.0.iter())
        .find(|(name, _)| name == AGENT_VARIABLE)
        .map(|(_, value)| value);
    let agent = Agent::named(PLUGIN, AGENT_VARIABLE, value)?;
    let mut covered = None;
    let prompt = if let Some(subject) = &brief.subject {
        let shown = record(host, subject)?;
        prompt(&path, &brief, "a record in a board", "record", &shown)
    } else {
        let repository = git::at(host, &root(host)?);
        // The schema gives a brief without a subject a range.
        let range = brief.range.as_deref().unwrap_or_default();
        let (base, head) = range.split_once("..").unwrap_or((range, ""));
        let head = repository.commit(head)?;
        // A base that moved on after the branch point would show its own
        // commits as reverted.
        let base = repository.merge_base(&repository.commit(base)?, &head)?;
        let commits = repository.commits(&base, &head)?;
        let ids = repository.patch_ids(&commits)?;
        let shown = format!(
            "Range: {base}..{head}\n\n## Diff\n\n{}",
            repository.diff(&base, &head)?
        );
        let kept = (commits.iter()).filter_map(|commit| ids.get(commit).cloned());
        covered = Some((format!("{base}..{head}"), kept.collect::<Vec<_>>()));
        prompt(
            &path,
            &brief,
            "a change to a git repository",
            "diff",
            &shown,
        )
    };
    let clock = Instant::now();
    let stdout = agent.ask(host, &prompt, AGENT_TIMEOUT, "narrow the brief's range")?;
    let (answer, tokens) = envelope(&stdout);
    let (verdict, findings) = answered(&answer, &stdout)?;
    let seconds = (clock.elapsed().as_secs_f64() * 1000.0).round() / 1000.0;
    let by = reviewer(&agent.argv[0]);
    let output = Output {
        report: report_path.clone(),
        range: covered.as_ref().map(|(range, _)| range.clone()),
        subject: brief.subject.clone(),
        verdict,
        findings: findings.len(),
    };
    let (range, patch_ids) = covered.unzip();
    let report = ReportRecord {
        tag: Tag::default(),
        brief: Some(path.clone()),
        range,
        patch_ids,
        subject: brief.subject,
        findings,
        verdict,
        by,
        cost: Some(Cost {
            seconds,
            input_tokens: tokens.map(|(input, _)| input),
            output_tokens: tokens.map(|(_, output)| output),
        }),
        source: None,
        follows: brief.follows,
    };
    let line = json!({"path": report_path, "expect": "none", "record": report});
    (host.put(&[line])).map_err(|error| refused("put --batch", &error.refusal()))?;
    Ok(serde_json::to_value(output).expect("the output is strings and numbers"))
}

/// The prompt's part for the subject: its path, blob, the references it
/// makes and its bytes.
///
/// Else `review::subject::not_found`, or `review::subject::moved` when the
/// record is no longer at its pinned blob. The blob is the one Board read
/// the bytes from, so the agent reads exactly the pinned version.
fn record(host: &dyn Host, subject: &Pinned) -> Result<String, PluginError> {
    let record = &subject.record;
    let not_found = || {
        refusal(
            "review::subject::not_found",
            format!(
                "no file is at the subject {record}; `board ls` shows what is there, so fix the \
                 brief's subject"
            ),
            [("record", json!(record))],
        )
    };
    let Ok(logical) = record.parse::<LogicalPath>() else {
        return Err(not_found());
    };
    let absent = |code: &str| code == Code::PathNotFound.as_str();
    let answers = match host.refs(std::slice::from_ref(&logical), Absent::Refuse, &[]) {
        Ok(answers) => answers,
        Err(error) if absent(error.refusal().code.as_str()) => return Err(not_found()),
        Err(error) => return Err(refused("refs", &error.refusal())),
    };
    let references = match answers.into_iter().next() {
        Some(Ok(report)) => report.from,
        Some(Err(error)) if absent(error.refusal().code.as_str()) => return Err(not_found()),
        Some(Err(error)) => return Err(refused("refs", &error.refusal())),
        None => return Err(not_found()),
    };
    let read = (host.get_each(std::slice::from_ref(&logical)))
        .map_err(|error| refused("get", &error.refusal("get")))?;
    let (bytes, current) = match read.into_iter().next() {
        Some(Ok(report)) => match report.location.object {
            board_model::Object::Blob(id) => (report.bytes, id.to_string()),
            board_model::Object::Directory => return Err(not_found()),
        },
        Some(Err(error)) if error.code() == Code::PathNotFound => return Err(not_found()),
        Some(Err(error)) => return Err(refused("get", &error.refusal("get"))),
        None => return Err(not_found()),
    };
    if current != subject.blob {
        let pinned = &subject.blob;
        return Err(refusal(
            "review::subject::moved",
            format!(
                "the subject {record} is at blob {current}, not the {pinned} the brief pins; write a \
                 new brief that pins {current} if that version is the one to review"
            ),
            [
                ("record", json!(record)),
                ("pinned", json!(pinned)),
                ("current", json!(current)),
            ],
        ));
    }
    let listed = (references.iter())
        .map(|outgoing| format!("- {}: {}\n", outgoing.pointer, outgoing.target.as_str()))
        .collect::<Vec<_>>()
        .concat();
    let listed = if listed.is_empty() {
        "- none\n".to_owned()
    } else {
        listed
    };
    Ok(format!(
        "Record: {record} at {current}\n\nReferences it makes:\n{listed}\n## Record\n\n{}",
        String::from_utf8_lossy(&bytes)
    ))
}

/// The agent's prompt: the instructions for reviewing `what` from its
/// `shown`, the brief, then `body`.
fn prompt(path: &str, brief: &BriefRecord, what: &str, shown: &str, body: &str) -> String {
    let questions = (brief.questions.iter().flatten())
        .map(|question| format!("- {question}\n"))
        .collect::<Vec<_>>()
        .concat();
    let questions = if questions.is_empty() {
        String::new()
    } else {
        format!("\nQuestions:\n{questions}")
    };
    format!(
        "{}\n## Brief {path}\n\nFocus: {}\n{questions}\n{body}",
        INSTRUCTIONS
            .replace("{what}", what)
            .replace("{shown}", shown),
        brief.focus
    )
}

/// Who reviewed: the program `program` starts, by its file name, when that
/// name is a slug such as `claude`; else nobody known.
pub(crate) fn reviewer(program: &str) -> Option<Actor> {
    let name = Path::new(program).file_name()?.to_str()?;
    let slug = !name.is_empty()
        && (name.split('-')).all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    slug.then(|| Actor {
        program: Some(name.to_owned()),
        ..Actor::default()
    })
}

/// The agent's answer and its input and output tokens: from
/// `claude -p --output-format json`'s envelope, else stdout and none.
fn envelope(stdout: &str) -> (String, Option<(u64, u64)>) {
    let parsed: Option<Map<String, Value>> = serde_json::from_str(stdout).ok();
    let Some((envelope, result)) = parsed.and_then(|envelope| {
        let result = envelope.get("result")?.as_str()?.to_owned();
        Some((envelope, result))
    }) else {
        return (stdout.to_owned(), None);
    };
    let tokens = envelope
        .get("usage")
        .and_then(Value::as_object)
        .map(|usage| {
            let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
            let inputs = [
                "input_tokens",
                "cache_creation_input_tokens",
                "cache_read_input_tokens",
            ];
            (inputs.into_iter().map(count).sum(), count("output_tokens"))
        });
    (result, tokens)
}

/// The verdict and findings of the last JSON object in `answer` that holds
/// a verdict, or `review::agent::bad_output`.
fn answered(answer: &str, stdout: &str) -> Result<(Verdict, Vec<Finding>), PluginError> {
    let found = agent::last_object(answer, "verdict").and_then(|found| checked(&found));
    found.ok_or_else(|| {
        refusal(
            "review::agent::bad_output",
            "the agent's answer does not end with a JSON object holding a verdict of approve, \
             changes or comment and findings each with a severity of high, medium or low and a \
             text; change the agent's command or prompt"
                .to_owned(),
            [("stdout", json!(agent::tail(stdout, agent::TAIL)))],
        )
    })
}

/// The verdict and findings in `found`, each as the report's schema needs
/// them, or none.
fn checked(found: &Map<String, Value>) -> Option<(Verdict, Vec<Finding>)> {
    let verdict = match found.get("verdict")?.as_str()? {
        "approve" => Verdict::Approve,
        "changes" => Verdict::Changes,
        "comment" => Verdict::Comment,
        _ => return None,
    };
    let empty = Vec::new();
    let findings = match found.get("findings") {
        None => &empty,
        Some(findings) => findings.as_array()?,
    };
    let kept = (findings.iter())
        .map(|item| {
            let item = item.as_object()?;
            let severity = match item.get("severity")?.as_str()? {
                "high" => Severity::High,
                "medium" => Severity::Medium,
                "low" => Severity::Low,
                _ => return None,
            };
            let text = item.get("text")?.as_str().filter(|text| !text.is_empty())?;
            let file = (item.get("file").and_then(Value::as_str))
                .filter(|file| !file.is_empty())
                .map(str::to_owned);
            let line = (item.get("line").and_then(Value::as_u64)).filter(|line| *line >= 1);
            Some(Finding {
                severity,
                text: text.to_owned(),
                file,
                line,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((verdict, kept))
}
