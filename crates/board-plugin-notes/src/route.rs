//! The choice of a notes repo for a call (plugins/notes/README.md, Routing
//! a note). Discovery and remote matching are board-repos', shared with
//! tasks.

use board_model::shown_remote;
use board_ops::{Host, PluginError};
use board_repos::{RECORD, Repo, covering, listed, record, root};
use serde_json::json;

use crate::{PLUGIN, refusal};

/// The repo a note goes to, and the repos it might belong to when it goes
/// to the inbox; none otherwise.
#[derive(Debug)]
pub(crate) struct Choice {
    pub(crate) repo: Repo,
    pub(crate) candidates: Option<Vec<String>>,
}

/// The notes repo at `name`; else the one the remote routes to; else the
/// personal one's inbox for `add`, when `inbox` is set.
pub(crate) fn choose(
    host: &dyn Host,
    name: Option<&str>,
    remote: Option<&str>,
    inbox: bool,
) -> Result<Choice, PluginError> {
    let found = listed(host, PLUGIN)?;
    let roots: Vec<String> = found.iter().map(|listed| listed.root.clone()).collect();
    let at = |at: &str, candidates| {
        let listed = (found.iter()).find(|listed| listed.root == at);
        let repo = Repo {
            root: at.to_owned(),
            clone: listed.and_then(|listed| listed.clone.clone()),
        };
        Choice { repo, candidates }
    };
    let name = name.map(root);
    if roots.is_empty() || name.as_ref().is_some_and(|name| !roots.contains(name)) {
        let shown = if roots.is_empty() {
            "none".to_owned()
        } else {
            roots.join(", ")
        };
        let why = format!(
            "no notes repo is at {} (notes repos: {shown}); a notes repo is the board's root or a \
             mount whose tree has a tracked file under daily/, so mount the repo, or name one \
             listed with --repo",
            name.as_deref().unwrap_or("any path")
        );
        return Err(refusal(
            "notes::repo::not_found",
            why,
            [("repos", json!(roots))],
        ));
    }
    if let Some(name) = name {
        return Ok(at(&name, None));
    }
    let (matched, personal) = covering(PLUGIN, &found, remote)?;
    if let [one] = &matched[..] {
        return Ok(at(one, None));
    }
    // No remote may mean a missing origin, so a repo that covers something
    // waits for one.
    if roots.len() == 1
        && (!personal.is_empty() || (remote.is_none() && record(PLUGIN, &found[0])?.is_none()))
    {
        return Ok(at(&roots[0], None));
    }
    if !inbox && remote.is_none() && personal.len() == 1 {
        return Ok(at(&personal[0], None));
    }
    let shown = remote.map_or_else(|| "no git remote".to_owned(), shown_remote);
    let or_none = |list: &[String]| {
        if list.is_empty() {
            "none".to_owned()
        } else {
            list.join(", ")
        }
    };
    if !inbox {
        let why = format!(
            "{} notes repos cover {shown} ({}), so no daily note is read; name one of {} with \
             --repo",
            matched.len(),
            or_none(&matched),
            roots.join(", ")
        );
        return Err(refusal(
            "notes::repo::ambiguous",
            why,
            [("candidates", json!(matched)), ("repos", json!(roots))],
        ));
    }
    if personal.len() != 1 {
        let why = format!(
            "{} notes repos cover {shown}, so the note goes to the personal repo's inbox/, but {} \
             of {} say personal: true in {RECORD}; nothing was written; mark exactly one, add the \
             remote to one repo's covers, or name one with --repo",
            matched.len(),
            personal.len(),
            roots.join(", ")
        );
        return Err(refusal(
            "notes::repo::no_personal",
            why,
            [("repos", json!(roots)), ("personal", json!(personal))],
        ));
    }
    Ok(at(&personal[0], Some(matched)))
}
