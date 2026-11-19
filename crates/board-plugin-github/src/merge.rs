//! `merge`: squash-merges a pull request with gh, only once the change it
//! lands is in its land phase with every blocking check passing and the
//! boundary passes the commit's subject and body. The change then moves to
//! its next phase.

use std::path::PathBuf;

use board_model::LogicalPath;
use board_ops::{Elsewhere, Host, PluginError};
use board_plugin::Target;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::gh;
use crate::parts::Parts;
use crate::send::{Delivered, after_sent, origin, output};

/// The phase a change must be in before its pull request merges.
const LAND: &str = "land";
/// The folder a board's own changes sit in.
const CHANGES: &str = "/changes/";
/// What to run again after a merge, so it opens the board the merge ran in.
const SAME_BOARD: &str = "in the board this merge ran in, with the same `-C` or `--personal`";

/// Squash-merge a pull request in the checkout's repository, once the
/// change it lands passes its land gate and the boundary passes the
/// commit's subject and body.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MergeInput")]
pub(crate) struct MergeInput {
    /// The pull request's number in the checkout's repository. Optional
    /// with `change` when the change records exactly one pull request of
    /// this repository, whose number is then used.
    #[schemars(range(min = 1))]
    number: Option<u64>,
    /// The squash commit's subject. Optional: the pull request's title
    /// followed by ` (#<number>)`.
    #[schemars(length(min = 1))]
    subject: Option<String>,
    /// The squash commit's body. Optional: the pull request's body.
    body: Option<String>,
    /// The change in the board this command runs in that the pull request
    /// lands, such as /changes/lever. Its land gate must pass, its head is
    /// the commit merged, and it moves to its next phase after the merge.
    #[schemars(regex(pattern = "^/"))]
    change: Option<String>,
    /// The full 40-hex commit the pull request's head must be at. With
    /// `change` it must equal the change's head; without it, it is
    /// required.
    #[schemars(regex(pattern = "^[0-9a-f]{40}$"))]
    head: Option<String>,
}

/// The crossing the boundary passed, the pull request's URL in `sent`, the
/// head commit merged, whether GitHub only queued the merge, and the phase
/// the change moved to, left out when no change was named or the merge was
/// queued.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MergeOutput")]
pub(crate) struct MergeOutput {
    digest: String,
    crossing: String,
    note: Option<String>,
    sent: String,
    head: String,
    queued: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase: Option<String>,
}

/// What this capability reads of `changes.next`'s output. `stop` is there
/// only when a run of the change set where it stops.
#[derive(Deserialize)]
struct Next {
    phase: String,
    gate: Vec<Gated>,
    stop: Option<Value>,
}

/// One check of a change's gate.
#[derive(Deserialize)]
struct Gated {
    check: String,
    status: String,
    blocks: bool,
    remedy: Option<String>,
}

