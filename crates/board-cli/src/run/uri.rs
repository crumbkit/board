//! Paths given as `board://` URIs, which choose the known board a run
//! reads (docs/architecture/paths/uris.md).

use std::path::{Path, PathBuf};

use board_model::{Code, Refusal, URI_SCHEME};

use super::{Command, Given, Refs, plain};
use crate::config::Context;

/// The root of the known board the URIs among the paths `command` reads
/// name, with each URI replaced by its record's path in that board, or none
/// when no path is a URI. URIs that name two boards are refused, since a
/// run reads one, and so is a URI given to a command that writes.
pub(super) fn chosen(
    command: &mut Command,
    context: &Context,
) -> Result<Option<PathBuf>, Box<Refusal>> {
    if let Some(refusal) = written_path(command).and_then(read_only) {
        return Err(Box::new(refusal));
    }
    let paths = read_paths(command);
    let mut known = None;
    let mut chosen: Option<(PathBuf, String)> = None;
    for given in paths {
        if !given.0.starts_with(URI_SCHEME) {
            continue;
        }
        let known = match &known {
            Some(known) => known,
            None => known.insert(context.known()?),
        };
        let (root, path) = known.by_uri(&given.0, context.home.as_deref())?;
        if let Some((first, named)) = &chosen
            && *first != root
        {
            let message = format!(
                "{named} names the board at {} and {} the board at {}; one run reads one board",
                first.display(),
                given.0,
                root.display()
            );
            let hint = "run the command once for each board's URIs";
            return Err(Box::new(plain(Code::CliUsage, &message).with_hint(hint)));
        }
        chosen.get_or_insert((root, given.0.clone()));
        given.0 = path.to_string();
    }
    Ok(chosen.map(|(root, _)| root))
}

/// The paths `command` reads, each of which may be a `board://` URI.
fn read_paths(command: &mut Command) -> Vec<&mut Given> {
    match command {
        Command::Check { path, .. } | Command::Ls { path, .. } => path.iter_mut().collect(),
        Command::Get { paths, .. } | Command::Refs(Refs { paths, .. }) => {
            paths.iter_mut().collect()
        }
        Command::Uri { path } => vec![path],
        _ => Vec::new(),
    }
}

/// The path `command` writes, and the command's name, if it writes one.
fn written_path(command: &Command) -> Option<(&Given, &'static str)> {
    match command {
        Command::Put(putting) => putting.path.as_ref().map(|path| (path, "put")),
        Command::Rm(removal) => Some((removal.path(), "rm")),
        _ => None,
    }
}

/// `refusal`, and when a URI chose the run's board at `clone` and nothing
/// is at the path, a hint that the clone may be behind its remote.
pub(super) fn behind(refusal: Refusal, clone: Option<&Path>) -> Refusal {
    let Some(clone) = clone.filter(|_| refusal.code.as_str() == Code::PathNotFound.as_str()) else {
        return refusal;
    };
    let hint = refusal.hint.clone().unwrap_or_default();
    let hint = format!(
        "{hint}; the clone at {} may be behind its remote, so run `board -C {} sync --no-push` and try again",
        clone.display(),
        clone.display()
    );
    refusal.with_hint(hint)
}

/// `board::uri::read_only` when `uri`, given to `name`, a command that
/// writes, is a URI.
fn read_only((given, name): (&Given, &str)) -> Option<Refusal> {
    let uri = given.0.as_str();
    if !uri.starts_with(URI_SCHEME) {
        return None;
    }
    let message = format!("{name} writes, and {uri} is a URI, which Board only reads");
    let hint = format!(
        "{name} in the clone of the board the URI names, with -C and the record's path, such as `board -C <dir> {name} /tasks/a`"
    );
    Some(plain(Code::UriReadOnly, &message).with_hint(hint))
}
