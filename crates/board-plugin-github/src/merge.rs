//! `merge`: squash-merges a pull request with gh, only once the change it
//! lands is in its land phase with every blocking check passing and the
//! boundary passes the commit's subject and body. The change then moves to
//! its next phase.

use board_model::LogicalPath;
use board_ops::{Host, PluginError};
use board_plugin::Target;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::gh;
use crate::parts::Parts;
use crate::send::{Delivered, after_sent, origin, output};

/// The phase a change must be in before its pull request merges.
const LAND: &str = "land";

/// Squash-merge a pull request in the checkout's repository, once the
/// change it lands passes its land gate and the boundary passes the
/// commit's subject and body.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MergeInput")]
pub(crate) struct MergeInput {
    /// The pull request's number in the checkout's repository.
    #[schemars(range(min = 1))]
    number: u64,
    /// The squash commit's subject.
    #[schemars(length(min = 1))]
    subject: String,
    /// The squash commit's body.
    body: String,
    /// The change in the personal board that the pull request lands, such
    /// as /changes/lever. Its land gate must pass, its head is the commit
    /// merged, and it moves to its next phase after the merge.
    #[schemars(regex(pattern = "^/"))]
    change: Option<String>,
    /// The commit the pull request's head must be at. With `change` it
    /// must equal the change's head; without it, it is required.
    #[schemars(length(min = 1))]
    head: Option<String>,
}

/// The crossing the boundary passed, the pull request's URL in `sent`, the
/// head commit merged, and the phase the change moved to, left out when no
/// change was named.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MergeOutput")]
pub(crate) struct MergeOutput {
    digest: String,
    crossing: String,
    note: Option<String>,
    sent: String,
    head: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase: Option<String>,
}

/// What this capability reads of `changes.next`'s output.
#[derive(Deserialize)]
struct Next {
    phase: String,
    gate: Vec<Gated>,
}

/// One check of a change's gate.
#[derive(Deserialize)]
struct Gated {
    check: String,
    status: String,
    blocks: bool,
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
    let place = origin(host)?;
    let head = match change.as_deref() {
        Some(change) => {
            gate(&*place.personal, change)?;
            let recorded = recorded_head(&*place.personal, change)?;
            match (recorded, head) {
                (None, _) => {
                    return Err(head_refused(format!(
                        "the change at {change} records no head commit; record one with `board \
                         changes attach --path {change} --head <sha>`"
                    )));
                }
                (Some(recorded), Some(asked)) if asked != recorded => {
                    return Err(head_refused(format!(
                        "--head {asked} is not {recorded}, the head the change at {change} \
                         records; leave --head out or attach the head you mean"
                    )));
                }
                (Some(recorded), _) => recorded,
            }
        }
        None => head.ok_or_else(|| {
            head_refused(
                "merge needs the commit to merge; pass --head <sha> or --change /changes/<name>"
                    .to_owned(),
            )
        })?,
    };
    let parts = Parts::from([
        ("pr.merge.subject".to_owned(), subject.clone()),
        ("pr.merge.body".to_owned(), body.clone()),
    ]);
    let passed = place.check("merge", &parts)?;
    let repository = format!("{}/{}", place.owner, place.repo);
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
    let phase = match change {
        Some(change) => Some(advance(&*place.personal, &change, &sent)?),
        None => None,
    };
    Ok(output(&MergeOutput {
        digest,
        crossing,
        note,
        sent,
        head,
        phase,
    }))
}

/// Runs `changes.<capability>` in the personal board with `path`.
fn changes(
    personal: &dyn Host,
    capability: &str,
    path: &str,
) -> Result<Value, Box<board_model::Refusal>> {
    let target: Target = format!("changes.{capability}")
        .parse()
        .expect("changes.<capability> is <plugin>.<capability>");
    (personal.call(&target, &json!({"path": path}))).map(|report| report.output)
}

/// Refuses with `github::merge::gate` unless the change at `path` is in
/// its land phase and no blocking check of its gate is missing.
fn gate(personal: &dyn Host, path: &str) -> Result<(), PluginError> {
    let output = changes(personal, "next", path)
        .map_err(|refusal| PluginError::board_refused("github", "--personal call", &refusal))?;
    let next = Next::deserialize(&output).map_err(|error| PluginError {
        code: "github::merge::unreadable".to_owned(),
        message: format!("changes.next gave output this plugin cannot read: {error}"),
        fields: Map::new(),
        hint: Some("update Board so its github and changes plugins agree".to_owned()),
    })?;
    let missing: Vec<String> = (next.gate.into_iter())
        .filter(|check| check.status == "missing" && check.blocks)
        .map(|check| check.check)
        .collect();
    if next.phase == LAND && missing.is_empty() {
        return Ok(());
    }
    let why = if next.phase == LAND {
        format!("its land gate misses {}", missing.join(", "))
    } else {
        format!("it is in its {} phase, not {LAND}", next.phase)
    };
    let mut fields = Map::new();
    fields.insert("path".to_owned(), json!(path));
    fields.insert("phase".to_owned(), json!(next.phase));
    fields.insert("missing".to_owned(), json!(missing));
    Err(PluginError {
        code: "github::merge::gate".to_owned(),
        message: format!(
            "the change at {path} cannot merge yet: {why}; run `board changes next --path {path}` \
             for the next step"
        ),
        fields,
        hint: None,
    })
}

/// The head commit the change at `path` records, if any.
fn recorded_head(personal: &dyn Host, path: &str) -> Result<Option<String>, PluginError> {
    let refused = |refusal: &board_model::Refusal| {
        PluginError::board_refused("github", "--personal get", refusal)
    };
    let logical: LogicalPath = path.parse().map_err(|_| {
        head_refused(format!(
            "{path} is not a logical path; name the change as /changes/<name>"
        ))
    })?;
    let mut answers = (personal.get_each(&[logical]))
        .map_err(|error| refused(&error.refusal("get")))?
        .into_iter();
    let report = answers
        .next()
        .expect("get_each answers each path asked")
        .map_err(|error| refused(&error.refusal("get")))?;
    let record = report.record.map_err(|refusal| refused(&refusal))?;
    Ok(record
        .get("head")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// Moves the change at `path` to its next phase after its pull request at
/// `url` merged, and returns that phase.
fn advance(personal: &dyn Host, path: &str, url: &str) -> Result<String, PluginError> {
    let lead = format!(
        "pull request {url} was already merged, so do not merge it again; run `board changes \
         advance --path {path}`"
    );
    let moved = changes(personal, "advance", path).map_err(|refusal| {
        let mut error = PluginError::board_refused("github", "--personal call", &refusal);
        "github::merge::advance".clone_into(&mut error.code);
        after_sent(&lead, url, error)
    })?;
    Ok((moved.get("to").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned())
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
