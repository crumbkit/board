//! The user's known boards, listed in `$XDG_CONFIG_HOME/board/config.json`.
//! When no board holds the directory Board started in, Board asks each one
//! for a mount whose repository holds it (docs/architecture.md, Boards and
//! entries).

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::Path;

use board_model::{Code, Fields, Refusal};
use board_ops::Board;
use serde::Deserialize;

use crate::config::Context;

/// The file's shape: `{"boards": ["/Users/me/Code/board"]}`. A relative
/// path is read against the file's directory.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Known {
    #[serde(default)]
    boards: Vec<std::path::PathBuf>,
}

/// The one known board with a mount whose repository holds `start`, and
/// `start`'s logical path in it. Otherwise `lost`, the refusal when no board
/// holds `start`, says which known boards were read and which were passed
/// over because they could not be opened or refreshed. A board listed under
/// several spellings counts once.
pub(crate) fn find(
    context: &Context,
    start: &Path,
    lost: Refusal,
) -> Result<(Board, String), Box<Refusal>> {
    let file = context.boards.as_deref();
    let listed = file.map(read).transpose()?.unwrap_or_default().boards;
    let shown = file.map_or("$XDG_CONFIG_HOME/board/config.json".into(), |file| {
        file.display().to_string()
    });
    let (mut seen, mut skipped, mut named, mut chosen) = (HashSet::new(), vec![], vec![], None);
    for root in listed
        .iter()
        .map(|root| file.and_then(Path::parent).unwrap_or(root).join(root))
    {
        let opened = Board::open(&root, context.home.clone());
        let found = opened.map(|board| (board.places(start, context.cache.as_deref()), board));
        let Ok((Ok(places), board)) = found else {
            skipped.push(root.display().to_string());
            continue;
        };
        let path = board.repository().path();
        if !seen.insert(path.canonicalize().unwrap_or_else(|_| path.to_path_buf())) {
            continue;
        }
        let at = |place: &board_tree::Place| format!("{} at {}", root.display(), place.prefix());
        named.extend(places.iter().map(at));
        if let Some(place) = places.first() {
            chosen = Some((board, place.prefix()));
        }
    }
    if named.len() > 1 {
        let message = format!(
            "{} is in a repository that the known boards mount more than once: {}",
            start.display(),
            named.join(", ")
        );
        let hint = format!(
            "choose a board with `board -C <dir> check`, naming one of the boards listed, or leave one mount of this repository among the boards in `{shown}`"
        );
        let refusal = Refusal::new(Code::ConfigAmbiguous, message, &Fields::default());
        return Err(Box::new(refusal.with_hint(hint)));
    }
    chosen.ok_or_else(|| Box::new(unmounted(lost, &shown, listed.is_empty(), &skipped)))
}

/// `lost`, saying that the known boards in `shown` were read and none mounts
/// the repository, or when `none` are listed, how to list one.
fn unmounted(lost: Refusal, shown: &str, none: bool, skipped: &[String]) -> Refusal {
    let hint = lost.hint.clone().unwrap_or_default();
    if none {
        return lost.with_hint(format!(
            "{hint}; to run from a repository a board mounts, list that board in `{shown}` as {{\"boards\": [\"<board directory>\"]}}"
        ));
    }
    let passed = match skipped {
        [] => String::new(),
        _ => format!(
            ", and these could not be opened or refreshed: {}",
            skipped.join(", ")
        ),
    };
    let message = format!(
        "{}; the known boards in `{shown}` were read and none mounts this repository{passed}",
        lost.message
    );
    Refusal { message, ..lost }.with_hint(format!(
        "put a `board/mount/v1` record whose `repo` is this repository into one of the known boards with `board -C <board> put <path>`, or list in `{shown}` the board that mounts it"
    ))
}

/// The known boards `file` lists: none when it is missing or empty.
fn read(file: &Path) -> Result<Known, Box<Refusal>> {
    let parsed = match std::fs::read(file) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Known::default()),
        Ok(bytes) if bytes.trim_ascii().is_empty() => return Ok(Known::default()),
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    parsed.map_err(|reason| {
        let message = format!("{} is not a list of known boards: {reason}", file.display());
        let hint = format!(
            "write `{}` as {{\"boards\": [\"<board directory>\"]}}, or remove it",
            file.display()
        );
        Box::new(Refusal::new(Code::ConfigInvalid, message, &Fields::default()).with_hint(hint))
    })
}
