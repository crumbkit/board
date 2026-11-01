//! Repo records: records of type `board/repo/v1` that name one repository
//! by its origin, and may mount a clone of it
//! (docs/architecture/paths/repos.md).

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;

use crate::location::ObjectId;
use crate::pointer::{Mount, PointerError, is_remote};

/// The `$type` of a repo record.
pub const REPO_TYPE: &str = "board/repo/v1";

/// A repo record: the repository's origin in its stored form, and the
/// clone it mounts, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub remote: String,
    pub checkout: Option<Mount>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepo {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    remote: String,
    checkout: Option<RawCheckout>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheckout {
    repo: String,
    follow: String,
    commit: String,
}

impl Repo {
    /// The repo record a parsed record of type [`REPO_TYPE`] holds.
    pub fn from_value(value: &Value) -> Result<Self, PointerError> {
        let raw = RawRepo::deserialize(value).map_err(|source| PointerError::Invalid { source })?;
        let shown = shown_remote(&raw.remote);
        if raw.remote.is_empty() || shown != raw.remote {
            return Err(PointerError::Unshown {
                remote: raw.remote,
                shown,
            });
        }
        let checkout = raw.checkout.map(mount).transpose()?;
        Ok(Self {
            remote: raw.remote,
            checkout,
        })
    }
}

/// The mount a checkout names, which means exactly what a mount pointer's
/// `repo`, `follow` and `commit` mean.
fn mount(checkout: RawCheckout) -> Result<Mount, PointerError> {
    let RawCheckout {
        repo,
        follow,
        commit,
    } = checkout;
    if is_remote(&repo) {
        return Err(PointerError::Remote { repo });
    }
    let Some(commit) = ObjectId::from_hex(&commit) else {
        return Err(PointerError::Commit { commit });
    };
    Ok(Mount {
        repo,
        follow,
        commit,
    })
}

/// `remote`, a git origin URL, as it may be shown and stored: `owner/name`
/// for GitHub, and otherwise the URL without its user and password. Two
/// clones of one repository give the same value. A port of
/// `board_plugin.repos.shown_remote`; both read the table in
/// python/board-plugin/tests/remotes.json in their tests.
pub fn shown_remote(remote: &str) -> String {
    match github(remote) {
        Some((owner, name)) => format!("{owner}/{name}"),
        None => without_userinfo(remote),
    }
}

/// The owner and name of a GitHub remote: `owner/name` itself, or a URL
/// over https, ssh or git, or git's scp-like `user@github.com:`. ssh, git
/// and scp-like remotes may name a host alias such as `github.com-work`.
fn github(remote: &str) -> Option<(&str, &str)> {
    let path = github_prefix(remote).unwrap_or(remote);
    let (owner, rest) = path.split_once('/')?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let name = match rest.strip_suffix(".git") {
        Some(name) if !name.is_empty() => name,
        _ => rest,
    };
    let in_owner = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'-';
    let in_name = |byte: u8| in_owner(byte) || matches!(byte, b'.' | b'_');
    let well_formed =
        |part: &str, allowed: &dyn Fn(u8) -> bool| !part.is_empty() && part.bytes().all(allowed);
    (well_formed(owner, &in_owner) && well_formed(name, &in_name)).then_some((owner, name))
}

/// What follows the host in a GitHub URL or scp-like remote, or `None`
/// when `remote` starts with neither.
fn github_prefix(remote: &str) -> Option<&str> {
    for scheme in ["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = remote.strip_prefix(scheme) {
            let aliased = matches!(scheme, "ssh://" | "git://");
            let user = rest
                .split_once('@')
                .filter(|(user, _)| !user.is_empty() && !user.contains('/'));
            let at = user.map_or(rest, |(_, host)| host);
            return host(at, aliased, '/').or_else(|| host(rest, aliased, '/'));
        }
    }
    let (user, at) = remote.split_once('@')?;
    if user.is_empty() || user.contains(['/', ':']) {
        return None;
    }
    host(at, true, ':')
}

/// What follows `github.com`, an alias such as `-work` when `aliased`,
/// and `end` at the start of `text`.
fn host(text: &str, aliased: bool, end: char) -> Option<&str> {
    let rest = text.strip_prefix("github.com")?;
    let rest = match rest.strip_prefix('-') {
        Some(alias) if aliased => {
            let alias_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '-');
            let length = alias.find(|c: char| !alias_char(c)).unwrap_or(alias.len());
            (length > 0).then(|| &alias[length..])?
        }
        _ => rest,
    };
    rest.strip_prefix(end)
}

/// `remote` without the user and password of a URL, or the user of an
/// scp-like remote, either of which may hold a token.
fn without_userinfo(remote: &str) -> String {
    let scheme = remote.find("://").filter(|&at| {
        let mut chars = remote[..at].chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
    });
    let start = scheme.map_or(0, |at| at + 3);
    let rest = &remote[start..];
    let authority = &rest[..rest.find('/').unwrap_or(rest.len())];
    match authority.rfind('@') {
        Some(at) => format!("{}{}", &remote[..start], &rest[at + 1..]),
        None => remote.to_owned(),
    }
}

#[cfg(test)]
mod tests;
