//! The `run` capability: start or resume a change, record where its run
//! stops, and build the run brief; with `inline`, return the brief, and
//! otherwise start the configured agent on it in a worktree for the change
//! and record the run.
//!
//! The agent is the argv in the `BOARD_AGENT` variable, or else `agent` in
//! the known boards file, which board-cli passes on as that variable. It
//! runs in `<clone>.worktrees/<name>` on branch `<name>`, made from the
//! branch the mount follows and reused on resume, with the brief on stdin
//! and `BOARD_CHANGE` set to the change's path. When it exits without
//! writing a `changes/stop/v1`, `run` writes one from the change's state.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use board_ops::agent::{AGENT_VARIABLE, Agent, program_name};
use board_ops::{Host, PluginError, Run, literal};
use board_plugin::child::ProcessError;
use board_plugin::shape::{Prose, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board::{self, rows, text};
use crate::brief::{Rule, RulesCommand, brief_of};
use crate::change::{line, name_in, read, touch};
use crate::diff::git;
use crate::flow::{Agent as Doer, Flow, Stop, of_change};
use crate::gates::remedy::{fill, values};
use crate::phases::{final_phase, refuse_v1, step_of};
use crate::plan::{stamp, time_of};
use crate::records::{PAUSED, Reason, RunRecord, StopRecord, is_final};
use crate::stop::{self, Shown, position};
use crate::{PLUGIN, refusal, to_value};

/// How long `run` may take, in seconds: the longest Board allows.
pub(crate) const TIMEOUT: u64 = 3600;
/// How long the agent may run: a minute less, so `run` can record it.
const AGENT_DEADLINE: Duration = Duration::from_secs(TIMEOUT - 60);
/// What `run` reads besides what Board passes every plugin: the agent, what
/// the agent may read, and the WIP limit's window for a change it starts.
pub(crate) const ENV: [&str; 4] = [
    AGENT_VARIABLE,
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
    crate::wip::ACTIVE_DAYS_VARIABLE,
];
/// The variable that names the change to the agent.
const CHANGE_VARIABLE: &str = "BOARD_CHANGE";
/// The most surfaced records the brief lists; it counts the rest.
const SURFACED_SHOWN: usize = 20;
/// How long each git command that makes the worktree may take.
const GIT_DEADLINE: Duration = Duration::from_secs(30);

/// Start a change, or resume one, and run it to a stop.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RunInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change to resume, such as /changes/suite-timeout; leave it out to \
                       start one with mount and title."
    )]
    path: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The repository's mount for a new change, such as /repos/board."
    )]
    mount: Option<String>,
    #[schemars(
        regex(pattern = "[A-Za-z0-9]"),
        description = "What a new change does, in a few words."
    )]
    title: Option<String>,
    #[schemars(
        regex(pattern = "^[a-z0-9][a-z0-9-]{0,59}$"),
        description = "A new change's name; made from the title when left out."
    )]
    name: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The task a new change does; when left out, tasks.add adds one."
    )]
    task: Option<String>,
    #[schemars(
        regex(pattern = "^[a-z0-9]+(-[a-z0-9]+)*$"),
        description = "The flow a new change follows, such as small; chosen as start chooses it \
                       when left out."
    )]
    flow: Option<String>,
    #[schemars(
        with = "Option<Prose>",
        length(min = 1),
        description = "Why a new change starts in build although the WIP limit is reached."
    )]
    over_limit: Option<String>,
    #[schemars(
        regex(pattern = "^[a-z0-9]+(-[a-z0-9]+)*$"),
        description = "Stop in this phase once its gate passes; the flow's stop when neither \
                       this nor until is given."
    )]
    through: Option<String>,
    #[schemars(
        regex(pattern = "^[a-z0-9]+(-[a-z0-9]+)*$"),
        description = "Stop as soon as the change enters this phase, or done; the flow's stop \
                       when neither this nor through is given."
    )]
    until: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "true to return the brief and start nothing, for the calling session to \
                       follow; false when left out."
    )]
    inline: bool,
    #[schemars(
        range(min = 0),
        description = "The most the agent should spend, in US dollars, which the brief and the \
                       run record carry; none when left out."
    )]
    budget: Option<f64>,
}

