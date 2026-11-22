//! Repository records: records of type `board/repo/v2` that name one
//! repository by its remote's identity form, at `/repositories/<remote>`;
//! and repo records of type `board/repo/v1`, which name one by its origin
//! and may mount a clone of it, until `board mount migrate` replaces them
//! (docs/architecture/paths/repos.md).

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::LogicalPath;
use crate::code::Code;
use crate::path::{MAX_PATH_BYTES, MAX_SEGMENT_BYTES, is_hidden};
use crate::pointer::{Mount, PointerError};

/// The `$type` of a repository record.
pub const REPOSITORY_TYPE: &str = "board/repo/v2";

/// The directory that holds a board's repository records.
pub const REPOSITORIES: &str = "/repositories";

/// The longest segment of a repository record's path, so that the file's
/// name, with `.json`, fits [`MAX_SEGMENT_BYTES`].
pub const MAX_REMOTE_SEGMENT_BYTES: usize = MAX_SEGMENT_BYTES - ".json".len();

/// A repository's identity: its remote in identity form, which
/// [`repo_identity`] computes, and the path of its record. Two clones of one
/// repository, over any protocol, give the same identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Remote {
    remote: String,
    path: LogicalPath,
}

impl Remote {
    /// The identity form, such as `crumbkit/board` or
    /// `gitlab.com/example-org/app`.
    pub fn as_str(&self) -> &str {
        &self.remote
    }

    /// Where the repository's record lives in every board:
    /// `/repositories/<remote>`.
    pub fn path(&self) -> &LogicalPath {
        &self.path
    }
}

impl fmt::Display for Remote {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.remote)
    }
}

impl Serialize for Remote {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.remote)
    }
}

impl JsonSchema for Remote {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Remote".into()
    }

    /// `owner/name` in lower case for GitHub, or a host with a dot, in lower
    /// case, and one or more path segments. The schema cannot say every
    /// rule; Board checks a repository record's `remote` with
    /// [`repo_identity`].
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^([a-z0-9-]+/[a-z0-9._-]+|[a-z0-9-]+(\\.[a-z0-9-]+)+(/[^/\\s]+)+)$"
        })
    }
}

/// Why a remote has no identity form. Each message names the remote
/// without its user and password.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error("{shown} is a local path, which names no repository outside this machine")]
    Local { shown: String },
    #[error("{shown} names no repository: {why}")]
    Unnamed { shown: String, why: &'static str },
    #[error(
        "{shown} gives the path segment {segment}, of {bytes} bytes, over {MAX_REMOTE_SEGMENT_BYTES}"
    )]
    TooLong {
        shown: String,
        segment: String,
        bytes: usize,
    },
    #[error("{shown} gives a record path of {bytes} bytes with `.json`, over {MAX_PATH_BYTES}")]
    PathTooLong { shown: String, bytes: usize },
}

impl IdentityError {
    pub fn code(&self) -> Code {
        match self {
            Self::Local { .. } => Code::RepoLocalRemote,
            Self::Unnamed { .. } => Code::RepoInvalid,
            Self::TooLong { .. } | Self::PathTooLong { .. } => Code::RepoTooLong,
        }
    }
}

