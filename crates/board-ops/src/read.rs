//! `board get` and `board ls`: reading what a path names
//! (docs/architecture.md, Commands).

use std::collections::BTreeMap;

use board_model::{
    Classified, Code, Content, Fields, Kind, Location, LogicalPath, Object, Source, classify,
};
use board_tree::{GitError, Repository, ResolveError, Status};
use serde_json::Value;

use crate::board::Board;

/// A file as `get` returns it: where it is, its stored bytes, and its parsed
/// record or the code that says why it has none.
#[derive(Clone, Debug, PartialEq)]
pub struct GetReport {
    pub location: Location,
    pub display: LogicalPath,
    pub bytes: Vec<u8>,
    pub record: Result<Value, Code>,
}

/// One line of `ls`: a directory, or an entry with its kind and the status
/// its path and parsing give it.
#[derive(Clone, Debug, PartialEq)]
pub enum Listed {
    Directory {
        name: String,
        logical: LogicalPath,
    },
    Entry {
        name: String,
        logical: Option<LogicalPath>,
        kind: Kind,
        status: Status,
    },
}

/// What `ls` lists, sorted by name.
#[derive(Clone, Debug, PartialEq)]
pub struct LsReport {
    pub location: Location,
    pub display: LogicalPath,
    pub listed: Vec<Listed>,
}

impl Board {
    /// Resolves `path`, following pointers, and returns the file there. A
    /// directory is `record::not_a_record`, and a file over 16 MiB is
    /// `json::too_large`.
    pub fn get(&self, path: &LogicalPath) -> Result<GetReport, ReadError> {
        let location = self.locate(path)?;
        let not_a_record = || ReadError::NotARecord { path: path.clone() };
        let Object::Blob(blob) = location.object else {
            return Err(not_a_record());
        };
        let repository = repository_of(&location)?;
        let bytes = repository
            .read(&location.board.source, &location.path, blob)
            .map_err(|source| ReadError::Git { source })?
            .ok_or(ReadError::Unread {
                path: path.clone(),
                code: Code::JsonTooLarge,
            })?;
        let record = match classify(&location.path, Some(&bytes)) {
            Classified {
                content: Content::Record { value, .. },
                ..
            } => Ok(value),
            Classified {
                code: Some(code), ..
            } => Err(code),
            Classified { code: None, .. } => Err(Code::RecordNotARecord),
        };
        Ok(GetReport {
            display: display(&location, path),
            location,
            bytes,
            record,
        })
    }

    /// Resolves `path`, following pointers, and lists the directory there
    /// one level deep, without following pointers inside it. A path that
    /// ends at a file lists that file.
    pub fn ls(&self, path: &LogicalPath) -> Result<LsReport, ReadError> {
        let location = self.locate(path)?;
        let display = display(&location, path);
        let repository = repository_of(&location)?;
        // Only the listed directory, or a listed file's own directory, is
        // walked, so a listing costs what it shows, not the whole board.
        let scope = match location.object {
            Object::Directory => location.path.as_str(),
            Object::Blob(_) => location
                .path
                .rsplit_once('/')
                .map_or("", |(parent, _)| parent),
        };
        let entries = repository
            .walk_under(&location.board.source, scope)
            .map_err(|source| ReadError::Git { source })?;
        let prefix = if location.path.is_empty() {
            String::new()
        } else {
            format!("{}/", location.path)
        };
        let mut listed: BTreeMap<String, Listed> = BTreeMap::new();
        for entry in entries {
            let (name, below) = match location.object {
                Object::Blob(_) if entry.path == location.path => {
                    (entry.path.rsplit('/').next().unwrap_or(&entry.path), false)
                }
                Object::Blob(_) => continue,
                Object::Directory => {
                    let Some(rest) = entry.path.strip_prefix(&prefix) else {
                        continue;
                    };
                    rest.split_once('/')
                        .map_or((rest, false), |(name, _)| (name, true))
                }
            };
            let child = |segment: &str| join(&display, segment);
            let item = if below {
                let Some(logical) = child(name) else { continue };
                Listed::Directory {
                    name: name.to_owned(),
                    logical,
                }
            } else {
                let kind = classify_entry(&repository, &location.board.source, &entry);
                Listed::Entry {
                    name: name.to_owned(),
                    logical: match location.object {
                        Object::Blob(_) => Some(display.clone()),
                        Object::Directory => entry
                            .logical
                            .as_ref()
                            .and_then(|logical| logical.segments().last().and_then(child)),
                    },
                    kind: kind.kind(),
                    status: match (entry.status, kind.code) {
                        (Status::Ok, Some(code)) => Status::Degraded(code),
                        (status, _) => status,
                    },
                }
            };
            listed.entry(name.to_owned()).or_insert(item);
        }
        Ok(LsReport {
            location,
            display,
            listed: listed.into_values().collect(),
        })
    }

    fn locate(&self, path: &LogicalPath) -> Result<Location, ReadError> {
        self.repository()
            .resolve(&Source::WorkingTree, path)
            .map_err(|source| ReadError::Resolve { source })
    }
}

/// The repository a Location is in, opened again when it is a mounted one.
fn repository_of(location: &Location) -> Result<Repository, ReadError> {
    Repository::open(&location.board.repository).map_err(|source| ReadError::Git { source })
}

/// An entry's kind and what parsing found, reading it only when it is a
/// `.json` file.
pub(crate) fn classify_entry(
    repository: &Repository,
    source: &Source,
    entry: &board_tree::Entry,
) -> Classified {
    if !board_model::needs_bytes(&entry.path) {
        return classify(&entry.path, None);
    }
    let read = entry.blob.ok_or(Code::RepoUnreadable).and_then(|blob| {
        repository
            .read(source, &entry.path, blob)
            .map_err(|error| error.code())
    });
    match read {
        Ok(bytes) => classify(&entry.path, bytes.as_deref()),
        Err(code) => Classified {
            content: Content::Unparsed(Kind::Json),
            code: Some(code),
        },
    }
}

/// The display path of `location`, or the path asked for if it has none.
pub(crate) fn display(location: &Location, asked: &LogicalPath) -> LogicalPath {
    location.display_path().unwrap_or_else(|| asked.clone())
}

fn join(parent: &LogicalPath, segment: &str) -> Option<LogicalPath> {
    if parent.segments().next().is_none() {
        format!("/{segment}")
    } else {
        format!("{parent}/{segment}")
    }
    .parse()
    .ok()
}

/// A path `get` or `ls` cannot read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    Resolve { source: ResolveError },
    #[error("{path} is not a record")]
    NotARecord { path: LogicalPath },
    #[error("{path} cannot be read: {code}")]
    Unread { path: LogicalPath, code: Code },
    #[error(transparent)]
    Git { source: GitError },
}

impl ReadError {
    pub fn code(&self) -> Code {
        match self {
            Self::Resolve { source } => source.code(),
            Self::NotARecord { .. } => Code::RecordNotARecord,
            Self::Unread { code, .. } => *code,
            Self::Git { source } => source.code(),
        }
    }

    pub fn fields(&self) -> Fields {
        match self {
            Self::Resolve { source } => source.fields(),
            Self::NotARecord { path } | Self::Unread { path, .. } => Fields::at(path),
            Self::Git { .. } => Fields::default(),
        }
    }
}