/// One phase the run works through: its name, who does its work, and the
/// instruction with its placeholders filled.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Step {
    phase: String,
    agent: Doer,
    instruction: String,
}

/// What an agent running a change needs: the change, where to stop and
/// how, each phase's instruction up to the stop, the records to consider,
/// and the rules on the planned paths.
///
/// `steps` runs from the change's phase to the stop: through its phase
/// with `through`, and up to it with `until`; the first step's instruction
/// is the one `next` gives now. `surfaced` lists at most 20 of the records
/// `start` surfaced, and `surfaced_left_out` counts the rest. The rules
/// fields are those of `changes.brief`, which fits them in its 600 words.
/// `stopping` says how an unattended agent stops.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct RunBrief {
    brief_line: String,
    path: String,
    board: Option<String>,
    flow: String,
    stop: Stop,
    stop_rule: String,
    steps: Vec<Step>,
    surfaced: Vec<String>,
    surfaced_left_out: usize,
    rules: Vec<Rule>,
    rules_left_out: Vec<String>,
    rules_unnamed: usize,
    rules_command: Option<RulesCommand>,
    controls: String,
    stopping: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_usd: Option<f64>,
}

/// The change, its stop and whether it is reached; with `inline`, the
/// brief; otherwise the run record, the stop record, why it stopped, the
/// agent's exit status and the worktree it ran in. A stop already reached
/// starts no agent, so those are left out.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "RunOutput")]
pub(crate) struct Output {
    path: String,
    stop: Shown,
    #[serde(skip_serializing_if = "Option::is_none")]
    brief: Option<RunBrief>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stopped: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<Reason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
}

pub(crate) fn run(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "run", input)?;
    let argv = if given.inline {
        None
    } else {
        Some(agent_argv(host)?)
    };
    let asked = asked_stop(&given)?;
    let path = change_of(host, &given)?;
    let (flow, stop) = set_stop(host, &path, asked)?;
    let (brief, shown, mount) = brief_for(host, &path, &flow, &stop, given.budget)?;
    let mut output = Output {
        path: path.clone(),
        stop: shown,
        brief: None,
        run: None,
        stopped: None,
        reason: None,
        exit: None,
        worktree: None,
    };
    let Some(argv) = argv else {
        output.brief = Some(brief);
        return Ok(to_value(output));
    };
    if output.stop.reached {
        return Ok(to_value(output));
    }
    let directory = worktree(host, &path, &mount)?;
    let ran = spawn(host, &path, &argv, &directory, &brief)?;
    let written = recorded(host, &path, &flow.name, &stop, &argv, &ran, given.budget)?;
    if let Some(now) = step_of(host, &path)?.0.stop {
        output.stop = now;
    }
    output.run = Some(written.run);
    output.reason = Some(written.reason);
    output.stopped = Some(written.stopped);
    output.exit = ran.exit;
    output.worktree = Some(directory.display().to_string());
    Ok(to_value(output))
}

