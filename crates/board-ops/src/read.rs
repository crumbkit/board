//! `board get` and `board ls`: reading what a path names
//! (docs/architecture.md, Commands).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use board_index::Fallback;
use board_model::{
    BINARY_NAME, Classified, Code, Content, Fields, Kind, Location, LogicalPath, Object, Refusal,
    Role, Source, classify,
};
use board_tree::{Contents, GitError, Reader, ResolveError, Resolver, Status};
use serde_json::Value;

use crate::attachment::FetchError;
use crate::board::Board;
use crate::check::CheckError;
use crate::index::Refreshed;

/// A file as `get` returns it: where it is, its stored bytes, and its parsed
/// record or the refusal that says why it has none.
#[derive(Clone, Debug, PartialEq)]
pub struct GetReport {
    pub location: Location,
    pub display: LogicalPath,
    pub bytes: Vec<u8>,
    pub record: Result<Value, Refusal>,
}

/// One line of `ls`: a directory, or an entry with its kind and the status
/// `check` gives it.
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
    /// Why the index was in memory, if it was.
    pub memory: Option<Fallback>,
    /// The work the listing's check did.
    pub work: Refreshed,
}

impl Board {
    /// Resolves `path`, following pointers, and returns the file there. A
    /// directory is `record::not_a_record`, and a file over 16 MiB is
    /// `json::too_large`.
    pub fn get(&self, path: &LogicalPath) -> Result<GetReport, ReadError> {
        let mut view = self.working_view()?;
        let location = self.locate_in(&mut view, path)?;
        read_at(&mut view, location, path, true)
    }