/// The identity form of `remote`, a git origin URL or a remote in
/// identity form. Its tests read the table in `repo/remotes.json`.
///
/// - A GitHub remote, over https, ssh, git or git's scp-like form, with or
///   without a host alias such as `github.com-work`, or `owner/name`
///   itself, is `owner/name` in lower case, since GitHub ignores case.
/// - Any other host is `<host>/<path>`, with the host in lower case and no
///   scheme, user, password, port, trailing `.git` or trailing `/`, such as
///   `gitlab.com/example-org/app`. A GitHub owner holds no dot and a host
///   does, so the two never collide.
/// - A local path, a `file://` URL or a relative path names a directory on
///   one machine and has no identity form.
///
/// The identity of an identity is itself.
pub fn repo_identity(remote: &str) -> Result<Remote, IdentityError> {
    let shown = without_userinfo(remote);
    let unnamed = |why| IdentityError::Unnamed {
        shown: shown.clone(),
        why,
    };
    let local = || IdentityError::Local {
        shown: shown.clone(),
    };
    let (host, path) = match hosted(remote) {
        Hosted::Host(host, path) => (Some(host), path),
        Hosted::GitHub(path) => (None, path),
        Hosted::Local => return Err(local()),
        Hosted::Unnamed(why) => return Err(unnamed(why)),
    };
    let mut path = path.trim_start_matches('/');
    while let Some(shorter) = path.strip_suffix('/').or_else(|| path.strip_suffix(".git")) {
        path = shorter;
    }
    let segments: Vec<&str> = path.split('/').collect();
    let bad = |segment: &&str| {
        segment.is_empty()
            || *segment == "."
            || *segment == ".."
            || segment.chars().any(|c| c.is_whitespace() || is_hidden(c))
    };
    if segments.iter().any(bad) {
        return Err(unnamed(
            "its path has an empty, `.` or `..` segment, a space or a hidden character",
        ));
    }
    let host = host
        .map(str::to_ascii_lowercase)
        .filter(|host| !github_host(host));
    let remote = if let Some(host) = host {
        let label = |label: &str| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        };
        if !host.contains('.') || !host.split('.').all(label) {
            return Err(unnamed(
                "its host is not a domain name with a dot, such as gitlab.com; \
                 an ssh alias names no host Board can read",
            ));
        }
        format!("{host}/{path}")
    } else {
        let [owner, name] = segments[..] else {
            return Err(unnamed(
                "a GitHub repository is `owner/name` and nothing more",
            ));
        };
        let in_owner = |b: u8| b.is_ascii_alphanumeric() || b == b'-';
        let in_name = |b: u8| in_owner(b) || matches!(b, b'.' | b'_');
        if !owner.bytes().all(in_owner) || !name.bytes().all(in_name) {
            return Err(unnamed(
                "a GitHub owner holds letters, digits and `-`, and a name also `.` and `_`",
            ));
        }
        format!("{owner}/{name}").to_ascii_lowercase()
    };
    if let Some(segment) = (remote.split('/')).find(|s| s.len() > MAX_REMOTE_SEGMENT_BYTES) {
        return Err(IdentityError::TooLong {
            shown: shown.clone(),
            segment: segment.to_owned(),
            bytes: segment.len(),
        });
    }
    let path = format!("{REPOSITORIES}/{remote}");
    let bytes = path.len() + ".json".len();
    if bytes > MAX_PATH_BYTES {
        return Err(IdentityError::PathTooLong { shown, bytes });
    }
    let path = path
        .parse()
        .map_err(|_| unnamed("its path is not a board path"))?;
    Ok(Remote { remote, path })
}

/// Where a remote says its repository is.
enum Hosted<'a> {
    /// A host other than GitHub's, as written, and the path on it.
    Host(&'a str, &'a str),
    /// A GitHub repository's path, `owner/name` when well formed.
    GitHub(&'a str),
    Local,
    Unnamed(&'static str),
}

/// What `remote` names: a URL's host and path, an scp-like remote's, a
/// bare `owner/name` or `<host>/<path>`, or a directory on this machine.
fn hosted(remote: &str) -> Hosted<'_> {
    if let Some((scheme, rest)) = remote.split_once("://") {
        if scheme.eq_ignore_ascii_case("file") {
            return Hosted::Local;
        }
        let Some((authority, path)) = rest.split_once('/') else {
            return Hosted::Unnamed("it names a host and no path");
        };
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let host = match host.rsplit_once(':') {
            Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
            _ => host,
        };
        return Hosted::Host(host, path);
    }
    if remote.is_empty() {
        return Hosted::Unnamed("it is empty");
    }
    if remote.starts_with(['/', '~', '.']) {
        return Hosted::Local;
    }
    // git reads a colon before the first `/` as an scp-like remote.
    let slash = remote.find('/').unwrap_or(remote.len());
    if let Some(colon) = remote[..slash].find(':') {
        let at = &remote[..colon];
        let host = at.rsplit_once('@').map_or(at, |(_, host)| host);
        return Hosted::Host(host, &remote[colon + 1..]);
    }
    let first = &remote[..slash];
    if first.contains('.') {
        Hosted::Host(first, remote[slash..].trim_start_matches('/'))
    } else if remote[slash..].len() > 1
        && first
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && remote[slash + 1..]
            .split('/')
            .filter(|s| !s.is_empty())
            .count()
            == 1
    {
        Hosted::GitHub(remote)
    } else {
        Hosted::Local
    }
}

/// Whether `host`, in lower case, is GitHub's, or an ssh alias of it such
/// as `github.com-work`.
fn github_host(host: &str) -> bool {
    match host.strip_prefix("github.com") {
        Some("") => true,
        Some(alias) => alias.strip_prefix('-').is_some_and(|alias| {
            !alias.is_empty()
                && alias
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        }),
        None => false,
    }
}

