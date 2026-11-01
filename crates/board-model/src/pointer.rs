//! Pointers: records of type `board/mount/v1` that resolution continues
//! through (docs/architecture/paths.md, Pointers: links and mounts).

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;

use crate::code::Code;
use crate::json::{JsonError, canonical_json, parse_json};
use crate::location::ObjectId;
use crate::path::LogicalPath;
use crate::record::{Kind, Role};

/// The `$type` of a pointer.
pub const POINTER_TYPE: &str = "board/mount/v1";

/// The largest record Board reads, in bytes.
pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;

/// Where a pointer leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pointer {
    /// A path in the board that holds the link.
    Link(LogicalPath),
    /// The root of another repository's tree at one commit.
    Mount(Mount),
}

/// A mounted repository, read at `commit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    /// A local path: absolute, `~/`-relative, or relative to the working
    /// directory of the repository that holds the mount.
    pub repo: String,
    /// What `board mount update` advances `commit` to.
    pub follow: String,
    pub commit: ObjectId,
}

#[derive(Deserialize)]
struct RawTag {
    #[serde(rename = "$type")]
    tag: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPointer {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    path: Option<LogicalPath>,
    repo: Option<String>,
    follow: Option<String>,
    commit: Option<String>,
}

/// The pointer `bytes` hold, if they are a record whose `$type` is
/// [`POINTER_TYPE`]. Anything else, including bytes that are not a JSON
/// object, is not a pointer; deciding what it is instead is not this
/// function's job. A pointer must also pass the strict parse.
pub fn pointer(bytes: &[u8]) -> Result<Option<Pointer>, PointerError> {
    // A struct deserializes from a sequence too, which would read
    // `["board/mount/v1"]` as tagged.
    if bytes.len() > MAX_RECORD_BYTES || bytes.trim_ascii_start().first() != Some(&b'{') {
        return Ok(None);
    }
    match serde_json::from_slice::<RawTag>(bytes) {
        Ok(RawTag { tag: Some(tag) }) if tag == POINTER_TYPE => {}
        _ => return Ok(None),
    }
    let value = parse_json(bytes).map_err(|source| PointerError::Json { source })?;
    pointer_from_value(&value).map(Some)
}

/// Whether the file at the `/`-separated physical `path` can hold a
/// pointer: only a plain `.json` record file, [`Role::Json`], can. Only that
/// role classifies a file as [`Kind::Pointer`], and resolution follows a
/// pointer only from a file this admits.
pub fn can_hold_pointer(path: &str) -> bool {
    Role::of(path) == Some(Role::Json)
}

/// Whether a file of `role` that [`Role::classify`] gave `kind` and `code`
/// holds no pointer: it is JSON that parsed, strictly or with comments, and
/// has no `$type` or one that is valid and not [`POINTER_TYPE`]. Resolution trusts this in
/// place of reading the file, so [`pointer()`] must answer `Ok(None)` for
/// every file this calls plain, and the property test checks it. The
/// index's stored facts and tree-row codes depend on [`pointer()`] and
/// [`Role::classify`], so any change to what either answers bumps the
/// index's `INDEX_VERSION`.
pub fn holds_no_pointer(role: Role, kind: Kind, code: Option<Code>) -> bool {
    role == Role::Json && code.is_none() && matches!(kind, Kind::Json | Kind::Record)
}

/// The pointer a parsed record of type [`POINTER_TYPE`] holds.
pub(crate) fn pointer_from_value(value: &Value) -> Result<Pointer, PointerError> {
    let raw = RawPointer::deserialize(value).map_err(|source| PointerError::Invalid { source })?;
    match raw {
        RawPointer {
            path: Some(path),
            repo: None,
            follow: None,
            commit: None,
            ..
        } => Ok(Pointer::Link(path)),
        RawPointer {
            path: None,
            repo: Some(repo),
            follow: Some(follow),
            commit: Some(commit),
            ..
        } => {
            if is_remote(&repo) {
                return Err(PointerError::Remote { repo });
            }
            let Some(commit) = ObjectId::from_hex(&commit) else {
                return Err(PointerError::Commit { commit });
            };
            Ok(Pointer::Mount(Mount {
                repo,
                follow,
                commit,
            }))
        }
        _ => Err(PointerError::Target),
    }
}

/// The mount pointer `bytes` with its `commit` set to `commit`, in
/// canonical form.
pub fn repinned(bytes: &[u8], commit: ObjectId) -> Result<Vec<u8>, JsonError> {
    let mut value = parse_json(bytes)?;
    value["commit"] = Value::String(commit.to_string());
    Ok(canonical_json(&value).into_bytes())
}

/// Whether `repo` names a remote repository: a URL, or git's scp-like
/// `user@host:path`.
fn is_remote(repo: &str) -> bool {
    repo.contains("://")
        || repo
            .split_once(':')
            .is_some_and(|(before, _)| before.contains('@') && !before.contains('/'))
}

/// A record that claims to be a pointer but is not a valid one.
#[derive(Debug, thiserror::Error)]
pub enum PointerError {
    #[error("pointer is not strict json: {source}")]
    Json {
        #[source]
        source: JsonError,
    },
    #[error("pointer is invalid: {source}")]
    Invalid {
        #[source]
        source: serde_json::Error,
    },
    #[error("pointer must hold either path, or repo, follow and commit")]
    Target,
    #[error("pointer commit {commit:?} is not a commit id")]
    Commit { commit: String },
    #[error("pointer repo {repo:?} is remote, and only local repositories mount")]
    Remote { repo: String },
}

impl PointerError {
    pub fn code(&self) -> Code {
        match self {
            Self::Json { source } => source.code(),
            Self::Invalid { .. } | Self::Target | Self::Commit { .. } => Code::MountInvalid,
            Self::Remote { .. } => Code::MountRemoteUnsupported,
        }
    }
}

#[cfg(test)]
mod tests;
