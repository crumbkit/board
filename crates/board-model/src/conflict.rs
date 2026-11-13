//! Conflict records: records of type `board/conflict/v1` that a sync's
//! merge commit holds for each file both sides changed
//! (docs/architecture/writes.md, Sync).

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::{Value, json};

use crate::location::ObjectId;

/// The `$type` of a conflict record.
pub const CONFLICT_TYPE: &str = "board/conflict/v1";

/// The directory, from a board's root, that holds conflict records.
pub const CONFLICTS_DIRECTORY: &str = "conflicts/";

/// How the two sides disagreed about a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Both edited the file, or both added it with different content.
    BothChanged,
    /// One side edited the file and the other deleted it.
    EditDelete,
    /// One side holds a file and the other a directory.
    FileDirectory,
}

impl ConflictKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BothChanged => "both_changed",
            Self::EditDelete => "edit_delete",
            Self::FileDirectory => "file_directory",
        }
    }
}

/// A side of a merge: the clone that merged, or the branch it fetched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Local,
    Remote,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

/// What one side held at the path, and who last changed it there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    /// The blob the side held, or `None` when it held no file.
    pub blob: Option<ObjectId>,
    /// The commit on that side that last changed the path.
    pub commit: ObjectId,
    /// That commit's author, as `Name <email>`.
    pub author: String,
}

/// A `board/conflict/v1` record. `path` is the file's path in the
/// repository, `kept` the side whose content is in the merge commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub path: String,
    pub kind: ConflictKind,
    pub kept: Side,
    /// The blob the common ancestor held, if it held a file.
    pub base: Option<ObjectId>,
    pub local: Version,
    pub remote: Version,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    path: String,
    kind: ConflictKind,
    kept: Side,
    base: Option<String>,
    local: RawVersion,
    remote: RawVersion,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVersion {
    blob: Option<String>,
    commit: String,
    author: String,
}

fn id(text: &str) -> Result<ObjectId, String> {
    ObjectId::from_hex(text).ok_or_else(|| format!("{text:?} is not a git object id"))
}

fn maybe(text: Option<&str>) -> Result<Option<ObjectId>, String> {
    text.map(id).transpose()
}

impl RawVersion {
    fn version(&self) -> Result<Version, String> {
        Ok(Version {
            blob: maybe(self.blob.as_deref())?,
            commit: id(&self.commit)?,
            author: self.author.clone(),
        })
    }
}

fn shown(version: &Version) -> Value {
    json!({
        "blob": version.blob.map(|blob| blob.to_string()),
        "commit": version.commit.to_string(),
        "author": version.author,
    })
}

impl Conflict {
    /// The record as JSON.
    pub fn to_value(&self) -> Value {
        json!({
            "$type": CONFLICT_TYPE,
            "path": self.path,
            "kind": self.kind.as_str(),
            "kept": self.kept.as_str(),
            "base": self.base.map(|base| base.to_string()),
            "local": shown(&self.local),
            "remote": shown(&self.remote),
        })
    }

    /// The conflict a parsed record of type [`CONFLICT_TYPE`] holds.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let raw = Raw::deserialize(value).map_err(|error| error.to_string())?;
        Ok(Self {
            path: raw.path,
            kind: raw.kind,
            kept: raw.kept,
            base: maybe(raw.base.as_deref())?,
            local: raw.local.version()?,
            remote: raw.remote.version()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Conflict {
        let version = |byte, blob| Version {
            blob,
            commit: ObjectId::Sha1([byte; 20]),
            author: "Ana <ana@example.com>".to_owned(),
        };
        Conflict {
            path: "tasks/a.json".to_owned(),
            kind: ConflictKind::EditDelete,
            kept: Side::Local,
            base: Some(ObjectId::Sha1([1; 20])),
            local: version(2, Some(ObjectId::Sha1([3; 20]))),
            remote: version(4, None),
        }
    }

    #[test]
    fn a_conflict_reads_back_as_written() {
        let conflict = sample();
        assert_eq!(Conflict::from_value(&conflict.to_value()), Ok(conflict));
    }

    #[test]
    fn a_record_with_a_bad_id_or_an_unknown_field_is_refused() {
        let mut bad_id = sample().to_value();
        bad_id["local"]["commit"] = json!("not-an-id");
        assert!(
            Conflict::from_value(&bad_id)
                .unwrap_err()
                .contains("object id")
        );
        let mut extra = sample().to_value();
        extra["more"] = json!(1);
        assert!(Conflict::from_value(&extra).is_err());
    }
}