/// A repository record: the repository's remote in identity form, and
/// nothing else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Repository")]
pub struct RepositoryRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    #[schemars(with = "RepositoryTag")]
    tag: RepositoryTag,
    /// The repository's remote in identity form: `owner/name` in lower case
    /// for GitHub, such as `crumbkit/board`, and otherwise the host in lower
    /// case and the path, such as `gitlab.com/example-org/app`. The record
    /// lives at `/repositories/<remote>`.
    pub remote: Remote,
}

/// A repository record's `$type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RepositoryTag;

impl Serialize for RepositoryTag {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REPOSITORY_TYPE)
    }
}

impl JsonSchema for RepositoryTag {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Tag".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "const": REPOSITORY_TYPE})
    }
}

/// What a repository record holds as it is written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepository {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    remote: String,
}

/// Why a record of type [`REPOSITORY_TYPE`] is invalid. Each message names
/// the value that would be valid when there is one.
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("a {REPOSITORY_TYPE} record holds `$type` and `remote` and nothing else, but {source}")]
    Shape { source: serde_json::Error },
    #[error("its remote `{shown}` is not in identity form; its identity form is `{identity}`")]
    NotIdentity { shown: String, identity: Remote },
    #[error("its remote is not a repository's: {source}")]
    Unnamed { source: IdentityError },
}

impl RepositoryError {
    pub fn code(&self) -> Code {
        Code::RepoInvalid
    }
}

impl RepositoryRecord {
    /// The record of the repository `remote` names.
    pub fn new(remote: Remote) -> Self {
        Self {
            tag: RepositoryTag,
            remote,
        }
    }

    /// The repository record a parsed record of type [`REPOSITORY_TYPE`]
    /// holds, whose `remote` must be in identity form.
    pub fn from_value(value: &Value) -> Result<Self, RepositoryError> {
        let raw = RawRepository::deserialize(value)
            .map_err(|source| RepositoryError::Shape { source })?;
        let identity =
            repo_identity(&raw.remote).map_err(|source| RepositoryError::Unnamed { source })?;
        if identity.as_str() != raw.remote {
            return Err(RepositoryError::NotIdentity {
                shown: without_userinfo(&raw.remote),
                identity,
            });
        }
        Ok(Self::new(identity))
    }

    /// The record as JSON.
    pub fn to_value(&self) -> Value {
        serde_json::json!({"$type": REPOSITORY_TYPE, "remote": self.remote.as_str()})
    }
}

/// The JSON Schema of [`REPOSITORY_TYPE`], draft 2020-12 without
/// `$schema`, as `types/board/repo/v2.schema.json` holds it. Board reads
/// the type by its Rust type; the schema says its shape to other readers.
pub fn repository_schema() -> Value {
    let mut settings = schemars::generate::SchemaSettings::draft2020_12();
    settings.meta_schema = None;
    settings
        .into_generator()
        .into_root_schema_for::<RepositoryRecord>()
        .into()
}

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
            return Err(PointerError::Unshown { shown });
        }
        let checkout = (raw.checkout)
            .map(
                |RawCheckout {
                     repo,
                     follow,
                     commit,
                 }| Mount::new(repo, follow, commit),
            )
            .transpose()?;
        Ok(Self {
            remote: raw.remote,
            checkout,
        })
    }
}

/// What begins a record's URI (docs/architecture/paths/uris.md).
pub const URI_SCHEME: &str = "board://";

/// The URI of the record at `path` in a board whose origin's stored form is
/// `remote`, such as `board://acme/team-board/tasks/a`.
pub fn record_uri(remote: &str, path: &LogicalPath) -> String {
    format!("{URI_SCHEME}{remote}{path}")
}

/// `remote`, a git origin URL, as it may be shown and stored: `owner/name`
/// for GitHub, and otherwise the URL without its user and password. Two
/// clones of one repository give the same value. Its tests read the table in
/// `repo/remotes.json`.
///
/// The form is reapplied until it holds still, so the stored form of a
/// stored form is itself: `owner/name.git.git` and `user@owner/name.git`
/// are both `owner/name`. Each change shortens the text, so this ends.
pub fn shown_remote(remote: &str) -> String {
    let mut shown = remote.to_owned();
    loop {
        let next = match github_remote(&shown) {
            Some((owner, name)) => format!("{owner}/{name}"),
            None => without_userinfo(&shown),
        };
        if next == shown {
            return shown;
        }
        shown = next;
    }
}

/// The owner and name of a GitHub remote: `owner/name` itself, or a URL
/// over https, ssh or git, or git's scp-like `user@github.com:`. ssh, git
/// and scp-like remotes may name a host alias such as `github.com-work`.
pub fn github_remote(remote: &str) -> Option<(&str, &str)> {
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
