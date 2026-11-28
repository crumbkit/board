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
use board_tree::{Checkout, Matched, Place};

use crate::checkout::Refused;
use serde::{Deserialize, Serialize};

use crate::{Asker, Board, OpenError, Signing};

mod personal;
mod uri;

pub use personal::Naming;
pub use uri::uri_of;

/// Where the run started, as a logical path in its board, or the refusal of
/// that path when it is not one.
pub type Prefix = Result<LogicalPath, PathError>;

/// The file's shape: `{"boards": ["/Users/me/Code/board"], "personal":
/// "/Users/me/Code/board", "day_start": "07:00", "agent": ["claude", "-p"],
/// "owner": "ana"}`,
/// every key optional. A relative path is read against the file's directory.
/// `board init --personal` writes it back in this order, leaving out what is unset.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawKnown {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    boards: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    personal: Option<PathBuf>,
    /// When the person's day starts, such as `"07:00"`, which a plugin
    /// receives as `BOARD_DAY_START`.
    #[serde(
        default,
        deserialize_with = "hh_mm",
        skip_serializing_if = "Option::is_none"
    )]
    day_start: Option<String>,
    /// The argv of the agent `changes.run` starts, such as `["claude",
    /// "-p"]`, which the `BOARD_AGENT` variable overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent: Option<Vec<String>>,
    /// The name of the person who owns the boards, such as `ana`, which
    /// `board search` reads as `from:me`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
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

    /// Each board the file lists, read against the file's directory.
    pub fn boards(&self) -> Vec<PathBuf> {
        let file = self.file.as_deref();
        (self.known.boards.iter())
            .map(|root| against(file, root))
            .collect()
    }

    /// When the person's day starts, if the file says.
    pub fn day_start(&self) -> Option<&str> {
        self.known.day_start.as_deref()
    }

    /// The name of the person who owns the boards, if the file names one
    /// that is not blank, without spaces around it.
    pub fn owner(&self) -> Option<&str> {
        (self.known.owner.as_deref())
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }

    /// The argv of the agent a plugin starts, if the file names one.
    pub fn agent(&self) -> Option<&[String]> {
        self.known.agent.as_deref()
    }

    /// The root of the personal board the file names, refused unless a
    /// board's root is there. `home` is what a mount's `~/` means.
    pub fn personal(&self, home: Option<&Path>) -> Result<PathBuf, Box<Refusal>> {
        let unnamed = format!("`{}` names no personal board", shown(self.file.as_deref()));
        self.named(home)?
            .ok_or_else(|| Box::new(self.no_personal(unnamed)))
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
        Err(Box::new(self.no_personal(message)))
    }

    fn no_personal(&self, message: String) -> Refusal {
        let shown = shown(self.file.as_deref());
        let hint = format!(
            "run `board init --personal` in the personal board's directory, or name it in `{shown}` as {{\"personal\": \"<board directory>\"}}, the directory that holds .board/board.json"
        );
        let refusal = Refusal::new(Code::ConfigNotFound, message, &Fields::default());
        refusal.with_hint(hint)
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

/// Which commit a run reads the mount holding its directory at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountsAt {
    /// The commit each pointer pins.
    Pins,
    /// The commit HEAD names in the directory's checkout (`board --checkout`).
    Checkout,
}

/// The board holding `start`, or else the one known board with a mount
/// whose repository holds `start`, or with a repo record whose `remote` is
/// that repository's origin, and `start`'s logical path in that board, if
/// it has one. `known` reads the known boards, only when no board
/// holds `start` and `start` is in a git repository, since outside one no
/// mount can hold it.
pub fn open(
    start: &Path,
    home: Option<&Path>,
    cache: Option<&Path>,
    known: impl FnOnce() -> Result<KnownBoards, Box<Refusal>>,
) -> Result<(Board, Option<Prefix>), Box<Refusal>> {
    open_at(start, home, cache, known, MountsAt::Pins, None)
}

