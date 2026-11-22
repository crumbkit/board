//! `tag`: a new annotated tag on GitHub at a commit on the default branch,
//! made through GitHub's git data API once the boundary passes it. It never
//! moves or deletes a tag, and runs no git command that reads history.

use std::path::Path;

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::gh;
use crate::git::{error, run};
use crate::parts::Parts;
use crate::send::{Delivered, origin, output};

/// The compare statuses that put the target on the default branch.
const MERGED: [&str; 2] = ["behind", "identical"];
/// What gh prints when GitHub refuses a ref because it exists.
const REF_EXISTS: &str = "Reference already exists";

/// Create a new annotated tag on GitHub, once the boundary passes it.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TagInput")]
pub(crate) struct TagInput {
    /// The tag's name, such as v0.1.0: up to 100 letters, digits, `.`, `_`,
    /// `+` and `-`, starting with a letter or digit, with no `..` and not
    /// ending in `.` or `.lock`, which git refuses.
    #[schemars(regex(pattern = r"^[0-9A-Za-z][0-9A-Za-z._+-]{0,99}$"))]
    #[schemars(extend("not" = {"anyOf": [{"pattern": r"\.\."}, {"pattern": r"\.(lock)?$"}]}))]
    name: String,
    /// The commit to tag, as its full 40-hex SHA. It must be on the
    /// repository's default branch on GitHub.
    #[schemars(regex(pattern = "^[0-9a-f]{40}$"))]
    target: String,
    /// The tag's message.
    #[schemars(length(min = 1))]
    message: String,
    /// Ask the owner to let this exact content through when a check fails,
    /// which files one request the owner answers; left out, an earlier answer
    /// still applies but no request is filed.
    ask_owner: Option<bool>,
}

/// The tag made: `sent` is its page on GitHub, `object` the tag object's
/// SHA.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TagOutput")]
pub(crate) struct Tagged {
    digest: String,
    crossing: String,
    note: Option<String>,
    sent: String,
    name: String,
    target: String,
    object: String,
}

/// Creates the tag, once the ref is free, the target is on the default
/// branch and the boundary passes the tag's name, message, tagger and
/// target.
pub(crate) fn tag(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let TagInput {
        name,
        target,
        message,
        ask_owner,
    } = PluginError::read_input("github", "tag", input)?;
    let place = origin(host)?;
    let (tagger_name, tagger_email) = tagger(host, &place.top)?;
    let repository = format!("repos/{}", place.repository());
    let fields = json!({"name": name, "target": target});
    if gh::api(host, &format!("{repository}/git/ref/tags/{name}"))?.is_some() {
        return Err(exists(&name, fields));
    }
    let about = gh::api(host, &repository)?.unwrap_or_default();
    let Some(branch) = about.get("default_branch").and_then(Value::as_str) else {
        return Err(gh::failed(
            &repository,
            "named no default branch",
            "check that the repository exists and gh can read it",
        ));
    };
    let Some(compared) = gh::api(host, &format!("{repository}/compare/{branch}...{target}"))?
    else {
        return Err(error(
            "github::tag::target_not_found",
            format!(
                "GitHub has no commit {target} in {}, so nothing was tagged; push and merge it \
                 first, or give the SHA of a commit GitHub has",
                place.repository()
            ),
            fields,
        ));
    };
    let status = compared.get("status").and_then(Value::as_str).unwrap_or("");
    if !MERGED.contains(&status) {
        return Err(error(
            "github::tag::target_not_merged",
            format!(
                "{target} is not on {branch}, the default branch on GitHub (it is {status} of \
                 it), so nothing was tagged; merge it first, then tag the merged commit"
            ),
            json!({"name": name, "target": target, "branch": branch, "status": status}),
        ));
    }
    let parts = Parts::from([
        ("tag.name".to_owned(), name.clone()),
        ("tag.message".to_owned(), message.clone()),
        ("tag.tagger_name".to_owned(), tagger_name.clone()),
        ("tag.tagger_email".to_owned(), tagger_email.clone()),
        ("tag.target".to_owned(), target.clone()),
    ]);
    let passed = place.check_asking("tag", &parts, ask_owner == Some(true))?;
    let body = json!({
        "tag": name,
        "message": message,
        "object": target,
        "type": "commit",
        "tagger": {"name": tagger_name, "email": tagger_email},
    });
    let made = gh::post(host, &format!("{repository}/git/tags"), &body, None)?;
    let object = (made.as_ref())
        .and_then(|made| made.get("sha"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(object) = object else {
        return Err(gh::unsent(
            "gh api --method POST git/tags",
            "answered without the tag object's sha",
        ));
    };
    let reference = json!({"ref": format!("refs/tags/{name}"), "sha": object});
    // GitHub refuses a ref that exists, so a tag made meanwhile is never
    // moved. Its other refusals carry GitHub's message.
    let refs = format!("{repository}/git/refs");
    if gh::post(host, &refs, &reference, Some(REF_EXISTS))?.is_none() {
        return Err(exists(&name, fields));
    }
    let url = format!(
        "https://github.com/{}/releases/tag/{name}",
        place.repository()
    );
    let Delivered {
        digest,
        crossing,
        note,
        sent,
    } = place.sent(passed, url)?;
    let fetch = format!("The checkout gets the tag with `git fetch origin tag {name}`.");
    let note = Some(note.map_or(fetch.clone(), |note| format!("{note} {fetch}")));
    Ok(output(&Tagged {
        digest,
        crossing,
        note,
        sent,
        name,
        target,
        object,
    }))
}

/// `github::tag::exists` for the tag `name`.
fn exists(name: &str, fields: Value) -> PluginError {
    error(
        "github::tag::exists",
        format!(
            "GitHub already has a tag {name}, and Board never moves a tag, so nothing was \
             tagged; release the next version under a new name"
        ),
        fields,
    )
}

/// The checkout's `user.name` and `user.email`, which name the tagger, or
/// `github::tag::no_tagger` naming the one git config lacks.
fn tagger(host: &dyn Host, top: &Path) -> Result<(String, String), PluginError> {
    let read = |key: &str| {
        let ran = run(host, top, &["config", "--get", key], "");
        Some(ran.stdout.trim().to_owned()).filter(|value| ran.code == 0 && !value.is_empty())
    };
    match (read("user.name"), read("user.email")) {
        (Some(name), Some(email)) => Ok((name, email)),
        (name, _) => {
            let key = if name.is_none() {
                "user.name"
            } else {
                "user.email"
            };
            Err(error(
                "github::tag::no_tagger",
                format!(
                    "git config has no {key}, which names the tag's tagger, so nothing was \
                     tagged; set it with `git config {key} <value>` and run the command again"
                ),
                json!({"key": key}),
            ))
        }
    }
}
