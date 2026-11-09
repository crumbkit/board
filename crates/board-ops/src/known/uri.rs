//! A record's URI, `board://` followed by its board's remote and its path,
//! and the known board a URI names (docs/architecture/paths/uris.md).

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

/// A known board whose origin begins a URI: the origin's length, the
/// board's root, and the rest of the URI.
type Match<'a> = (usize, PathBuf, &'a str);

impl KnownBoards {
    /// The root of the one known board whose origin, in its stored form,
    /// is the longest that begins what follows `board://` in `uri` and ends
    /// at a `/`, and the record's path, the rest of the URI. `home` is what
    /// a mount's `~/` means.
    pub fn by_uri(
        &self,
        uri: &str,
        home: Option<&Path>,
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
        let file = self.file.as_deref();
        let (mut seen, mut skipped) = (HashSet::new(), Vec::new());
        let (mut origins, mut best): (Vec<String>, Vec<Match>) = (vec![], vec![]);
        for root in self.known.boards.iter().map(|root| against(file, root)) {
            let Ok(board) = Board::open(&root, home.map(Path::to_path_buf)) else {
                skipped.push(root.display().to_string());
                continue;
            };
            let repository = board.repository();
            let Some(remote) = repository.remote() else {
                continue;
            };
            if !seen.insert(repository.path().to_path_buf()) {
                continue;
            }
            if after(&format!("{rest}/"), &remote) == Some("/") {
                return Err(invalid("it names a board and no path; add / for its root"));
            }
            if let Some(tail) = after(rest, &remote) {
                best.push((remote.len(), root, tail));
            }
            origins.push(remote);
        }
        let passed = match skipped.as_slice() {
            [] => String::new(),
            _ => format!(
                "; these known boards could not be opened: {}",
                skipped.join(", ")
            ),
        };
        let longest = best.iter().map(|(length, _, _)| *length).max();
        best.retain(|(length, _, _)| Some(*length) == longest);
        match best.as_slice() {
            [] => Err(Box::new(self.not_found(uri, rest, &origins, &passed))),
            [(_, root, tail)] => {
                let path = (LogicalPath::root().join(tail))
                    .map_err(|error| invalid(&format!("{error}")))?;
                Ok((root.clone(), path))
            }
            [(_, _, tail), ..] => {
                let roots: Vec<String> = (best.iter())
                    .map(|(_, root, _)| root.display().to_string())
                    .collect();
                let message = format!(
                    "{uri} names a remote that more than one known board has as its origin: {}{passed}",
                    roots.join(", ")
                );
                let hint = format!(
                    "leave one clone of that remote among the boards in `{}`, or choose one with `board -C <dir> get {tail}`",
                    shown(file),
                );
                let refusal = Refusal::new(Code::UriAmbiguous, message, &Fields::default());
                Err(Box::new(refusal.with_hint(hint)))
            }
        }
    }

    /// `board::uri::not_found` for `uri`, naming the origins of the known
    /// boards read, those `passed` over, and the remote to clone: a GitHub
    /// `owner/name`, or else the part of the URI before the record's path.
    fn not_found(&self, uri: &str, rest: &str, origins: &[String], passed: &str) -> Refusal {
        let file = shown(self.file.as_deref());
        let read = match origins {
            [] => format!("the known boards in `{file}` have no origin"),
            _ => format!(
                "the known boards in `{file}` have the origins {}",
                origins.join(", ")
            ),
        };
        let message = format!("no known board has the remote {uri} names; {read}{passed}");
        let owner_name: Vec<&str> = rest.splitn(3, '/').take(2).collect();
        let owner_name = owner_name.join("/");
        let remote = match github_remote(&owner_name) {
            Some(_) => format!("`{owner_name}`"),
            None => format!("the part of `{uri}` before the record's path"),
        };
        let hint = format!(
            "clone the board whose remote is {remote} and list the clone in `{file}` as {{\"boards\": [\"<board directory>\"]}}"
        );
        let refusal = Refusal::new(Code::UriNotFound, message, &Fields::default());
        refusal.with_hint(hint)
    }
}