/// Squash-merges a pull request at its head commit, once the change's land
/// gate and the boundary pass, then advances the change.
pub(crate) fn merge(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let MergeInput {
        number,
        subject,
        body,
        change,
        head,
    } = PluginError::read_input("github", "merge", input)?;
    if let Some(change) = change.as_deref()
        && !change.starts_with(CHANGES)
    {
        return Err(not_a_change(change));
    }
    let place = origin(host)?;
    let changed = match change.as_deref() {
        Some(path) => Some((path, own(host)?)),
        None => None,
    };
    let mut pulls = Vec::new();
    let head = match &changed {
        Some((change, board)) => {
            gate(&**board, &*place.personal, change)?;
            let (recorded, found) = recorded_change(&**board, change)?;
            pulls = found;
            change_head(change, recorded, head)?
        }
        None => head.ok_or_else(|| {
            head_refused(
                "merge needs the commit to merge; pass --head <sha> or --change /changes/<name>"
                    .to_owned(),
            )
        })?,
    };
    let repository = format!("{}/{}", place.owner, place.repo);
    let number = match number {
        Some(number) => number,
        None => only_pull(&pulls, &repository, changed.is_some())?,
    };
    let (subject, body) = commit_text(host, &repository, number, subject, body)?;
    let parts = Parts::from([
        ("pr.merge.subject".to_owned(), subject.clone()),
        ("pr.merge.body".to_owned(), body.clone()),
    ]);
    let passed = place.check("merge", &parts)?;
    let number_text = number.to_string();
    let arguments: Vec<String> = [
        "pr",
        "merge",
        &number_text,
        "--repo",
        &repository,
        "--squash",
        "--match-head-commit",
        &head,
        "--subject",
        &subject,
        "--body-file",
        "-",
    ]
    .map(str::to_owned)
    .into();
    // In the checkout, so gh reads no other repository's state.
    gh::send(host, &arguments, &body, "gh pr merge", Some(&place.top))?;
    let url = format!("https://github.com/{repository}/pull/{number}");
    let Delivered {
        digest,
        crossing,
        note,
        sent,
    } = place.sent(passed, url)?;
    let queued = !merged(host, &repository, number, &sent)?;
    let phase = match &changed {
        Some((change, board)) if !queued => Some(advance(&**board, change, &sent)?),
        _ => None,
    };
    Ok(output(&MergeOutput {
        digest,
        crossing,
        note,
        sent,
        head,
        queued,
        phase,
    }))
}

/// The head `change` records, which `asked` must equal when given, or
/// `github::merge::head`.
fn change_head(
    change: &str,
    recorded: Option<String>,
    asked: Option<String>,
) -> Result<String, PluginError> {
    match (recorded, asked) {
        (None, _) => Err(head_refused(format!(
            "the change at {change} records no head commit; record one with `board changes \
             attach --path {change} --head <sha>`"
        ))),
        (Some(recorded), _) if !is_full_commit(&recorded) => Err(head_refused(format!(
            "the change at {change} records the head {recorded}, which is not a full 40-hex \
             commit; attach the full commit with `board changes attach --path {change} --head \
             <sha>`"
        ))),
        (Some(recorded), Some(asked)) if asked != recorded => Err(head_refused(format!(
            "--head {asked} is not {recorded}, the head the change at {change} records; leave \
             --head out or attach the head you mean"
        ))),
        (Some(recorded), _) => Ok(recorded),
    }
}

/// The squash commit's subject and body: each one given, else the pull
/// request's title and number, or its body.
fn commit_text(
    host: &dyn Host,
    repository: &str,
    number: u64,
    subject: Option<String>,
    body: Option<String>,
) -> Result<(String, String), PluginError> {
    if let (Some(subject), Some(body)) = (&subject, &body) {
        return Ok((subject.clone(), body.clone()));
    }
    let endpoint = format!("repos/{repository}/pulls/{number}");
    let pull = gh::api(host, &endpoint)?.ok_or_else(|| {
        gh::failed(
            &endpoint,
            "answered 404",
            "check the pull request on GitHub",
        )
    })?;
    let text = |key| pull.get(key).and_then(Value::as_str).unwrap_or_default();
    Ok((
        subject.unwrap_or_else(|| format!("{} (#{number})", text("title"))),
        body.unwrap_or_else(|| text("body").to_owned()),
    ))
}

/// Whether `commit` is a full 40-hex commit name.
fn is_full_commit(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether GitHub says pull request `number` of `repository`, at `url`,
/// merged. gh exits 0 when a merge queue only queued it, so its exit code
/// alone does not say.
fn merged(host: &dyn Host, repository: &str, number: u64, url: &str) -> Result<bool, PluginError> {
    let lead = format!(
        "the merge of pull request {url} was already sent, so do not merge it again; look on \
         GitHub whether it merged, then run `board changes advance` for its change {SAME_BOARD}"
    );
    let endpoint = format!("repos/{repository}/pulls/{number}");
    let answer = gh::api(host, &endpoint).map_err(|error| after_sent(&lead, url, error))?;
    let Some(pull) = answer else {
        let error = gh::failed(
            &endpoint,
            "answered 404",
            "check the pull request on GitHub",
        );
        return Err(after_sent(&lead, url, error));
    };
    Ok(pull.get("merged").and_then(Value::as_bool) == Some(true))
}

/// The board the command runs in, opened again as `board changes next`
/// and `board changes advance` there open it: it reads each mount at its
/// pin and may write, even when the command reads a mount at a checkout.
fn own(host: &dyn Host) -> Result<Box<dyn Host + '_>, PluginError> {
    (host.board(&Elsewhere::Directory(PathBuf::from("."))))
        .map_err(|refusal| PluginError::board_refused("github", "-C .", &refusal))
}

