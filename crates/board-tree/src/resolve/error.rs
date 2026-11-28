//! Why a path does not resolve.

use std::sync::Arc;

use board_model::{Code, Fields, LogicalPath, ObjectId, PointerError};

use super::MAX_HOPS;
use crate::git::GitError;
use crate::symlink::Refused;

/// How many names a refusal for a path that names nothing offers.
pub(crate) const MAX_NEAREST: usize = 5;

/// The most entries a directory may hold for a refusal for a path that
/// names nothing in it to offer their names. Ranking a larger one costs
/// more than the `board ls` the names would save.
pub const MAX_NEAREST_LISTED: usize = 10_000;

/// A path that does not resolve.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// `ancestor` is the deepest ancestor of `path` that resolves, and
    /// `beside` the segments in it nearest the one that missed, nearest
    /// first, as the refusal ranks them, among the entries resolution sees:
    /// empty when none is near, resolution read no directory there, or the
    /// directory holds more than [`MAX_NEAREST_LISTED`] entries.
    #[error("nothing is at {path}")]
    NotFound {
        path: LogicalPath,
        ancestor: LogicalPath,
        beside: Vec<String>,
    },
    #[error("{segment:?} in {path} names more than one entry")]
    Shadowed { path: LogicalPath, segment: String },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    Submodule { path: LogicalPath },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    Unreadable { path: LogicalPath },
    #[error("the link at {link} points to {target}, which is missing")]
    Dangling {
        path: LogicalPath,
        link: LogicalPath,
        target: LogicalPath,
    },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    InvalidPointer {
        path: LogicalPath,
        pointer: String,
        #[source]
        source: Arc<PointerError>,
    },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    RepoMissing { path: LogicalPath, repo: String },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    CommitMissing { path: LogicalPath, commit: ObjectId },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    Cycle { path: LogicalPath },
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    TooManyHops { path: LogicalPath },
    /// `path` reaches the symlink at the physical `link`, which its first
    /// `reached` segments name, and which leads to no file.
    #[error("{path} {}", self.predicate().unwrap_or_default())]
    Symlink {
        path: LogicalPath,
        reached: usize,
        link: Box<str>,
        refused: Refused,
    },
    #[error(transparent)]
    Git(#[from] GitError),
}

impl ResolveError {
    /// Nothing at `path`, whose deepest ancestor that resolves is
    /// `ancestor`, with no names read beside it.
    pub fn not_found(path: &LogicalPath, ancestor: LogicalPath) -> Self {
        Self::NotFound {
            path: path.clone(),
            ancestor,
            beside: Vec::new(),
        }
    }

    /// This error with the names `listed` gives for the segment that
    /// missed, called only when it is [`ResolveError::NotFound`].
    pub(crate) fn beside(mut self, listed: impl FnOnce(&str) -> Vec<String>) -> Self {
        if let Self::NotFound {
            path,
            ancestor,
            beside,
        } = &mut self
            && let Some(missed) = path.segments().nth(ancestor.segments().count())
        {
            *beside = listed(missed);
        }
        self
    }

    /// What the message says of its path, for an error whose message starts
    /// with that path.
    pub(crate) fn predicate(&self) -> Option<String> {
        Some(match self {
            Self::Submodule { .. } => "reaches a submodule, which is not descended".to_owned(),
            Self::Unreadable { .. } => "reaches a file that cannot be read".to_owned(),
            Self::InvalidPointer {
                pointer, source, ..
            } => format!("reaches the invalid pointer {pointer:?}: {source}"),
            Self::RepoMissing { repo, .. } => {
                format!("mounts the repository {repo:?}, which cannot be opened")
            }
            Self::CommitMissing { commit, .. } => {
                format!("mounts commit {commit}, which its repository does not hold")
            }
            Self::Cycle { .. } => {
                "returns to a board and path it already resolved through a pointer".to_owned()
            }
            Self::TooManyHops { .. } => format!("follows more than {MAX_HOPS} pointers"),
            Self::Symlink {
                path,
                reached,
                refused,
                ..
            } if *reached < path.segments().count() => {
                format!("passes through {}, {refused}", path.prefix(*reached))
            }
            Self::Symlink { refused, .. } => format!("is {refused}"),
            Self::NotFound { .. }
            | Self::Shadowed { .. }
            | Self::Dangling { .. }
            | Self::Git(_) => {
                return None;
            }
        })
    }

    /// Whether resolving stopped because `path` is itself a mount whose
    /// repository cannot be opened or lacks the pinned commit.
    pub fn is_unread_mount(&self, path: &LogicalPath) -> bool {
        matches!(
            self,
            Self::RepoMissing { path: at, .. } | Self::CommitMissing { path: at, .. } if at == path
        )
    }

    pub fn code(&self) -> Code {
        match self {
            Self::NotFound { .. } => Code::PathNotFound,
            Self::Shadowed { .. } => Code::PathShadowed,
            Self::Submodule { .. } => Code::RepoSubmodule,
            Self::Unreadable { .. } => Code::RepoUnreadable,
            Self::Dangling { .. } => Code::RefDangling,
            Self::InvalidPointer { source, .. } => source.code(),
            Self::RepoMissing { .. } => Code::MountRepoMissing,
            Self::CommitMissing { .. } => Code::MountCommitMissing,
            Self::Cycle { .. } => Code::ResolveCycle,
            Self::TooManyHops { .. } => Code::ResolveTooManyHops,
            Self::Symlink { refused, .. } => refused.code,
            Self::Git(error) => error.code(),
        }
    }

