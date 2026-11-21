//! A record's URI, `board://` followed by its board's remote and its path,
//! and the board a URI names (docs/architecture/paths/uris.md).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use board_model::{Code, Fields, LogicalPath, Refusal, URI_SCHEME, github_remote, record_uri};
use board_tree::Repository;

use super::{KnownBoards, against, shown};
use crate::Board;

/// The URI of the record at `path` in `repository`'s board, from the stored
/// form of its `origin`, or `board::uri::no_remote` when it has none.
pub fn uri_of(repository: &Repository, path: &LogicalPath) -> Result<String, Box<Refusal>> {
    let Some(remote) = repository.remote() else {
        let message = format!(
            "the board at {} has no origin git can read, so its records have no URI",
            repository.path().display()
        );
        return Err(Box::new(Refusal::new(
            Code::UriNoRemote,
            message,
            &Fields::default(),
        )));
    };
    Ok(record_uri(&remote, path))
}

/// What follows `remote` in `rest` when `rest` begins with it and then a
/// `/`. A GitHub remote, `owner/name`, matches in any case, as GitHub does.
fn after<'a>(rest: &'a str, remote: &str) -> Option<&'a str> {
    let head = rest.get(..remote.len())?;
    let same = if github_remote(remote).is_some() {
        head.eq_ignore_ascii_case(remote)
    } else {
        head == remote
    };
    let tail = &rest[remote.len()..];
    (same && tail.starts_with('/')).then_some(tail)
}

/// A board whose origin begins a URI: the origin's length, the board's
/// root, its repository's path, and the rest of the URI.
type Match<'a> = (usize, PathBuf, PathBuf, &'a str);

/// What one URI's lookup has read.
struct Lookup<'a> {
    /// What follows `board://` in the URI.
    rest: &'a str,
    /// What a mount's `~/` means.
    home: Option<&'a Path>,
    /// The repository path of each board read.
    seen: HashSet<PathBuf>,
    /// The boards that could not be opened, or whose mounts could not be read.
    skipped: Vec<String>,
    /// The origin of each board read.
    origins: Vec<String>,
    /// The boards whose origin begins the URI.
    best: Vec<Match<'a>>,
    /// Each board read, to read its mounts.
    opened: Vec<(PathBuf, Board)>,
}

impl Lookup<'_> {
    /// Reads the board at `root`, which is passed over and named among the
    /// boards that could not be opened when it is no board and `named`. A
    /// board that is not `named` is known by its repository's root, since
    /// `root` may be a directory inside it. `Err` when the URI names this
    /// board and no path; otherwise the board's repository path, if it
    /// opened.
    fn read(&mut self, root: PathBuf, named: bool) -> Result<Option<PathBuf>, ()> {
        let Ok(board) = Board::open(&root, self.home.map(Path::to_path_buf)) else {
            if named {
                self.skipped.push(root.display().to_string());
            }
            return Ok(None);
        };
        let repository = board.repository().path().to_path_buf();
        if !self.seen.insert(repository.clone()) {
            return Ok(Some(repository));
        }
        let root = if named { root } else { repository.clone() };
        if let Some(remote) = board.repository().remote() {
            if after(&format!("{}/", self.rest), &remote) == Some("/") {
                return Err(());
            }
            if let Some(tail) = after(self.rest, &remote) {
                let found = (remote.len(), root.clone(), repository.clone(), tail);
                self.best.push(found);
            }
            self.origins.push(remote);
        }
        self.opened.push((root, board));
        Ok(Some(repository))
    }

    /// The repositories the boards opened so far mount, naming each board
    /// whose mounts could not be read.
    fn mounted(&mut self, cache: Option<&Path>) -> Vec<PathBuf> {
        let mut mounted = Vec::new();
        for (root, board) in &self.opened {
            match board.mounted_repositories(cache) {
                Ok(repositories) => mounted.extend(repositories),
                Err(_) => self
                    .skipped
                    .push(format!("the mounts of {}", root.display())),
            }
        }
        mounted
    }
}

impl KnownBoards {
    /// [`KnownBoards::by_uri_from`] with no start directory and the default
    /// cache.
    pub fn by_uri(
        &self,
        uri: &str,
        home: Option<&Path>,
    ) -> Result<(PathBuf, LogicalPath), Box<Refusal>> {
        self.by_uri_from(uri, home, None, None)
    }