/// Records `asked`, else the change's stop, else its flow's, as the stop
/// of the change at `path`, and returns its flow and the stop; refused for a change that
/// is v1, ended, paused or follows no flow, and for a phase its flow lacks.
fn set_stop(host: &dyn Host, path: &str, asked: Option<Stop>) -> Result<(Flow, Stop), PluginError> {
    let (mut change, blob) = read(host, path)?;
    refuse_v1(path, &change, "run")?;
    if is_final(&change.phase) {
        return Err(final_phase(
            path,
            &change.phase,
            format!(
                "the change at {path} is {}, so there is nothing to run; start a new one with \
                 --mount and --title",
                change.phase
            ),
        ));
    }
    if change.phase == PAUSED {
        return Err(refusal(
            "changes::phase::paused",
            format!(
                "the change at {path} is paused; resume it with `board call changes.resume --path \
                 {path}`, then run it"
            ),
            [("path", json!(path)), ("phase", json!(change.phase))],
        ));
    }
    let Some(flow) = of_change(host, path, &change)? else {
        return Err(refusal(
            "changes::run::no_flow",
            format!(
                "the change at {path} follows no flow, as a prototype does, so it has no stop; \
                 work on a prototype with its brief from `board call changes.brief --path {path}`"
            ),
            [("path", json!(path))],
        ));
    };
    let stop = (asked.or_else(|| change.stop.clone())).unwrap_or_else(|| flow.stop.clone());
    if position(&flow, stop.phase()).is_none() {
        let mut phases = flow.names();
        phases.push(crate::records::DONE.to_owned());
        return Err(refusal(
            "changes::run::no_phase",
            format!(
                "the flow {} has no phase {}, so the run cannot stop there; name one of {}",
                flow.name,
                stop.phase(),
                phases.join(", ")
            ),
            [
                ("path", json!(path)),
                ("phase", json!(stop.phase())),
                ("phases", json!(phases)),
            ],
        ));
    }
    change.stop = Some(stop.clone());
    board::put(host, &[line(path, &blob, &touch(host, change))])?;
    Ok((flow, stop))
}

/// The run brief of the change at `path`, which stops at `stop` in
/// `flow`, with the stop as `next` shows it and the change's mount.
fn brief_for(
    host: &dyn Host,
    path: &str,
    flow: &Flow,
    stop: &Stop,
    budget: Option<f64>,
) -> Result<(RunBrief, Shown, String), PluginError> {
    let (now, observed) = step_of(host, path)?;
    let shown = now.stop.clone().unwrap_or(Shown {
        stop: stop.clone(),
        reached: false,
    });
    let builder = brief_of(host, path)?;
    let filled = values(&observed);
    let from = position(flow, &now.phase).unwrap_or(0);
    let end = position(flow, stop.phase()).unwrap_or(0);
    let last = match stop {
        Stop::Through(_) => end + 1,
        Stop::Until(_) => end,
    };
    let steps = (flow.phases.iter().enumerate())
        .filter(|(at, _)| *at >= from && *at < last)
        .map(|(at, phase)| Step {
            phase: phase.name.clone(),
            agent: phase.agent,
            instruction: if at == from {
                now.instruction.clone()
            } else {
                fill(&phase.instruction, &filled)
            },
        })
        .collect();
    let mount = observed.change.mount.clone();
    let surfaced = &observed.change.surfaced;
    let brief = RunBrief {
        brief_line: builder.brief_line,
        path: path.to_owned(),
        board: observed.board.clone(),
        flow: flow.name.clone(),
        stop_rule: stop.rule(),
        stop: stop.clone(),
        steps,
        surfaced: surfaced.iter().take(SURFACED_SHOWN).cloned().collect(),
        surfaced_left_out: surfaced.len().saturating_sub(SURFACED_SHOWN),
        rules: builder.rules,
        rules_left_out: builder.rules_left_out,
        rules_unnamed: builder.rules_unnamed,
        rules_command: builder.rules_command,
        controls: builder.controls,
        stopping: stopping(path, &mount, budget),
        budget_usd: budget,
    };
    Ok((brief, shown, mount))
}

/// The stop `through` or `until` asks for, none when neither does, or
/// `changes::run::two_stops` when both do.
fn asked_stop(given: &Input) -> Result<Option<Stop>, PluginError> {
    match (&given.through, &given.until) {
        (Some(_), Some(_)) => Err(refusal(
            "changes::run::two_stops",
            "the run was given both --through and --until, and stops at one place; give one of \
             them"
                .to_owned(),
            [
                ("through", json!(given.through)),
                ("until", json!(given.until)),
            ],
        )),
        (Some(phase), None) => Ok(Some(Stop::Through(phase.clone()))),
        (None, Some(phase)) => Ok(Some(Stop::Until(phase.clone()))),
        (None, None) => Ok(None),
    }
}