/// [`open`], reading the mount holding `start` at `at`, through the board
/// whose root is `through` when one is named (`board --board`).
///
/// At [`MountsAt::Checkout`], the known board must mount the repository
/// holding `start`, linked worktrees included, and the run reads that mount
/// at the commit HEAD names in `start`'s working tree and writes nothing.
/// A board's own tree is read through the known board that mounts its
/// repository. A board's own tree no known board mounts, a mount only of
/// another clone or a repo record, and a HEAD with no commit are refused
/// with `board::cli::usage`.
///
/// `through` must be a board's root, or the run is refused with
/// `board::config::not_found`. It is then the only board read, so two
/// known boards that mount one repository are not ambiguous; without
/// `--checkout`, a board's own tree still opens as itself when `through`
/// names that board.
pub fn open_at(
    start: &Path,
    home: Option<&Path>,
    cache: Option<&Path>,
    known: impl FnOnce() -> Result<KnownBoards, Box<Refusal>>,
    at: MountsAt,
    through: Option<&Path>,
) -> Result<(Board, Option<Prefix>), Box<Refusal>> {
    let checkout = at == MountsAt::Checkout;
    // The chosen board's root as an opened board names it, and as given.
    let chosen =
        (through.map(|given| root_of(given, home).map(|root| (root, given)))).transpose()?;
    // `lost` is the refusal when no board holds `start`; `None` means a
    // board's own tree holds it.
    let lost = match Board::open(start, home.map(Path::to_path_buf)) {
        Ok(board)
            if !checkout
                && (chosen.as_ref()).is_none_or(|(root, _)| root == board.repository().path()) =>
        {
            return Ok((board, board_tree::within(start)));
        }
        Ok(_) => None,
        Err(error) => {
            let outside = matches!(error, OpenError::NoRepository { .. });
            let refusal = unopened(error.code(), &error, &error.fields(), outside);
            if !matches!(error, OpenError::NotFound { .. }) {
                return Err(Box::new(refusal));
            }
            Some(refusal)
        }
    };
    let found = if let Some((root, given)) = chosen {
        find(&[root], None, start, home, cache)?.map_err(|_| not_through(start, given))
    } else {
        let known = known()?;
        let file = known.file.as_deref();
        match (find(&known.boards(), file, start, home, cache)?, lost) {
            (Ok(found), _) => Ok(found),
            (Err(_), None) => return Err(Refused::OwnTree(start).refusal()),
            (Err(skipped), Some(lost)) => {
                let none = known.known.boards.is_empty();
                Err(unmounted(lost, &shown(file), none, &skipped))
            }
        }
    };
    let (mut board, place) = found.map_err(Box::new)?;
    if checkout {
        if place.matched != Matched::Mount {
            return Err(Refused::NotMounted(start).refusal());
        }
        let Some(commit) = board_tree::head_of(start) else {
            return Err(Refused::NoCommit(start).refusal());
        };
        let pointer = place.pointer.clone();
        board.read_checkout(Checkout { pointer, commit });
    } else if place.matched == Matched::Mount {
        board.started_from(place.mount.clone(), start.to_path_buf());
    }
    Ok((board, Some(place.prefix())))
}