    /// Each of `paths` as [`Board::get`] gives it, in order, read as the
    /// iterator is advanced. All resolve in one snapshot of the working
    /// tree, so each directory is listed once. A snapshot that cannot be
    /// taken is the one error, for every path.
    pub fn get_each<'a>(
        &'a self,
        paths: &'a [LogicalPath],
    ) -> Result<impl Iterator<Item = Result<GetReport, ReadError>> + 'a, ReadError> {
        let mut view = self.working_view()?;
        Ok(paths.iter().map(move |path| {
            let location = self.locate_in(&mut view, path)?;
            read_at(&mut view, location, path, true)
        }))
    }

    /// One snapshot of the working tree to resolve and read in.
    fn working_view(&self) -> Result<Resolver<'_, 'static>, ReadError> {
        (self.repositories().snapshot()).map_err(|source| ReadError::Resolve {
            source: ResolveError::Git(source),
        })
    }

    /// Resolves `path`, following pointers, and lists the directory there
    /// one level deep, without following pointers inside it. A path that
    /// ends at a file lists that file. Each entry has the kind and status
    /// `check` gives it, from the index kept under `cache`.
    pub fn ls(&self, path: &LogicalPath, cache: Option<&Path>) -> Result<LsReport, ReadError> {
        let location = self.locate(path)?;
        let display = display(&location, path);
        let index = self.index(cache).map_err(CheckError::from);
        let snapshot = index
            .and_then(|index| self.snapshot(index, Some(&display)))
            .map_err(|source| ReadError::Check { source })?;
        let checked: HashMap<(&Source, &str), (Kind, Status)> = snapshot
            .report
            .entries
            .iter()
            .map(|entry| {
                let at = (&entry.source, entry.path.as_str());
                (at, (entry.kind, entry.status))
            })
            .collect();
        let repository = self
            .repositories()
            .repository(&location.board)
            .map_err(|source| ReadError::Git { source })?;
        // The check walked every source; a source it did not reach is walked
        // only in the listed directory, or a listed file's own directory.
        let walked = snapshot
            .walked
            .iter()
            .find(|(source, _)| *source == location.board.source);
        let fallback;
        let entries = if let Some((_, entries)) = walked {
            entries
        } else {
            let scope = match location.object {
                Object::Directory => location.path.as_str(),
                Object::Blob(_) => location.path.rsplit_once('/').map_or("", |(up, _)| up),
            };
            fallback = repository
                .walk_under(&location.board.source, scope)
                .map_err(|source| ReadError::Git { source })?;
            &fallback
        };
        let prefix = if location.path.is_empty() {
            String::new()
        } else {
            format!("{}/", location.path)
        };
        let mut listed: BTreeMap<String, Listed> = BTreeMap::new();
        let reader = &mut repository.reader();
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
                // A scoped check skips an entry whose path has no logical
                // path, so `ls` gives it what its path and parsing say.
                let (kind, status) = checked
                    .get(&(&location.board.source, entry.path.as_str()))
                    .copied()
                    .unwrap_or_else(|| {
                        let classified = classify_entry(reader, &location.board.source, entry);
                        let status = match (entry.status, classified.code) {
                            (Status::Ok, Some(code)) => Status::Degraded(code),
                            (status, _) => status,
                        };
                        (classified.kind(), status)
                    });
                Listed::Entry {
                    name: name.to_owned(),
                    logical: match location.object {
                        Object::Blob(_) => Some(display.clone()),
                        Object::Directory => entry
                            .logical
                            .as_ref()
                            .and_then(|logical| logical.segments().last().and_then(child)),
                    },
                    kind,
                    status,
                }
            };
            listed.entry(name.to_owned()).or_insert(item);
        }
        Ok(LsReport {
            location,
            display,
            listed: listed.into_values().collect(),
            memory: snapshot.report.memory,
            work: snapshot.report.work,
        })
    }

    /// The logical paths that complete `typed`, one directory level deep,
    /// sorted: each name in the directory before its last `/`, reached by
    /// following pointers, that starts with what follows. A directory ends
    /// in `/`. A name that starts with `.` is offered only once a `.` is
    /// typed, as shells do. Nothing is hashed, validated or indexed, and
    /// anything that cannot be read offers nothing.
    pub fn complete(&self, typed: &str) -> Vec<String> {
        let typed = if typed.is_empty() { "/" } else { typed };
        let Some((parent, partial)) = typed.rsplit_once('/') else {
            return Vec::new();
        };
        let directory = if parent.is_empty() {
            Ok(LogicalPath::root())
        } else {
            parent.parse()
        };
        let Some(location) = directory
            .ok()
            .filter(|_| typed.starts_with('/'))
            .and_then(|directory| self.locate(&directory).ok())
            .filter(|location| location.object == Object::Directory)
        else {
            return Vec::new();
        };
        let names = self
            .repositories()
            .repository(&location.board)
            .ok()
            .and_then(|repository| {
                repository
                    .names(&location.board.source, &location.path)
                    .ok()
            })
            .unwrap_or_default();
        let mut offered: Vec<String> = names
            .into_iter()
            .filter(|name| {
                name.segment.starts_with(partial)
                    && (partial.starts_with('.') || !name.segment.starts_with('.'))
            })
            .map(|name| {
                let slash = if name.directory { "/" } else { "" };
                format!("{parent}/{}{slash}", name.segment)
            })
            .collect();
        offered.sort();
        offered.dedup();
        offered
    }

    /// Where `path` leads in the working tree. When nothing is at a path
    /// that ends `.json` and the path without it resolves, the error says
    /// that the path names an entry by its file.
    pub(crate) fn locate(&self, path: &LogicalPath) -> Result<Location, ReadError> {
        let _timed = self.timed("resolve");
        let resolve = |path: &LogicalPath| self.repositories().resolve(&Source::WorkingTree, path);
        Self::located(path, resolve)
    }

    /// Where `path` leads in `view`, as [`Board::locate`] says.
    pub(crate) fn locate_in(
        &self,
        view: &mut Resolver,
        path: &LogicalPath,
    ) -> Result<Location, ReadError> {
        let _timed = self.timed("resolve");
        Self::located(path, |path| view.resolve(path))
    }

    /// Where `path` leads by `resolve`, as [`Board::locate`] says.
    fn located(
        path: &LogicalPath,
        mut resolve: impl FnMut(&LogicalPath) -> Result<Location, ResolveError>,
    ) -> Result<Location, ReadError> {
        resolve(path).map_err(|source| {
            let stem = path.as_str().strip_suffix(".json");
            match stem.and_then(|stem| stem.parse::<LogicalPath>().ok()) {
                Some(stem)
                    if matches!(source, ResolveError::NotFound { .. })
                        && resolve(&stem).is_ok() =>
                {
                    ReadError::ByFile { source, stem }
                }
                _ => ReadError::Resolve { source },
            }
        })
    }
}