    /// The root of the one board whose origin, in its stored form, is the
    /// longest that begins what follows `board://` in `uri` and ends at a
    /// `/`, and the record's path, the rest of the URI. The boards listed
    /// under `boards` are read first. When none has the URI's remote, the
    /// personal board, the board holding `start` and each board that one of
    /// these or a listed board mounts are read, their index in `cache`, and
    /// the board holding `start` wins over the others with that origin.
    /// `home` is what a mount's `~/` means.
    pub fn by_uri_from(
        &self,
        uri: &str,
        home: Option<&Path>,
        start: Option<&Path>,
        cache: Option<&Path>,
    ) -> Result<(PathBuf, LogicalPath), Box<Refusal>> {
        let invalid = |why: &str| {
            let message = format!("{uri} is not a board URI: {why}");
            Box::new(Refusal::new(Code::UriInvalid, message, &Fields::default()))
        };
        let rest = uri
            .strip_prefix(URI_SCHEME)
            .ok_or_else(|| invalid("it does not start with board://"))?;
        if !rest.trim_start_matches('/').contains('/') {
            return Err(invalid("it names no remote and path after board://"));
        }
        let no_path = |()| invalid("it names a board and no path; add / for its root");
        let file = self.file.as_deref();
        let mut lookup = Lookup {
            rest,
            home,
            seen: HashSet::new(),
            skipped: Vec::new(),
            origins: Vec::new(),
            best: Vec::new(),
            opened: Vec::new(),
        };
        for root in self.known.boards.iter().map(|root| against(file, root)) {
            lookup.read(root, true).map_err(no_path)?;
        }
        let second = lookup.best.is_empty();
        let mut holding = None;
        if second {
            if let Some(personal) = &self.known.personal {
                lookup
                    .read(against(file, personal), true)
                    .map_err(no_path)?;
            }
            if let Some(start) = start {
                holding = lookup.read(start.to_path_buf(), false).map_err(no_path)?;
            }
            for root in lookup.mounted(cache) {
                lookup.read(root, false).map_err(no_path)?;
            }
        }
        let Lookup {
            skipped,
            origins,
            mut best,
            ..
        } = lookup;
        let passed = match skipped.as_slice() {
            [] => String::new(),
            _ => format!(
                "; these boards could not be opened or read: {}",
                skipped.join(", ")
            ),
        };
        let longest = best.iter().map(|(length, ..)| *length).max();
        best.retain(|(length, ..)| Some(*length) == longest);
        if best
            .iter()
            .any(|(.., repository, _)| Some(repository) == holding.as_ref())
        {
            best.retain(|(.., repository, _)| Some(repository) == holding.as_ref());
        }
        match best.as_slice() {
            [] => Err(Box::new(self.not_found(uri, rest, &origins, &passed))),
            [(_, root, _, tail)] => {
                let path = (LogicalPath::root().join(tail))
                    .map_err(|error| invalid(&format!("{error}")))?;
                Ok((root.clone(), path))
            }
            [(.., tail), ..] => {
                let roots: Vec<String> = (best.iter())
                    .map(|(_, root, ..)| root.display().to_string())
                    .collect();
                let message = format!(
                    "{uri} names a remote that more than one board has as its origin: {}{passed}",
                    roots.join(", ")
                );
                let hint = if second {
                    format!(
                        "run the command inside the clone you mean, or choose one with `board -C <dir> get {tail}`"
                    )
                } else {
                    format!(
                        "leave one clone of that remote among the boards in `{}`, or choose one with `board -C <dir> get {tail}`",
                        shown(file),
                    )
                };
                let refusal = Refusal::new(Code::UriAmbiguous, message, &Fields::default());
                Err(Box::new(refusal.with_hint(hint)))
            }
        }
    }

    /// `board::uri::not_found` for `uri`. The message names the origins of
    /// the boards read and those `passed` over. The hint says how to read a
    /// clone the person has, with the record's path when the remote is a
    /// GitHub `owner/name`, and which remote to clone: that `owner/name`,
    /// or else the part of the URI before the record's path.
    fn not_found(&self, uri: &str, rest: &str, origins: &[String], passed: &str) -> Refusal {
        let file = shown(self.file.as_deref());
        let read = match origins {
            [] => "no board read has an origin".to_owned(),
            _ => format!("the boards read have the origins {}", origins.join(", ")),
        };
        let message = format!("no board read has the remote {uri} names; {read}{passed}");
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        let owner_name = parts[..2].join("/");
        let (remote, read) = match (github_remote(&owner_name), parts.get(2)) {
            (Some(_), Some(path)) => (
                format!("`{owner_name}`"),
                format!("`board -C <dir> get /{path}` with the clone as <dir>"),
            ),
            _ => (
                format!("the part of `{uri}` before the record's path"),
                "`board -C <dir> get` with the clone as <dir> and the record's path".to_owned(),
            ),
        };
        let hint = format!(
            "if you have a clone of that board, run {read}; otherwise clone the board whose remote is {remote} and list the clone in `{file}` as {{\"boards\": [\"<board directory>\"]}}"
        );
        let refusal = Refusal::new(Code::UriNotFound, message, &Fields::default());
        refusal.with_hint(hint)
    }
}

#[cfg(test)]
mod tests;