/// The root of each known board, as `known` lists it, other than the board
/// whose root is `opened`, that has a mount or repo record for the
/// repository holding `start` and enables `plugin`. It writes nothing in
/// any board's tree and makes no commit, but, as opening through a mount does, it refreshes
/// the index of each listed board that enables `plugin`, kept in `cache`
/// when one is given. A board that cannot be opened, refreshed or read, or
/// that has two places for `start` of its best rank, is passed over, and
/// outside a git repository no board is read.
pub fn enabling(
    known: &KnownBoards,
    opened: &Path,
    start: &Path,
    plugin: &PluginName,
    home: Option<&Path>,
    cache: Option<&Path>,
) -> Vec<PathBuf> {
    if board_tree::repository_root(start).is_none() {
        return Vec::new();
    }
    // A board listed under several spellings counts once, and the opened
    // board never counts.
    let mut seen = HashSet::from([opened.to_path_buf()]);
    let enables = |root: &PathBuf, seen: &mut HashSet<PathBuf>| {
        let Ok(board) = Board::open(root, home.map(Path::to_path_buf)) else {
            return false;
        };
        if !seen.insert(board.repository().path().to_path_buf())
            || !board.config().plugins.contains(plugin)
        {
            return false;
        }
        let Ok(places) = board.places(start, cache) else {
            return false;
        };
        let best = places.iter().map(Place::rank).min();
        (places.iter())
            .filter(|place| Some(place.rank()) == best)
            .count()
            == 1
    };
    (known.boards().into_iter())
        .filter(|root| enables(root, &mut seen))
        .collect()
}

/// The root of the board at `root`, as an opened board names it, or the
/// refusal when `root` is not a board's root, which names `root` and
/// `--board`.
fn root_of(root: &Path, home: Option<&Path>) -> Result<PathBuf, Box<Refusal>> {
    let canonical = root.canonicalize().ok();
    let found = match Board::open(root, home.map(Path::to_path_buf)) {
        Ok(board) => Some(board.repository().path().to_path_buf()),
        Err(OpenError::NotFound { .. } | OpenError::NoRepository { .. }) => None,
        Err(error) => {
            let refusal = Refusal::new(error.code(), &error, &Fields::default());
            return Err(Box::new(refusal));
        }
    };
    match (canonical, found) {
        (Some(canonical), Some(found)) if canonical == found => Ok(found),
        _ => {
            let message = format!(
                "--board names {}, which is not the root of a board",
                lexical(root).display()
            );
            let hint = "give --board the root of a board, the directory that holds .board/board.json, such as one of the boards in the known boards file";
            let refusal = Refusal::new(Code::ConfigNotFound, message, &Fields::default());
            Err(Box::new(refusal.with_hint(hint)))
        }
    }
}

/// The refusal when `root`, the board `--board` names, has no mount or repo
/// record for the repository holding `start`.
fn not_through(start: &Path, root: &Path) -> Refusal {
    let message = format!(
        "{} is in a repository that {}, the board --board names, has no mount or repo record for",
        start.display(),
        root.display()
    );
    let hint = "name with --board a board that mounts this repository, or leave --board out to read through the known boards";
    Refusal::new(Code::ConfigNotFound, message, &Fields::default()).with_hint(hint)
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
/// or the refusal of the first that is not a directory. The refusal names
/// the directory as given, the directory a relative one was read from, and
/// the path it led to with each `..` taken out as text, and says so.
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
        let given = Path::new(directory);
        let from = if given.is_relative() {
            format!(" from {}", start.display())
        } else {
            String::new()
        };
        let dots = (given.components()).any(|part| part == std::path::Component::ParentDir);
        let text = if dots {
            "; Board took each .. out of that path as text, without following links"
        } else {
            ""
        };
        let message = format!(
            "cannot change to '{}'{from}: {} {fault}{text}",
            given.display(),
            lexical(&joined).display()
        );
        let hint = "give -C a directory that exists, such as the board's root or a directory \
                    inside it; a relative -C is taken from the one before it";
        return Err(Box::new(
            Refusal::new(Code::CliUsage, message, &Fields::default()).with_hint(hint),
        ));
    }
    Ok(start)
}

/// `path` with each `.` left out and each `..` taking out the name before
/// it, without reading the file system.
fn lexical(path: &Path) -> PathBuf {
    let mut kept = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if kept.file_name().is_some() => {
                kept.pop();
            }
            component => kept.push(component),
        }
    }
    kept
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