/// The file at `location`, which `path` resolved to, with the blob of the
/// bytes read. With `latest`, a file that changed since it was resolved is
/// read as it is now; without, it is [`GitError::Changed`].
pub(crate) fn read_at(
    view: &mut Resolver,
    mut location: Location,
    path: &LogicalPath,
    latest: bool,
) -> Result<GetReport, ReadError> {
    let Object::Blob(blob) = location.object else {
        return Err(ReadError::NotARecord { path: path.clone() });
    };
    let read = view.read(&location.board, &location.path, blob);
    let Contents { blob, bytes } = if latest {
        board_tree::latest(read)
    } else {
        read
    }
    .map_err(|source| ReadError::Git { source })?
    .ok_or(ReadError::Unread {
        path: path.clone(),
        code: Code::JsonTooLarge,
    })?;
    location.object = Object::Blob(blob);
    let display = display(&location, path);
    let fields = Fields {
        blob: Some(blob),
        ..Fields::at(&display)
    };
    let not_a_record = |code: Option<Code>| {
        let message = format_args!("{display} is not a record");
        Refusal::new(code.unwrap_or(Code::RecordNotARecord), message, &fields)
    };
    let record = match classify(&location.path, Some(&bytes)) {
        Classified {
            content: Content::Record { value, .. },
            ..
        } => Ok(value),
        // classify keeps only the code, so bytes that failed are parsed
        // again, and only then, for the parser's message.
        Classified {
            content: Content::Unparsed(_),
            code,
        } => Err(
            match Role::of(&location.path).and_then(|role| role.parse_error(&bytes)) {
                Some((code, error)) => {
                    let message = format_args!("{display} is not a record: {error}");
                    Refusal::new(code, message, &fields)
                }
                None => not_a_record(code),
            },
        ),
        Classified { code, .. } => Err(not_a_record(code)),
    };
    Ok(GetReport {
        location,
        display,
        bytes,
        record,
    })
}

/// An entry's kind and what parsing found, reading it only when it is a
/// `.json` or Markdown file.
pub(crate) fn classify_entry(
    reader: &mut Reader<'_>,
    source: &Source,
    entry: &board_tree::Entry,
) -> Classified {
    if Role::of(&entry.path).is_none() {
        return classify(&entry.path, None);
    }
    let read = entry.blob.ok_or(Code::RepoUnreadable).and_then(|blob| {
        reader
            .read(source, &entry.path, blob)
            .map_err(|error| error.code())
    });
    match read {
        Ok(contents) => classify(&entry.path, contents.map(|read| read.bytes).as_deref()),
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

pub(crate) fn join(parent: &LogicalPath, segment: &str) -> Option<LogicalPath> {
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
    /// Nothing is at the path, which ends `.json`, but `stem`, the path
    /// without it, resolves.
    #[error("{source}")]
    ByFile {
        source: ResolveError,
        stem: LogicalPath,
    },
    #[error("{path} is not a record")]
    NotARecord { path: LogicalPath },
    #[error("{path} cannot be read: {code}")]
    Unread { path: LogicalPath, code: Code },
    #[error(transparent)]
    Git { source: GitError },
    #[error(transparent)]
    Check { source: CheckError },
    #[error("{path} {source}")]
    Fetch {
        path: LogicalPath,
        source: FetchError,
    },
}

impl ReadError {
    pub fn code(&self) -> Code {
        match self {
            Self::Resolve { source } | Self::ByFile { source, .. } => source.code(),
            Self::NotARecord { .. } => Code::RecordNotARecord,
            Self::Unread { code, .. } => *code,
            Self::Git { source } => source.code(),
            Self::Check { source } => source.code(),
            Self::Fetch { source, .. } => source.code(),
        }
    }

    /// This error as `board <command>` reports it. A path that names an
    /// entry by its file is hinted without `.json`, since a logical path
    /// leaves the extension out.
    pub fn refusal(&self, command: &'static str) -> Refusal {
        let fields = match self {
            Self::Resolve { source } | Self::ByFile { source, .. } => source.fields(),
            Self::NotARecord { path } | Self::Unread { path, .. } | Self::Fetch { path, .. } => {
                Fields::at(path)
            }
            Self::Git { .. } => Fields::default(),
            Self::Check { source } => return source.refusal(),
        };
        let refusal = Refusal::new(self.code(), self, &fields);
        match self {
            Self::ByFile { stem, .. } => refusal.with_hint(format!(
                "a logical path leaves out .json: try `{BINARY_NAME} {command} {stem}`"
            )),
            _ => refusal,
        }
    }
}
