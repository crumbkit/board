//! The references each entry of a kept decision makes, and where each one
//! resolved, kept under the decision's key
//! (docs/architecture/performance/warm/decisions.md).

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use board_model::{Board, Code, Digest, Location, LogicalPath, Object, ObjectId, Source};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::open::IndexError;
use crate::store::{Decided, Writer};

/// One reference a record or Markdown file makes, and where it led.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reached {
    pub pointer: String,
    /// Whether it is a Markdown link, whose `pointer` is its destination.
    pub link: bool,
    pub target: LogicalPath,
    /// Where it resolved, or `None` when it dangles.
    pub location: Option<Location>,
    pub code: Option<Code>,
}

/// The references one entry of a kept decision makes, in order, by the
/// entry's path in its source.
pub type KeptReferences = (String, Vec<Reached>);

impl Writer {
    /// Keeps under `key` the references `references` gives, by the path in
    /// the board `own` of the entry that makes each, in order for each path:
    /// one row for each path. A location in `own` itself is kept without its
    /// board, since a key names a tree, not a commit.
    ///
    /// # Panics
    ///
    /// Never: the rows hold only strings, booleans and arrays, which always
    /// serialize.
    pub fn put_references<'r>(
        &self,
        key: &Digest,
        own: &Board,
        references: impl IntoIterator<Item = (&'r str, &'r Reached)>,
    ) -> Result<(), IndexError> {
        let mut by_path: BTreeMap<&str, Vec<RawReached<'r>>> = BTreeMap::new();
        for (path, reached) in references {
            by_path.entry(path).or_default().push(RawReached {
                pointer: (&reached.pointer).into(),
                link: reached.link,
                target: reached.target.as_str().into(),
                location: reached.location.as_ref().map(|at| encoded(at, own)),
                code: reached.code.map(|code| code.as_str().into()),
            });
        }
        let key = key.hex();
        let mut statement = self.connection().prepare_cached(
            "INSERT OR REPLACE INTO main.resolutions (key, path, reached) VALUES (?1, ?2, ?3)",
        )?;
        for (path, reached) in by_path {
            let text = serde_json::to_string(&reached);
            let text = text.expect("strings, booleans and arrays always serialize");
            statement.execute(params![key, path, text])?;
        }
        Ok(())
    }

    /// The references kept under `key`, by path, each path's in order, with
    /// each location kept without its board in the board `own`.
    pub fn references(&self, key: &Digest, own: &Board) -> Result<Vec<KeptReferences>, IndexError> {
        let rows = self
            .connection()
            .prepare_cached(
                "SELECT path, reached FROM main.resolutions WHERE key = ?1 ORDER BY path",
            )?
            .query_map([key.hex()], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<(String, String)>, _>>()?;
        rows.into_iter()
            .map(|(path, text)| {
                let read = serde_json::from_str::<Vec<RawReached<'_>>>(&text).ok();
                let reached = read.and_then(|raw| {
                    (raw.into_iter())
                        .map(|raw| decoded_reached(raw, own))
                        .collect::<Option<Vec<Reached>>>()
                });
                let reached = reached.ok_or_else(|| IndexError::Unreadable {
                    path: path.clone(),
                    logical: None,
                    column: "reached",
                    value: text.clone(),
                })?;
                Ok((path, reached))
            })
            .collect()
    }
}

impl Decided {
    /// The blob the decision kept under `key` holds for the entry at `path`:
    /// `None` when it keeps no such entry, which another process's prune
    /// may have deleted.
    pub fn kept_blob(
        &self,
        key: &Digest,
        path: &str,
    ) -> Result<Option<Option<ObjectId>>, IndexError> {
        let mut statement = (self.index.connection)
            .prepare_cached("SELECT blob FROM main.decided WHERE key = ?1 AND path = ?2")?;
        let mut rows = statement.query(params![key.hex(), path])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let blob: Option<String> = row.get(0)?;
        Ok(Some(blob.and_then(|blob| ObjectId::from_hex(&blob))))
    }
}

/// One reference as the index keeps it. `link` is left out when false, and
/// `location` and `code` when absent.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReached<'r> {
    #[serde(borrow)]
    pointer: std::borrow::Cow<'r, str>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    link: bool,
    #[serde(borrow)]
    target: std::borrow::Cow<'r, str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    location: Option<RawLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

/// A location as the index keeps it: its board only when it is not the
/// decided board, as its repository and source, each `None` where it is
/// that board's; and its pointers and `follow` only when it has them.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocation {
    path: String,
    /// The blob's id, or `None` for a directory.
    object: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    board: Option<(Option<RawPath>, Option<String>)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pointers: Vec<RawLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    follow: Option<String>,
}

/// A repository path: its text, or its bytes when it is not UTF-8.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum RawPath {
    Text(String),
    Bytes(Vec<u8>),
}

/// `location` as the index keeps it, in the board `own`.
fn encoded(location: &Location, own: &Board) -> RawLocation {
    let Board { repository, source } = &location.board;
    let board = (location.board != *own).then(|| {
        let repository = (*repository != own.repository).then(|| match repository.to_str() {
            Some(text) => RawPath::Text(text.to_owned()),
            None => RawPath::Bytes(repository.as_os_str().as_bytes().to_vec()),
        });
        let source = (*source != own.source).then(|| match source {
            Source::WorkingTree => "worktree".to_owned(),
            Source::Commit(commit) => commit.to_string(),
        });
        (repository, source)
    });
    RawLocation {
        path: location.path.clone(),
        object: match location.object {
            Object::Blob(blob) => Some(blob.to_string()),
            Object::Directory => None,
        },
        board,
        pointers: (location.pointers.iter())
            .map(|pointer| encoded(pointer, own))
            .collect(),
        follow: location.follow.clone(),
    }
}

/// The reference `raw` keeps, in the board `own`, or `None` when it reads
/// as none.
fn decoded_reached(raw: RawReached<'_>, own: &Board) -> Option<Reached> {
    Some(Reached {
        pointer: raw.pointer.into_owned(),
        link: raw.link,
        target: raw.target.parse().ok()?,
        location: match raw.location {
            Some(location) => Some(decoded(location, own)?),
            None => None,
        },
        code: match raw.code {
            Some(code) => Some(Code::parse(&code)?),
            None => None,
        },
    })
}

/// The location `raw` keeps, in the board `own`, or `None` when it reads as
/// none.
fn decoded(raw: RawLocation, own: &Board) -> Option<Location> {
    let board = match raw.board {
        None => own.clone(),
        Some((repository, source)) => Board {
            repository: match repository {
                None => own.repository.clone(),
                Some(RawPath::Text(text)) => PathBuf::from(text),
                Some(RawPath::Bytes(bytes)) => Path::new(OsStr::from_bytes(&bytes)).to_path_buf(),
            },
            source: match source.as_deref() {
                None => own.source.clone(),
                Some("worktree") => Source::WorkingTree,
                Some(commit) => Source::Commit(ObjectId::from_hex(commit)?),
            },
        },
    };
    let object = match raw.object {
        None => Object::Directory,
        Some(blob) => Object::Blob(ObjectId::from_hex(&blob)?),
    };
    let pointers = (raw.pointers.into_iter())
        .map(|pointer| decoded(pointer, own))
        .collect::<Option<Vec<Location>>>()?;
    Some(Location {
        board,
        path: raw.path,
        object,
        pointers,
        follow: raw.follow,
    })
}