/// Whether `personal` holds a change at `path`.
fn holds_change(personal: &dyn Host, path: &str) -> bool {
    let Ok(logical) = path.parse::<LogicalPath>() else {
        return false;
    };
    let record = (personal.get_each(&[logical]).ok())
        .and_then(|answers| answers.into_iter().next()?.ok())
        .and_then(|report| report.record.ok());
    (record.as_ref())
        .and_then(|record| record.get("$type")?.as_str())
        .is_some_and(|kind| kind.starts_with("changes/change/"))
}

/// Runs `changes.<capability>` in `host`'s board with `path`.
fn changes(
    host: &dyn Host,
    capability: &str,
    path: &str,
) -> Result<Value, Box<board_model::Refusal>> {
    let target: Target = format!("changes.{capability}")
        .parse()
        .expect("changes.<capability> is <plugin>.<capability>");
    (host.call(&target, &json!({"path": path}))).map(|report| report.output)
}

/// Refuses with `github::merge::gate` unless the change at `path` is in
/// its land phase and no blocking check of its gate is missing. When
/// `host` holds no change there, the hint names `--personal` only when
/// `personal` holds one.
fn gate(host: &dyn Host, personal: &dyn Host, path: &str) -> Result<(), PluginError> {
    let output = changes(host, "next", path).map_err(|refusal| {
        let mut error = PluginError::board_refused("github", "call", &refusal);
        if refusal.code.as_str() == "changes::change::not_found" {
            let personal = if holds_change(personal, path) {
                "the personal board holds this change, so add `--personal`; otherwise, "
            } else {
                ""
            };
            error.hint = Some(format!(
                "merge reads the change in the board the command runs in; {personal}check the \
                 path with `board changes open`, or run in the board that holds the change with \
                 `-C <board>`"
            ));
        }
        error
    })?;
    let next = Next::deserialize(&output).map_err(|error| PluginError {
        code: "github::merge::unreadable".to_owned(),
        message: format!("changes.next gave output this plugin cannot read: {error}"),
        fields: Map::new(),
        hint: Some("update Board so its github and changes plugins agree".to_owned()),
    })?;
    let blocking: Vec<Gated> = (next.gate.into_iter())
        .filter(|check| check.status == "missing" && check.blocks)
        .collect();
    let missing: Vec<&str> = blocking.iter().map(|check| check.check.as_str()).collect();
    if next.phase == LAND && missing.is_empty() && next.stop.is_none() {
        return Ok(());
    }
    let why = if let Some(stop) = &next.stop {
        format!("a run of it is set to stop at {stop}; end the run with `board changes stop` first")
    } else if next.phase == LAND {
        format!("its land gate misses {}", missing.join(", "))
    } else {
        format!("it is in its {} phase, not {LAND}", next.phase)
    };
    let mut fields = Map::new();
    fields.insert("path".to_owned(), json!(path));
    fields.insert("phase".to_owned(), json!(next.phase));
    fields.insert("missing".to_owned(), json!(missing));
    if let Some(stop) = next.stop {
        fields.insert("stop".to_owned(), stop);
    }
    let remedies: Vec<String> = (blocking.iter())
        .filter_map(|check| Some(format!("{}: {}", check.check, check.remedy.as_ref()?)))
        .collect();
    Err(PluginError {
        code: "github::merge::gate".to_owned(),
        message: format!(
            "the change at {path} cannot merge yet: {why}; run `board changes next --path {path}` \
             for the next step"
        ),
        fields,
        hint: (!remedies.is_empty()).then(|| remedies.join("; ")),
    })
}

