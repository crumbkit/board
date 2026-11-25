//! What `add` says about where it wrote a task: the remote its repo
//! pushes to, whether the call pushed it off this Mac, and one sentence
//! that says both; and the commands its routing refusals give.

use std::path::Path;

use board_model::shown_remote;
use board_ops::{Host, Origin, PluginError};
use board_repos::{Listed, Repo, covering};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;

use super::Written;
use crate::{PLUGIN, hinted};

/// What a hint's command gives `--reason`, for the reader to fill in.
const REASON: &str = "'<why it belongs with this board>'";

/// The task as `add` left it, with where it went. `about` says in one
/// sentence which repo holds the task and whether it left this Mac.
/// `remote` is the remote, owner/name for GitHub, that the task's repo
/// pushes to, null for the board's own tree. `leaves_this_mac` is true
/// when this call pushed the task to that remote; it cannot see a later
/// push by anything else.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Added {
    pub(crate) about: String,
    #[serde(flatten)]
    pub(crate) written: Written,
    pub(crate) remote: Option<String>,
    pub(crate) leaves_this_mac: bool,
}

/// How `add` came to its repo: named with `--repo`, or routed by the
/// remote it was given, if any.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Route<'a> {
    Named,
    Routed(Option<&'a str>),
}

/// `written` with where it went, `reason` printed in its sentence.
pub(crate) fn added(
    host: &dyn Host,
    repo: &Repo,
    written: Written,
    route: Route<'_>,
    reason: Option<&str>,
) -> Added {
    let remote = repo
        .clone
        .as_deref()
        .and_then(|clone| pushes_to(host, clone));
    let unpushed = written.unpushed.as_deref();
    let about = said(
        &written.path,
        &repo.root,
        (remote.as_deref(), unpushed),
        route,
        reason,
    );
    Added {
        about,
        leaves_this_mac: repo.clone.is_some() && unpushed.is_none(),
        written,
        remote,
    }
}

/// The remote the clone at `clone` pushes to, as it may be shown.
fn pushes_to(host: &dyn Host, clone: &str) -> Option<String> {
    match host.clone_origin(Path::new(clone)) {
        Origin::Url(url) => Some(shown_remote(&url)),
        Origin::Outside | Origin::Unread(_) => None,
    }
}

/// The sentence that says where the task at `path` went: to `repo`,
/// pushed to `remote` unless `unpushed` gives the push's code, or to the
/// board's own tree, which `add` never pushes.
pub(crate) fn said(
    path: &str,
    repo: &str,
    (remote, unpushed): (Option<&str>, Option<&str>),
    route: Route<'_>,
    reason: Option<&str>,
) -> String {
    let mut said = match (repo, unpushed) {
        ("/", _) => {
            let why = match route {
                Route::Named => String::new(),
                Route::Routed(remote) => {
                    let covered = remote.map_or_else(
                        || "the call gave no git remote".to_owned(),
                        |remote| format!("no notes repo covers {remote}"),
                    );
                    format!(", because {covered} and the board lists no personal notes repo")
                }
            };
            format!(
                "Wrote {path} to this board's own tree{why}. tasks add does not push it, so it \
                 stays on this Mac unless something else pushes this board."
            )
        }
        (_, None) => format!(
            "Wrote {path} to {repo} and pushed it to {}.",
            remote.unwrap_or("its remote")
        ),
        (_, Some(code)) => format!(
            "Wrote {path} to {repo}, but the push failed with {code}, so the task is only on this \
             Mac until the next sync."
        ),
    };
    if let Some(reason) = reason {
        let reason = reason.trim().trim_end_matches('.');
        said.extend([" The reason given: ", reason, "."]);
    }
    said
}

/// The command that adds the task titled `title` to `repo`, with a
/// reason to fill in when `reason`.
pub(crate) fn command(title: &str, repo: &str, reason: bool) -> String {
    let mut command = format!("board tasks add --title {} --repo {repo}", quote(title));
    if reason {
        command.extend([" --reason ", REASON]);
    }
    command
}

/// `text` quoted for a POSIX shell, as Python's `shlex.quote` quotes it.
fn quote(text: &str) -> String {
    let safe =
        |character: char| character.is_ascii_alphanumeric() || "@%+=:,./-_".contains(character);
    if !text.is_empty() && text.chars().all(safe) {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

/// The board's one personal notes repo, with the remote it pushes to.
fn personal(host: &dyn Host, found: &[Listed]) -> Option<(String, Option<String>)> {
    let (_, personal) = covering(PLUGIN, found, None).ok()?;
    let [root] = personal.as_slice() else {
        return None;
    };
    let clone = (found.iter())
        .find(|listed| &listed.root == root)?
        .clone
        .as_deref();
    Some((root.clone(), clone.and_then(|clone| pushes_to(host, clone))))
}

/// `, which pushes to <remote>`, when the remote is known.
fn pushing(remote: Option<&str>) -> String {
    remote.map_or_else(String::new, |remote| format!(", which pushes to {remote}"))
}

/// The ways to file the task titled `title` other than by routing: the
/// personal notes repo first, when the board has one, then `/` with a
/// reason.
fn ways(personal: Option<&(String, Option<String>)>, title: &str) -> Vec<String> {
    let mut ways = Vec::new();
    if let Some((root, remote)) = personal {
        ways.push(format!(
            "`{}` to file it in the personal notes repo{}",
            command(title, root, false),
            pushing(remote.as_deref())
        ));
    }
    ways.push(format!(
        "`{}` to keep it in this board's own tree, where it stays on this Mac",
        command(title, "/", true)
    ));
    ways
}

/// The hint of `tasks::repo::origin_unread` for the task titled `title`.
pub(crate) fn unread_hint(host: &dyn Host, found: &[Listed], title: &str) -> String {
    let mut ways = ways(personal(host, found).as_ref(), title);
    ways.push("`--remote <owner/name>` for the repository it belongs to".to_owned());
    format!("run it again as {}", ways.join(", or "))
}

/// The hint of `tasks::repo::ambiguous`: a command for each candidate.
pub(crate) fn ambiguous_hint(candidates: &[String], title: &str) -> String {
    let commands: Vec<String> = (candidates.iter())
        .map(|candidate| format!("`{}`", command(title, candidate, false)))
        .collect();
    format!(
        "run one of {}, with the call's other flags",
        commands.join(" or ")
    )
}

/// The refusal of `--repo /` given without a reason, naming the
/// personal notes repo and its remote when the board has one.
pub(crate) fn reason_needed(host: &dyn Host, found: &[Listed], title: &str) -> PluginError {
    let personal = personal(host, found);
    let elsewhere = personal
        .as_ref()
        .map_or_else(String::new, |(root, remote)| {
            format!(
                ", or file it in the personal notes repo {root}{}",
                pushing(remote.as_deref())
            )
        });
    let why = format!(
        "--repo / writes the task to this board's own tree, which tasks add never pushes, so it \
         stays on this Mac; give --reason to say why it belongs with this board{elsewhere}"
    );
    let hint = format!("run {}", ways(personal.as_ref(), title).join(", or "));
    let (root, remote) = personal.unzip();
    hinted(
        "tasks::repo::reason_needed",
        why,
        [
            ("personal", json!(root)),
            ("remote", json!(remote.flatten())),
        ],
        &hint,
    )
}