/// What [`find`] found among the boards it read: the one board with a
/// place for `start` and that place, or, when none has one, the boards that
/// could not be opened or refreshed.
type Found = Result<(Board, Place), Vec<String>>;

/// The one board of `roots` with a mount whose repository holds `start`, or
/// else a repo record for it, and where `start` is in it, or that none has
/// one. `file` is the known boards file that lists `roots`, if any. Two
/// places of the best rank are refused as ambiguous, and a board that is
/// there but cannot be opened is refused when no other board matches. A
/// board listed under several spellings counts once.
fn find(
    roots: &[PathBuf],
    file: Option<&Path>,
    start: &Path,
    home: Option<&Path>,
    cache: Option<&Path>,
) -> Result<Found, Box<Refusal>> {
    let shown = shown(file);
    let (mut seen, mut skipped, mut matches) = (HashSet::new(), vec![], vec![]);
    let mut unopenable = None;
    for root in roots {
        let board = match Board::open(root, home.map(Path::to_path_buf)) {
            Ok(board) => board,
            // A listed directory that is no board any more is passed over.
            Err(OpenError::NotFound { .. } | OpenError::NoRepository { .. }) => {
                skipped.push(root.display().to_string());
                continue;
            }
            // One that is a board this Board cannot open must not read as a
            // repository with no board, and so no controls: remember why, and
            // refuse with it only when no other board matches.
            Err(error) => {
                unopenable
                    .get_or_insert_with(|| Refusal::new(error.code(), &error, &Fields::default()));
                continue;
            }
        };
        let Ok(places) = board.places(start, cache) else {
            skipped.push(root.display().to_string());
            continue;
        };
        if !seen.insert(board.repository().path().to_path_buf()) {
            continue;
        }
        if !places.is_empty() {
            matches.push((root, board, places));
        }
    }
    // Across the known boards only the places of the smallest rank count, as
    // within one board.
    let best = (matches.iter())
        .flat_map(|(_, _, places)| places.iter().map(Place::rank))
        .min();
    let mut named = Vec::new();
    let mut boards = 0;
    let mut chosen = None;
    for (root, board, places) in matches {
        let places: Vec<Place> = (places.into_iter())
            .filter(|place| Some(place.rank()) == best)
            .collect();
        boards += usize::from(!places.is_empty());
        named.extend(places.iter().map(|place| match place.prefix() {
            Ok(prefix) => format!("{} at {prefix}", root.display()),
            Err(error) => format!("{} at a mount where {error}", root.display()),
        }));
        if let Some(place) = places.first() {
            chosen = Some((board, place.clone()));
        }
    }
    if named.len() > 1 {
        let message = format!(
            "{} is in a repository that the known boards hold more than one mount or repo record for: {}",
            start.display(),
            named.join(", ")
        );
        // `--board` chooses among boards, not among one board's mounts.
        let hint = if boards > 1 {
            format!(
                "choose the board to read through with `board --board <dir> <command>`, naming one of the boards listed, or leave one mount or repo record of this repository among the boards in `{shown}`"
            )
        } else {
            "leave one mount or repo record of this repository in the board listed".to_owned()
        };
        let refusal = Refusal::new(Code::ConfigAmbiguous, message, &Fields::default());
        return Err(Box::new(refusal.with_hint(hint)));
    }
    match (chosen, unopenable) {
        (Some(found), _) => Ok(Ok(found)),
        (None, Some(refusal)) => Err(Box::new(refusal)),
        (None, None) => Ok(Err(skipped)),
    }
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
        "{}; the known boards in `{shown}` were read and none has a mount or repo record for this repository{passed}",
        lost.message
    );
    Refusal { message, ..lost }.with_hint(format!(
        "put a `board/repo/v1` record whose `remote` is this repository's origin, as `owner/name` for GitHub, into one of the known boards with `board -C <board> put /repos/<name>`, or list in `{shown}` the board that has one"
    ))
}

#[cfg(test)]
mod tests;
