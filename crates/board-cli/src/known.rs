//! The user's known boards, listed in `$XDG_CONFIG_HOME/board/config.json`.
//! When no board holds the directory Board started in, Board asks each one
//! for a mount whose repository holds it. `--personal` runs as the personal
//! board the file names (docs/architecture.md, Boards and entries).

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use board_model::{Code, Fields, Refusal};
use board_ops::{Board, OpenError};
use serde::Deserialize;

use crate::config::Context;

/// The file's shape: `{"boards": ["/Users/me/Code/board"], "personal":
/// "/Users/me/Code/board"}`. A relative path is read against the file's
/// directory.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Known {
    #[serde(default)]
    boards: Vec<PathBuf>,
    personal: Option<PathBuf>,
    /// When the person's day starts, such as `"07:00"`, which a plugin
    /// receives as `BOARD_DAY_START`.
    #[serde(default, deserialize_with = "hh_mm")]
    day_start: Option<String>,
}

/// A `day_start` of the form `HH:MM`, on a 24-hour clock; `null` is unset.
fn hh_mm<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let Some(text) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let part = |digits: &str, below: u8| {
        digits.len() == 2
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && digits.parse::<u8>().is_ok_and(|value| value < below)
    };
    match text.split_once(':') {
        Some((hours, minutes)) if part(hours, 24) && part(minutes, 60) => Ok(Some(text)),
        _ => Err(serde::de::Error::custom(format!(
            "day_start {text:?} is not HH:MM, such as \"07:00\""
        ))),
    }
}

/// When the person's day starts, from the known boards file, if it says.
pub(crate) fn day_start(context: &Context) -> Result<Option<String>, Box<Refusal>> {
    Ok(read(context.boards.as_deref())?.day_start)
}

/// The root of the personal board the known boards file names, refused
/// unless a board's root is there.
pub(crate) fn personal(context: &Context) -> Result<PathBuf, Box<Refusal>> {
    let file = context.boards.as_deref();
    let shown = shown(file);
    let message = match read(file)?.personal.map(|root| against(file, &root)) {
        None => format!("--personal was given and `{shown}` names no personal board"),
        Some(root) => {
            let canonical = root.canonicalize().ok();
            let found = match Board::open(&root, context.home.clone()) {
                Ok(board) => board.repository().path().canonicalize().ok(),
                Err(OpenError::NotFound { .. } | OpenError::NoRepository { .. }) => None,
                Err(error) => {
                    let refusal = Refusal::new(error.code(), &error, &Fields::default());
                    return Err(Box::new(refusal));
                }
            };
            if canonical.is_some() && found == canonical {
                return Ok(root);
            }
            let named = root.display();
            format!("`personal` in `{shown}` names {named}, which is not the root of a board")
        }
    };
    let hint = format!(
        "name the personal board in `{shown}` as {{\"personal\": \"<board directory>\"}}, the directory that holds .board/board.json"
    );
    let refusal = Refusal::new(Code::ConfigNotFound, message, &Fields::default());
    Err(Box::new(refusal.with_hint(hint)))
}

/// `root` read against the directory of `file`, the known boards file.
fn against(file: Option<&Path>, root: &Path) -> PathBuf {
    file.and_then(Path::parent).unwrap_or(root).join(root)
}

/// How a message names the known boards file.
fn shown(file: Option<&Path>) -> String {
    file.map_or("$XDG_CONFIG_HOME/board/config.json".into(), |file| {
        file.display().to_string()
    })
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
    let listed = read(file)?.boards;
    let shown = shown(file);
    let (mut seen, mut skipped, mut named, mut chosen) = (HashSet::new(), vec![], vec![], None);
    for root in listed.iter().map(|root| against(file, root)) {
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

/// What the known boards `file` names: nothing when there is no file, or it
/// is missing or empty.
fn read(file: Option<&Path>) -> Result<Known, Box<Refusal>> {
    let Some(file) = file else {
        return Ok(Known::default());
    };
    let parsed = match std::fs::read(file) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Known::default()),
        Ok(bytes) if bytes.trim_ascii().is_empty() => return Ok(Known::default()),
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    parsed.map_err(|reason| {
        let message = format!("{} is not a list of known boards: {reason}", file.display());
        let hint = format!(
            "write `{}` as {{\"boards\": [\"<board directory>\"], \"personal\": \"<board directory>\"}}, either key optional, or remove it",
            file.display()
        );
        Box::new(Refusal::new(Code::ConfigInvalid, message, &Fields::default()).with_hint(hint))
    })
}
