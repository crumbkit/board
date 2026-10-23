//! Pointers: records of type `board/mount/v1` that resolution continues
//! through (docs/architecture/paths.md, Pointers: links and mounts).

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;

use crate::code::Code;
use crate::json::{JsonError, canonical_json, parse_json};
use crate::location::ObjectId;
use crate::path::LogicalPath;

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
    if bytes.len() > MAX_RECORD_BYTES {
        return Ok(None);
    }
    match serde_json::from_slice::<RawTag>(bytes) {
        Ok(RawTag { tag: Some(tag) }) if tag == POINTER_TYPE => {}
        _ => return Ok(None),
    }
    let value = parse_json(bytes).map_err(|source| PointerError::Json { source })?;
    pointer_from_value(&value).map(Some)
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
mod tests {
    use super::*;

    fn link(path: &str) -> Pointer {
        Pointer::Link(path.parse().unwrap())
    }

    #[test]
    fn a_link_is_a_pointer_record_with_one_path() {
        let bytes = br#"{"$type": "board/mount/v1", "path": "/work/x"}"#;
        assert_eq!(pointer(bytes).unwrap(), Some(link("/work/x")));
    }

    #[test]
    fn a_record_of_another_type_is_not_a_pointer() {
        for bytes in [
            &br#"{"$type": "lr/task/v1", "path": "/work/x"}"#[..],
            br#"{"path": "/work/x"}"#,
            br#"{"$type": 1}"#,
            b"not json",
            b"[]",
        ] {
            assert_eq!(pointer(bytes).unwrap(), None);
        }
    }

    #[test]
    fn a_pointer_with_an_unknown_field_is_invalid() {
        let admitted = br#"{"$type": "board/mount/v1", "path": "/x"}"#;
        assert!(pointer(admitted).is_ok());
        let refused = br#"{"$type": "board/mount/v1", "path": "/x", "note": 1}"#;
        assert_eq!(pointer(refused).unwrap_err().code(), Code::MountInvalid);
    }

    #[test]
    fn a_pointer_whose_path_is_malformed_or_missing_is_invalid() {
        for refused in [
            &br#"{"$type": "board/mount/v1", "path": "x"}"#[..],
            br#"{"$type": "board/mount/v1"}"#,
        ] {
            assert_eq!(pointer(refused).unwrap_err().code(), Code::MountInvalid);
        }
    }

    #[test]
    fn a_pointer_must_pass_the_strict_parse() {
        let admitted = br#"{"$type": "board/mount/v1", "path": "/x"}"#;
        assert!(pointer(admitted).is_ok());
        let refused = br#"{"$type": "board/mount/v1", "path": "/x", "path": "/y"}"#;
        assert_eq!(pointer(refused).unwrap_err().code(), Code::JsonDuplicateKey);
    }

    #[test]
    fn the_record_limit_is_sixteen_mebibytes() {
        assert_eq!(MAX_RECORD_BYTES, 16_777_216);
    }

    #[test]
    fn a_record_over_the_size_limit_is_never_read_as_a_pointer() {
        let small = format!(
            r#"{{"$type": "board/mount/v1", "path": "/x"}}{}"#,
            " ".repeat(64)
        );
        assert!(pointer(small.as_bytes()).unwrap().is_some());
        let large = format!(
            r#"{{"$type": "board/mount/v1", "path": "/x"}}{}"#,
            " ".repeat(MAX_RECORD_BYTES)
        );
        assert_eq!(pointer(large.as_bytes()).unwrap(), None);
    }

    fn mount(repo: &str) -> String {
        format!(
            r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{}"}}"#,
            "ab".repeat(20)
        )
    }

    #[test]
    fn a_mount_names_a_local_repository_at_a_commit() {
        let parsed = pointer(mount("~/src/board").as_bytes()).unwrap();
        assert_eq!(
            parsed,
            Some(Pointer::Mount(Mount {
                repo: "~/src/board".to_owned(),
                follow: "main".to_owned(),
                commit: ObjectId::Sha1([0xab; 20]),
            }))
        );
    }

    #[test]
    fn a_remote_repository_cannot_be_mounted() {
        for local in ["/srv/board", "../board", "~/src/board", "dir:with-colon"] {
            assert!(
                pointer(mount(local).as_bytes()).unwrap().is_some(),
                "{local}"
            );
        }
        for remote in [
            "https://example.com/board.git",
            "ssh://host/board",
            "git@example.com:board.git",
        ] {
            let error = pointer(mount(remote).as_bytes()).unwrap_err();
            assert_eq!(error.code(), Code::MountRemoteUnsupported, "{remote}");
        }
    }

    #[test]
    fn a_pointer_holds_a_link_or_a_mount_never_both_or_neither() {
        let commit = "ab".repeat(20);
        for refused in [
            r#"{"$type": "board/mount/v1"}"#.to_owned(),
            format!(
                r#"{{"$type": "board/mount/v1", "path": "/x", "repo": "/r", "follow": "main", "commit": "{commit}"}}"#
            ),
            format!(r#"{{"$type": "board/mount/v1", "repo": "/r", "commit": "{commit}"}}"#),
            r#"{"$type": "board/mount/v1", "repo": "/r", "follow": "main"}"#.to_owned(),
        ] {
            assert_eq!(
                pointer(refused.as_bytes()).unwrap_err().code(),
                Code::MountInvalid,
                "{refused}"
            );
        }
    }

    #[test]
    fn a_mount_commit_must_be_a_full_lowercase_id() {
        assert!(pointer(mount("/r").as_bytes()).unwrap().is_some());
        for commit in [
            "AB".repeat(20),
            "ab".repeat(19),
            "zz".repeat(20),
            "abc".to_owned(),
        ] {
            let bytes = format!(
                r#"{{"$type": "board/mount/v1", "repo": "/r", "follow": "main", "commit": "{commit}"}}"#
            );
            assert_eq!(
                pointer(bytes.as_bytes()).unwrap_err().code(),
                Code::MountInvalid,
                "{commit}"
            );
        }
        assert!(ObjectId::from_hex(&"cd".repeat(32)).is_some());
    }
}