/// The change to run: the one `path` names, or a new one `start` writes
/// from `mount` and `title`, else `changes::run::which_change`.
fn change_of(host: &dyn Host, given: &Input) -> Result<String, PluginError> {
    let starting = given.mount.is_some() || given.title.is_some();
    match (&given.path, &given.mount, &given.title) {
        (Some(path), _, _) if !starting => Ok(path.clone()),
        (None, Some(mount), Some(title)) => {
            let mut input = json!({"mount": mount, "title": title});
            let optional = [
                ("name", &given.name),
                ("task", &given.task),
                ("flow", &given.flow),
                ("over_limit", &given.over_limit),
            ];
            for (key, value) in optional {
                if let Some(value) = value {
                    input[key] = json!(value);
                }
            }
            let started = crate::start::start(host, &input)?;
            Ok(text(started.get("path")).unwrap_or_default().to_owned())
        }
        _ => Err(refusal(
            "changes::run::which_change",
            "the run names no one change: give --path to resume a change, or --mount and --title \
             to start one, and not both"
                .to_owned(),
            [
                ("path", json!(given.path)),
                ("mount", json!(given.mount)),
                ("title", json!(given.title)),
            ],
        )),
    }
}

/// How an unattended agent stops, for the brief.
fn stopping(path: &str, mount: &str, budget: Option<f64>) -> String {
    let budget = budget.map_or_else(String::new, |usd| {
        format!(
            " Spend at most {usd} US dollars; before you would pass it, stop with --reason budget."
        )
    });
    format!(
        "Run `board call changes.next --path {path}` at each phase. When it returns `stop` with \
         `reached` true, run `board call changes.stop --path {path} --reason reached --summary \
         '<one line>'` and exit 0. When it returns `waiting_on`, write its `ask` as a \
         plans/question/v1 with `board put`, its blocks naming {path} and its mount {mount}; then \
         run `board call changes.stop --path {path} --reason waiting --question <its path> \
         --summary '<one line>'` and exit 0. When a gate refuses what you cannot fix, stop with \
         --reason refused. Never advance past the stop: `advance` refuses it.{budget}"
    )
}

/// The agent's argv: `BOARD_AGENT`, which board-cli sets from `agent` in
/// the known boards file when the variable is unset; else
/// `changes::agent::not_set`, or `changes::agent::invalid` when it is not
/// a JSON array of strings.
fn agent_argv(host: &dyn Host) -> Result<Vec<String>, PluginError> {
    let value = (host.settings().environment.0.iter())
        .find(|(name, _)| name == AGENT_VARIABLE)
        .map(|(_, value)| value);
    if value.is_none() {
        return Err(refusal(
            "changes::agent::not_set",
            format!(
                "no agent is set to run the change; set {AGENT_VARIABLE}, or `agent` in \
                 ~/.config/board/config.json, to its argv as a JSON array of strings, such as \
                 [\"claude\", \"-p\"], or pass --inline true and follow the brief yourself"
            ),
            [("variable", json!(AGENT_VARIABLE))],
        ));
    }
    Ok(Agent::named(PLUGIN, AGENT_VARIABLE, value)?.argv)
}

