//! A record's URI, `board://` followed by its board's remote and its path,
//! and the known board a URI names (docs/architecture/paths/uris.md).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use board_model::{
    Code, Fields, LogicalPath, Refusal, URI_SCHEME, github_remote, record_uri, shown_remote,
};
use board_tree::{Origin, own_origin};

use super::{KnownBoards, against, shown};
use crate::Board;

/// The URI of the record at `path` in the board whose repository is at
/// `repository`, from the stored form of its `origin`, or
/// `board::uri::no_remote` when it has none.
pub fn uri_of(repository: &Path, path: &LogicalPath) -> Result<String, Box<Refusal>> {
    match own_origin(repository) {
        Origin::Url(url) => Ok(record_uri(&shown_remote(&url), path)),
        Origin::Unread(why) => Err(no_remote(repository, &why)),
        Origin::Outside => Err(no_remote(repository, "it is not a git repository")),
    }
}

fn no_remote(repository: &Path, why: &str) -> Box<Refusal> {
    let message = format!(
        "the board at {} has no origin, so its records have no URI: {why}",
        repository.display()
    );
    Box::new(Refusal::new(Code::UriNoRemote, message, &Fields::default()))
}

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
        let mut seen = HashSet::new();
        let (mut origins, mut best): (Vec<String>, Vec<(usize, PathBuf, &str)>) = (vec![], vec![]);
        for root in self.known.boards.iter().map(|root| against(file, root)) {
            let Ok(board) = Board::open(&root, home.map(Path::to_path_buf)) else {
                continue;
            };
            let repository = board.repository().path();
            let Origin::Url(url) = own_origin(repository) else {
                continue;
            };
            if !seen.insert(repository.to_path_buf()) {
                continue;
            }
            let remote = shown_remote(&url);
            if rest == remote {
                return Err(invalid("it names a board and no path; add / for its root"));
            }
            let after = rest.strip_prefix(remote.as_str());
            if let Some(after) = after.filter(|after| after.starts_with('/')) {
                best.push((remote.len(), root, after));
            }
            origins.push(remote);
        }
        let longest = best.iter().map(|(length, _, _)| *length).max();
        best.retain(|(length, _, _)| Some(*length) == longest);
        match best.as_slice() {
            [] => Err(self.not_found(uri, rest, &origins)),
            [(_, root, after)] => {
                let path = after
                    .parse()
                    .map_err(|error| invalid(&format!("{error}")))?;
                Ok((root.clone(), path))
            }
            [(_, _, after), ..] => {
                let roots: Vec<String> = (best.iter())
                    .map(|(_, root, _)| root.display().to_string())
                    .collect();
                let message = format!(
                    "{uri} names a remote that more than one known board has as its origin: {}",
                    roots.join(", ")
                );
                let hint = format!(
                    "leave one clone of that remote among the boards in `{}`, or choose one with `board -C <dir> get {}`",
                    shown(file),
                    after
                );
                let refusal = Refusal::new(Code::UriAmbiguous, message, &Fields::default());
                Err(Box::new(refusal.with_hint(hint)))
            }
        }
    }

    /// `board::uri::not_found` for `uri`, naming the origins of the known
    /// boards read and, when it reads as one, the GitHub remote to clone.
    fn not_found(&self, uri: &str, rest: &str, origins: &[String]) -> Box<Refusal> {
        let file = shown(self.file.as_deref());
        let read = match origins {
            [] => format!("the known boards in `{file}` have no origin"),
            _ => format!(
                "the known boards in `{file}` have the origins {}",
                origins.join(", ")
            ),
        };
        let message = format!("no known board has the remote {uri} names; {read}");
        let owner_name: Vec<&str> = rest.splitn(3, '/').take(2).collect();
        let owner_name = owner_name.join("/");
        let remote = match github_remote(&owner_name) {
            Some(_) => format!("`{owner_name}`"),
            None => "at the start of the URI".to_owned(),
        };
        let hint = format!(
            "clone the board whose remote is {remote} and list the clone in `{file}` as {{\"boards\": [\"<board directory>\"]}}"
        );
        let refusal = Refusal::new(Code::UriNotFound, message, &Fields::default());
        Box::new(refusal.with_hint(hint))
    }
}