/// The pull request number the change records for `repository`, or
/// `github::merge::number` saying why there is none: no change was named
/// (`named` false), it records no pull request of `repository`, or several.
/// Owner and repository match without regard to case, as GitHub names them.
fn only_pull(pulls: &[String], repository: &str, named: bool) -> Result<u64, PluginError> {
    let prefix = format!("/gh/{}/pulls/", repository.to_lowercase());
    let mut numbers: Vec<u64> = (pulls.iter())
        .filter_map(|pull| pull.to_lowercase().strip_prefix(&prefix)?.parse().ok())
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    if let [number] = numbers[..] {
        return Ok(number);
    }
    let message = if !named {
        "merge needs the pull request to merge; pass --change /changes/<name> or --number <n>"
            .to_owned()
    } else if numbers.is_empty() {
        format!("the change records no pull request of {repository}; pass --number <n>")
    } else {
        let list: Vec<String> = numbers.iter().map(|number| format!("#{number}")).collect();
        format!(
            "the change records several pull requests of {repository}, {}; pass --number <n> \
             for the one to merge",
            list.join(", ")
        )
    };
    let mut fields = Map::new();
    fields.insert("pulls".to_owned(), json!(numbers));
    Err(PluginError {
        code: "github::merge::number".to_owned(),
        message,
        fields,
        hint: Some("pass --number <n>, the pull request's number".to_owned()),
    })
}

/// The head commit the change at `path` records, if any, and the pull
/// requests it records.
fn recorded_change(
    host: &dyn Host,
    path: &str,
) -> Result<(Option<String>, Vec<String>), PluginError> {
    let refused =
        |refusal: &board_model::Refusal| PluginError::board_refused("github", "get", refusal);
    let logical: LogicalPath = path.parse().map_err(|_| {
        head_refused(format!(
            "{path} is not a logical path; name the change as /changes/<name>"
        ))
    })?;
    let mut answers = (host.get_each(&[logical]))
        .map_err(|error| refused(&error.refusal("get")))?
        .into_iter();
    let report = answers
        .next()
        .expect("get_each answers each path asked")
        .map_err(|error| refused(&error.refusal("get")))?;
    let record = report.record.map_err(|refusal| refused(&refusal))?;
    let head = record.get("head").and_then(Value::as_str);
    let pulls = (record.get("pulls").and_then(Value::as_array).into_iter())
        .flatten()
        .filter_map(|pull| pull.as_str().map(str::to_owned));
    Ok((head.map(str::to_owned), pulls.collect()))
}

/// Moves the change at `path` to its next phase after its pull request at
/// `url` merged, and returns that phase.
fn advance(host: &dyn Host, path: &str, url: &str) -> Result<String, PluginError> {
    let lead = format!(
        "pull request {url} was already merged, so do not merge it again; run `board changes \
         advance --path {path}` {SAME_BOARD}"
    );
    let moved = changes(host, "advance", path).map_err(|refusal| {
        let mut error = PluginError::board_refused("github", "call", &refusal);
        "github::merge::advance".clone_into(&mut error.code);
        after_sent(&lead, url, error)
    })?;
    Ok((moved.get("to").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned())
}

/// `github::merge::change`: `path` is not under /changes/, so it names no
/// change of the board the command runs in.
fn not_a_change(path: &str) -> PluginError {
    let mut fields = Map::new();
    fields.insert("path".to_owned(), json!(path));
    PluginError {
        code: "github::merge::change".to_owned(),
        message: format!(
            "{path} is not a change of this board; a board's own changes are at \
             /changes/<name>, and a change is merged in the board that holds it"
        ),
        fields,
        hint: Some(
            "run `board -C <that board's directory> github merge ... --change /changes/<name>`"
                .to_owned(),
        ),
    }
}

/// `github::merge::head`, saying `message`.
fn head_refused(message: String) -> PluginError {
    PluginError {
        code: "github::merge::head".to_owned(),
        message,
        fields: Map::new(),
        hint: None,
    }
}