/// The worktree the change's agent works in: `<clone>.worktrees/<name>` of
/// the clone the mount reads, on branch `<name>`, made from the branch the
/// mount follows when it is not there yet; the mount's own directory in
/// it. Else `changes::run::worktree`.
fn worktree(host: &dyn Host, path: &str, mount: &str) -> Result<PathBuf, PluginError> {
    let name = name_in(path);
    let failed = |why: String| {
        refusal(
            "changes::run::worktree",
            format!(
                "the worktree for {path} cannot be made: {why}; fix the clone of {mount}, or run \
                 with --inline true"
            ),
            [("path", json!(path)), ("mount", json!(mount))],
        )
    };
    let Some(answer) = board::resolve(host, mount)? else {
        return Err(failed(format!("nothing is at {mount}")));
    };
    let location = &answer.location;
    let clone = &location.board.repository;
    let follow = (location.pointers.last())
        .and_then(|pointer| pointer.follow.as_deref())
        .unwrap_or("HEAD");
    let directory = PathBuf::from(format!("{}.worktrees/{name}", clone.display()));
    if !directory.is_dir() {
        let shown = directory.display().to_string();
        let branch = format!("refs/heads/{name}");
        let held = git(
            host,
            clone,
            &["rev-parse", "--verify", "--quiet", &branch],
            GIT_DEADLINE,
        )
        .is_ok();
        let arguments: Vec<&str> = if held {
            vec!["worktree", "add", &shown, name]
        } else {
            vec!["worktree", "add", "-b", name, &shown, follow]
        };
        git(host, clone, &arguments, GIT_DEADLINE).map_err(|failure| failed(failure.why))?;
    }
    Ok(directory.join(&location.path))
}

/// What the agent did: when it started and ended, its exit status, what it
/// printed last, and why it was stopped, if it was.
struct Ran {
    started: SystemTime,
    ended: SystemTime,
    exit: Option<i32>,
    stdout: String,
    stopped: Option<String>,
}

/// Starts `argv` in `directory` with the brief on stdin and `BOARD_CHANGE`
/// set, and waits for it; `changes::agent::not_started` when it cannot
/// start.
fn spawn(
    host: &dyn Host,
    path: &str,
    argv: &[String],
    directory: &Path,
    brief: &RunBrief,
) -> Result<Ran, PluginError> {
    let text = format!(
        "{}\n\n{}\n",
        brief.brief_line,
        serde_json::to_string_pretty(brief).unwrap_or_default()
    );
    let added = [(CHANGE_VARIABLE, path)];
    let program = &argv[0];
    let run = Run {
        program,
        arguments: &argv[1..],
        directory: Some(directory),
        input: text.as_bytes(),
        deadline: AGENT_DEADLINE,
        added: &added,
    };
    let started = SystemTime::now();
    let result = host.run(&run);
    let ended = SystemTime::now();
    let mut ran = Ran {
        started,
        ended,
        exit: None,
        stdout: String::new(),
        stopped: None,
    };
    match result {
        Ok(exited) => {
            ran.exit = exited.code;
            ran.stdout = String::from_utf8_lossy(&exited.stdout).into_owned();
        }
        // Only the program is named: the arguments may hold a credential.
        Err(ProcessError::NotStarted { source }) => {
            return Err(refusal(
                "changes::agent::not_started",
                format!(
                    "the agent {program:?} cannot be started: {source}; change {AGENT_VARIABLE} \
                     or `agent` in the known boards file"
                ),
                [("program", json!(program))],
            ));
        }
        Err(ProcessError::Timeout { .. }) => {
            ran.stopped = Some(format!(
                "The agent ran past {} s and was stopped.",
                AGENT_DEADLINE.as_secs()
            ));
        }
        Err(ProcessError::TooLarge { limit, .. }) => {
            ran.stopped = Some(format!(
                "The agent printed more than {limit} bytes and was stopped."
            ));
        }
    }
    Ok(ran)
}

/// The cost in US dollars and the duration in milliseconds the agent
/// reported, when its last line of stdout is a JSON object that holds
/// `total_cost_usd` or `duration_ms`.
fn reported(stdout: &str) -> (Option<f64>, Option<u64>) {
    let last = stdout.lines().rev().find(|line| !line.trim().is_empty());
    let Some(Value::Object(found)) = last.and_then(|line| serde_json::from_str(line).ok()) else {
        return (None, None);
    };
    (
        found.get("total_cost_usd").and_then(Value::as_f64),
        found.get("duration_ms").and_then(Value::as_u64),
    )
}