    /// The path at fault: the link for a dangling link, else the path.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Dangling { link, .. } => Fields::at(link),
            Self::NotFound {
                path,
                ancestor,
                beside,
            } => Fields {
                ancestor: Some(ancestor.clone()),
                nearest: beside.clone(),
                ..Fields::at(path)
            },
            Self::Symlink { path, refused, .. } => Fields {
                target: Some(refused.target.clone()),
                ..Fields::at(path)
            },
            Self::Shadowed { path, .. }
            | Self::Submodule { path }
            | Self::Unreadable { path }
            | Self::InvalidPointer { path, .. }
            | Self::RepoMissing { path, .. }
            | Self::CommitMissing { path, .. }
            | Self::Cycle { path }
            | Self::TooManyHops { path } => Fields::at(path),
            Self::Git(error) => error.fields(),
        }
    }
}

/// The names in `beside` nearest `missing`, a segment that names nothing,
/// at most [`MAX_NEAREST`], as [`ranked`] orders them.
pub(crate) fn nearest(missing: &str, beside: &[String]) -> Vec<String> {
    ranked(missing, beside).take(MAX_NEAREST).cloned().collect()
}

/// Each name in `beside` near `missing`, a segment that names nothing,
/// nearest first. A name that starts with `missing` ranks first,
/// shortest first; then a name within an edit distance of a third of
/// `missing`'s length, closest first; then a name that shares a prefix of at
/// least that third, and at least one character, longest prefix first. Ties
/// go by name. A name that starts with `.` is offered only for a `missing`
/// that does too, as `board complete` offers it. A `.json` that ends
/// `missing` is left out first, as a logical path leaves it out.
pub(crate) fn ranked<'n>(
    missing: &str,
    beside: &'n [String],
) -> impl Iterator<Item = &'n String> + use<'n> {
    let missing = missing.strip_suffix(".json").unwrap_or(missing);
    let length = missing.chars().count();
    let within = length / 3;
    let mut ranked: Vec<(u8, usize, &String)> = beside
        .iter()
        .filter(|name| missing.starts_with('.') || !name.starts_with('.'))
        .filter_map(|name| {
            if name.starts_with(missing) {
                return Some((0, name.chars().count(), name));
            }
            if name.chars().count().abs_diff(length) <= within {
                let distance = strsim::levenshtein(missing, name);
                if distance <= within {
                    return Some((1, distance, name));
                }
            }
            let shared = (missing.chars().zip(name.chars()))
                .take_while(|(one, two)| one == two)
                .count();
            (shared >= within.max(1)).then(|| (2, usize::MAX - shared, name))
        })
        .collect();
    ranked.sort();
    ranked.into_iter().map(|(.., name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_name_that_starts_with_the_missing_segment_ranks_before_a_near_one() {
        let beside = names(&["notes.md", "notex", "nothing", "zeta"]);
        assert_eq!(
            nearest("notes", &beside),
            names(&["notes.md", "notex", "nothing"])
        );
    }

    #[test]
    fn near_names_rank_by_edit_distance_then_by_shared_prefix() {
        let beside = names(&[
            "20261127T1947Z-95a5",
            "20261127T2010Z-0c1d",
            "20261127T1950Z-95a6",
            "a-slug",
        ]);
        assert_eq!(
            nearest("20261127T1948Z-95a5", &beside),
            names(&[
                "20261127T1947Z-95a5",
                "20261127T1950Z-95a6",
                "20261127T2010Z-0c1d"
            ])
        );
    }

    #[test]
    fn no_name_is_offered_when_none_is_close_and_at_most_five_when_many_are() {
        assert_eq!(
            nearest("qqqq", &names(&["notes.md", "zeta", ".board"])),
            Vec::<String>::new()
        );
        let many: Vec<String> = (0..9).map(|number| format!("task-{number}")).collect();
        assert_eq!(nearest("task", &many), many[..MAX_NEAREST].to_vec());
    }

    #[test]
    fn a_missing_segment_is_ranked_without_its_json() {
        let beside = names(&["notes.json", "other"]);
        assert_eq!(nearest("other.json", &beside), names(&["other"]));
    }

    #[test]
    fn a_hidden_name_is_offered_only_for_a_hidden_segment() {
        let beside = names(&[".board", "board"]);
        assert_eq!(nearest("boar", &beside), names(&["board"]));
        assert_eq!(nearest(".boar", &beside), names(&[".board"]));
    }

    #[test]
    fn the_fields_of_a_missing_path_name_the_entries_listed_for_its_missing_segment() {
        let missing = ResolveError::not_found(
            &"/decisions/notes/x".parse().unwrap(),
            "/decisions".parse().unwrap(),
        )
        .beside(|missed| nearest(missed, &names(&["notes.md", "zeta"])));
        assert_eq!(missing.fields().nearest, names(&["notes.md"]));
        let unread = ResolveError::not_found(
            &"/decisions/notes".parse().unwrap(),
            "/decisions".parse().unwrap(),
        );
        assert_eq!(unread.fields().nearest, Vec::<String>::new());
    }
}
