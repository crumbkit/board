//! The user's known boards, which an adapter reads from
//! `$XDG_CONFIG_HOME/board/config.json`. When no board holds the directory
//! Board started in, Board asks each one for a mount whose repository holds
//! it. `--personal` runs as the personal board the file names
//! (docs/architecture.md, Boards and entries). Hints name `board` commands.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use board_model::{Code, Fields, LogicalPath, PathError, PluginName, Refusal};
use serde::Deserialize;

use crate::{Asker, Board, OpenError, Signing};

/// Where the run started, as a logical path in its board, or the refusal of
/// that path when it is not one.
pub type Prefix = Result<LogicalPath, PathError>;

/// The file's shape: `{"boards": ["/Users/me/Code/board"], "personal":
/// "/Users/me/Code/board"}`. A relative path is read against the file's
/// directory.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKnown {
    #[serde(default)]
    boards: Vec<PathBuf>,
    personal: Option<PathBuf>,
    /// When the person's day starts, such as `"07:00"`, which a plugin
    /// receives as `BOARD_DAY_START`.
    #[serde(default, deserialize_with = "hh_mm")]
    day_start: Option<String>,
}

/// What a known boards file names, and the file.
#[derive(Clone, Debug, Default)]
pub struct KnownBoards {
    file: Option<PathBuf>,
    known: RawKnown,
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

impl KnownBoards {
    /// What the known boards `file` names, given `read`, what reading it
    /// gave: nothing when it is missing or empty.
    pub fn parse(file: &Path, read: io::Result<Vec<u8>>) -> Result<Self, Box<Refusal>> {
        let parsed = match read {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(RawKnown::default()),
            Ok(bytes) if bytes.trim_ascii().is_empty() => Ok(RawKnown::default()),
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        let known = parsed.map_err(|reason| {
            let message = format!("{} is not a list of known boards: {reason}", file.display());
            let hint = format!(
                "write `{}` as {{\"boards\": [\"<board directory>\"], \"personal\": \"<board directory>\"}}, either key optional, or remove it",
                file.display()
            );
            Box::new(Refusal::new(Code::ConfigInvalid, message, &Fields::default()).with_hint(hint))
        })?;
        let file = Some(file.to_path_buf());
        Ok(Self { file, known })
    }

    /// When the person's day starts, if the file says.
    pub fn day_start(&self) -> Option<&str> {
        self.known.day_start.as_deref()
    }

    /// The root of the personal board the file names, refused unless a
    /// board's root is there. `home` is what a mount's `~/` means.
    pub fn personal(&self, home: Option<&Path>) -> Result<PathBuf, Box<Refusal>> {
        let unnamed = format!("`{}` names no personal board", shown(self.file.as_deref()));
        self.named(home)?.ok_or_else(|| self.no_personal(unnamed))
    }

    /// The root of the personal board the file names, if it names one,
    /// refused unless a board's root is there.
    fn named(&self, home: Option<&Path>) -> Result<Option<PathBuf>, Box<Refusal>> {
        let file = self.file.as_deref();
        let Some(root) = self.known.personal.as_ref().map(|root| against(file, root)) else {
            return Ok(None);
        };
        let canonical = root.canonicalize().ok();
        let found = match Board::open(&root, home.map(Path::to_path_buf)) {
            Ok(board) => Some(board.repository().path().to_path_buf()),
            Err(OpenError::NotFound { .. } | OpenError::NoRepository { .. }) => None,
            Err(error) => {
                let refusal = Refusal::new(error.code(), &error, &Fields::default());
                return Err(Box::new(refusal));
            }
        };
        if canonical.is_some() && found == canonical {
            return Ok(Some(root));
        }
        let message = format!(
            "`personal` in `{}` names {}, which is not the root of a board",
            shown(file),
            root.display()
        );
        Err(self.no_personal(message))
    }

    fn no_personal(&self, message: String) -> Box<Refusal> {
        let shown = shown(self.file.as_deref());
        let hint = format!(
            "name the personal board in `{shown}` as {{\"personal\": \"<board directory>\"}}, the directory that holds .board/board.json"
        );
        let refusal = Refusal::new(Code::ConfigNotFound, message, &Fields::default());
        Box::new(refusal.with_hint(hint))
    }
}

/// Who asks for an effect: `plugin`, and the personal board `personal`
/// names when the run writes there, refused unless a board's root is there.
/// `Some(known)` means this run writes to the personal board; a put passes `None`.
pub fn asker(
    plugin: Option<PluginName>,
    personal: Option<&KnownBoards>,
    home: Option<PathBuf>,
    signing: Signing,
) -> Result<Asker, Box<Refusal>> {
    let personal = personal.map(|known| known.named(home.as_deref()));
    Ok(Asker {
        plugin,
        personal: personal.transpose()?.flatten(),
        home,
        signing,
        sign_owner_writes: true,
    })
}

/// The board holding `start`, or else the one known board with a mount
/// whose repository holds `start`, and `start`'s logical path in that
/// board, if it has one. `known` reads the known boards, only when no board
/// holds `start` and `start` is in a git repository, since outside one no
/// mount can hold it.
pub fn open(
    start: &Path,
    home: Option<&Path>,
    cache: Option<&Path>,
    known: impl FnOnce() -> Result<KnownBoards, Box<Refusal>>,
) -> Result<(Board, Option<Prefix>), Box<Refusal>> {
    let error = match Board::open(start, home.map(Path::to_path_buf)) {
        Ok(board) => return Ok((board, board_tree::within(start))),
        Err(error) => error,
    };
    let outside = matches!(error, OpenError::NoRepository { .. });
    let refusal = unopened(error.code(), &error, &error.fields(), outside);
    if !matches!(error, OpenError::NotFound { .. }) {
        return Err(Box::new(refusal));
    }
    find(&known()?, start, home, cache, refusal).map(|(board, prefix)| (board, Some(prefix)))
}

/// The refusal when a board cannot be opened or made. `outside` a git
/// repository, `board init` fails too, so the hint names only what can run.
pub fn unopened(
    code: Code,
    error: &dyn std::fmt::Display,
    fields: &Fields,
    outside: bool,
) -> Refusal {
    let refusal = Refusal::new(code, error, fields);
    if outside {
        refusal.with_hint(
            "run `git init` first to make the directory a repository; for a board elsewhere, run `board -C <dir> check`",
        )
    } else {
        refusal
    }
}

/// `start` after entering each of `directories` in turn, as `git -C` does,
/// or the refusal of the first that is not a directory.
pub fn entered(start: &Path, directories: &[OsString]) -> Result<PathBuf, Box<Refusal>> {
    let mut start = start.to_path_buf();
    for directory in directories {
        let joined = start.join(directory);
        let fault = match std::fs::canonicalize(&joined) {
            Ok(path) if path.is_dir() => {
                start = path;
                continue;
            }
            Ok(_) => "is not a directory".to_owned(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                "does not exist".to_owned()
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => {
                "is not a directory".to_owned()
            }
            Err(error) => format!("cannot be read: {error}"),
        };
        let message = format!(
            "cannot change to '{}': {} {fault}",
            Path::new(directory).display(),
            joined.display()
        );
        let hint = "give -C a directory that exists, such as the board's root or a directory \
                    inside it; a relative -C is taken from the one before it";
        return Err(Box::new(
            Refusal::new(Code::CliUsage, message, &Fields::default()).with_hint(hint),
        ));
    }
    Ok(start)
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
fn find(
    known: &KnownBoards,
    start: &Path,
    home: Option<&Path>,
    cache: Option<&Path>,
    lost: Refusal,
) -> Result<(Board, Prefix), Box<Refusal>> {
    let file = known.file.as_deref();
    let listed = &known.known.boards;
    let shown = shown(file);
    let (mut seen, mut skipped, mut named, mut chosen) = (HashSet::new(), vec![], vec![], None);
    for root in listed.iter().map(|root| against(file, root)) {
        let opened = Board::open(&root, home.map(Path::to_path_buf));
        let found = opened.map(|board| (board.places(start, cache), board));
        let Ok((Ok(places), board)) = found else {
            skipped.push(root.display().to_string());
            continue;
        };
        if !seen.insert(board.repository().path().to_path_buf()) {
            continue;
        }
        let at = |place: &board_tree::Place| match place.prefix() {
            Ok(prefix) => format!("{} at {prefix}", root.display()),
            Err(error) => format!("{} at a mount where {error}", root.display()),
        };
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

#[cfg(test)]
mod tests;