/// The newest stop record of the change at `path` written since `since`,
/// a stamp, and its reason.
fn written_since(
    host: &dyn Host,
    path: &str,
    since: &str,
) -> Result<Option<(String, Reason)>, PluginError> {
    let prefix = format!("{path}/stops/");
    let sql = format!(
        "SELECT logical, json_extract(body, '$.reason') FROM records WHERE type = {} AND \
         substr(logical, 1, {}) = {} AND logical >= {} ORDER BY logical DESC LIMIT 1",
        literal(<StopRecord as board_plugin::shape::Record>::TYPE),
        prefix.len(),
        literal(&prefix),
        literal(&format!("{prefix}{since}"))
    );
    let found = rows(host, &sql)?;
    Ok(found.first().and_then(|row| {
        let reason = serde_json::from_value(Value::from(text(row.get(1))?)).ok()?;
        Some((text(row.first())?.to_owned(), reason))
    }))
}

/// The run record written, the stop record it names and why the run
/// stopped.
struct Recorded {
    run: String,
    stopped: String,
    reason: Reason,
}

/// Writes the run record of `ran`, and a stop record when the agent wrote
/// none since it started.
fn recorded(
    host: &dyn Host,
    path: &str,
    flow: &str,
    stop: &Stop,
    argv: &[String],
    ran: &Ran,
    budget: Option<f64>,
) -> Result<Recorded, PluginError> {
    let since = stamp(ran.started);
    let (cost_usd, duration_ms) = reported(&ran.stdout);
    let (stopped, reason) = if let Some(found) = written_since(host, path, &since)? {
        found
    } else {
        let over = matches!((cost_usd, budget), (Some(cost), Some(most)) if cost > most);
        let (reason, summary) = unrecorded(host, path, ran, over)?;
        (
            stop::record(host, path, reason, None, summary)?.stop,
            reason,
        )
    };
    let (change, _) = read(host, path)?;
    let run = format!("{path}/runs/{since}");
    let record = RunRecord {
        tag: Tag::default(),
        flow: flow.to_owned(),
        stop: stop.clone(),
        program: program_name(&argv[0]),
        started: time_of(ran.started),
        ended: time_of(ran.ended),
        exit: ran.exit,
        cost_usd,
        duration_ms,
        budget_usd: budget,
        sandbox: false,
        phase: change.phase,
        stopped: stopped.clone(),
    };
    board::put(
        host,
        &[json!({"path": run, "expect": "none", "record": record})],
    )?;
    Ok(Recorded {
        run,
        stopped,
        reason,
    })
}

/// Why the run of the change at `path` stopped when its agent recorded no
/// stop, and a summary: `reached` or `waiting` from the change's state
/// when the agent exited 0, `budget` when it spent more than `over`
/// allows, and `error` otherwise.
fn unrecorded(
    host: &dyn Host,
    path: &str,
    ran: &Ran,
    over: bool,
) -> Result<(Reason, String), PluginError> {
    let now = step_of(host, path)?.0;
    let reached = now.stop.as_ref().is_some_and(|shown| shown.reached);
    Ok(match (&ran.stopped, ran.exit) {
        (Some(why), _) => (Reason::Error, why.clone()),
        (None, Some(0)) if reached => (
            Reason::Reached,
            "The agent reached the stop and exited without recording it.".to_owned(),
        ),
        (None, Some(0)) if !now.waiting_on.is_empty() => (
            Reason::Waiting,
            "The agent exited while the change waits on the owner or a reviewer.".to_owned(),
        ),
        (None, _) if over => (
            Reason::Budget,
            "The agent spent more than its budget before the stop.".to_owned(),
        ),
        (None, Some(0)) => (
            Reason::Error,
            "The agent exited 0 before the change reached the stop.".to_owned(),
        ),
        (None, Some(code)) => (
            Reason::Error,
            format!("The agent exited {code} before recording a stop."),
        ),
        (None, None) => (
            Reason::Error,
            "The agent ended on a signal before recording a stop.".to_owned(),
        ),
    })
}

#[cfg(test)]
mod tests;
