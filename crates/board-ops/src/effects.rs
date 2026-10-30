//! Effects: what only the owner may write, and the pending effects and
//! answers the personal board holds (docs/architecture/writes/effects.md).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use board_model::{
    Answer, Code, EFFECT_TYPE, EFFECT_TYPES, Effect, Fields, LogicalPath, MAX_DECIDE_BYTES,
    MAX_RECORD_BYTES, ObjectId, PUT_OP, PathError, PluginName, RM_OP, Refusal, SEEN_TYPE, Seen,
    canonical_json, parse_json,
};
use board_tree::{GitError, Removes, Target, Working, WriteError};
use serde_json::Value;

use crate::board::{Board, OpenError};
use crate::put::{Expect, PutError};
use crate::verdict::Admitted;

/// Who asks, and where their pending effects and answers go.
#[derive(Clone, Debug, Default)]
pub struct Asker {
    /// `BOARD_PLUGIN`: the plugin a Board runs inside, if any.
    pub plugin: Option<PluginName>,
    /// The personal board's root, if the known boards file names one. With
    /// none, a plugin's decide is refused.
    pub personal: Option<PathBuf>,
    /// What a mount's `~/` means in the personal board.
    pub home: Option<PathBuf>,
    /// How an approval is signed and checked.
    pub signing: Signing,
    /// Whether the owner's own put under `effects/` needs their
    /// signature (docs/architecture/writes/effects.md). `board put` sets
    /// it; Board's own writes of pending effects and answers do not.
    pub sign_owner_writes: bool,
}

/// How the owner signs an approval, and the keys a signature must verify
/// against (docs/architecture/writes/signing.md).
#[derive(Clone, Debug, Default)]
pub struct Signing {
    /// The program run as `<signer> approve <statement-file>`, which shows
    /// the statement and prints an SSH signature after Touch ID.
    pub signer: Option<OsString>,
    /// The allowed signers file that lists the owner's key.
    pub allowed: Option<PathBuf>,
    /// The cache, where the statement file is written.
    pub cache: Option<PathBuf>,
    /// All the signer sees of the environment.
    pub environment: Vec<(&'static str, OsString)>,
}

/// The directories of pending effects and answers in the personal board.
const PENDING: &str = "effects/pending";
const SEEN: &str = "effects/seen";
/// The directory that holds them, in any board.
const EFFECTS: &str = "effects";

/// A request refused by the effects check, or one it could not finish.
#[derive(Debug, thiserror::Error)]
pub enum EffectError {
    #[error("plugin {plugin} may not {op} {path} until the owner answers pending effect {id}")]
    Asked {
        plugin: PluginName,
        op: String,
        path: LogicalPath,
        id: String,
    },
    #[error("plugin {plugin} may not {op} {path}: the owner refused pending effect {id}")]
    Refused {
        plugin: PluginName,
        op: String,
        path: LogicalPath,
        id: String,
    },
    #[error("plugin {plugin} may not {what}; only the owner may, outside a plugin")]
    FromPlugin {
        plugin: PluginName,
        path: Option<LogicalPath>,
        what: String,
    },
    #[error(
        "{path} lands at a file with no logical path, so no pending effect can name it: {source}"
    )]
    Unlanded {
        path: LogicalPath,
        source: PathError,
    },
    #[error("{what}, but the known boards file names no personal board")]
    NoPersonal { what: String },
    #[error("the record at {path} in the personal board is over {MAX_RECORD_BYTES} bytes")]
    TooLarge { path: String },
    #[error("{why}")]
    Unshown { code: Code, why: String },
    #[error("{source}")]
    Resolve { source: Box<WriteError> },
    #[error("no pending effect {id} is in the personal board")]
    NotFound { id: String },
    #[error("the record at /{PENDING}/{id} is not the pending effect {id} names: {why}")]
    Forged { id: String, why: String },
    #[error("pending effect {id} has no good approval signature: {why}")]
    Unsigned { id: String, why: String },
    #[error(
        "the owner's {op} of {path} changes an effect or the board's config and has no good signature: {why}"
    )]
    OwnerUnsigned {
        op: &'static str,
        path: String,
        why: String,
    },
    #[error("the records in {} cannot be read: {source}", directory)]
    Read { directory: String, source: GitError },
    #[error("the personal board cannot be opened: {source}")]
    Open { source: OpenError },
    #[error("the personal board refused a write: {source}")]
    Write { source: Box<PutError> },
}

impl EffectError {
    pub fn code(&self) -> Code {
        match self {
            Self::Asked { .. } => Code::SettingsAsked,
            Self::Refused { .. } => Code::SettingsRefused,
            Self::FromPlugin { .. } => Code::SettingsFromPlugin,
            Self::NoPersonal { .. } => Code::SettingsNoPersonal,
            Self::Unlanded { source, .. } => source.code(),
            Self::TooLarge { .. } => Code::JsonTooLarge,
            Self::Unshown { code, .. } => *code,
            Self::Resolve { source } => source.code(),
            Self::NotFound { .. } | Self::Forged { .. } => Code::SettingsNotFound,
            Self::Unsigned { .. } | Self::OwnerUnsigned { .. } => Code::SettingsUnsigned,
            Self::Read { source, .. } => source.code(),
            Self::Open { source } => source.code(),
            Self::Write { source } => source.code(),
        }
    }

    pub fn fields(&self) -> Fields {
        match self {
            Self::Asked {
                plugin, path, id, ..
            }
            | Self::Refused {
                plugin, path, id, ..
            } => Fields {
                plugin: Some(plugin.clone()),
                effect: Some(id.clone()),
                ..Fields::at(path)
            },
            Self::FromPlugin { plugin, path, .. } => Fields {
                plugin: Some(plugin.clone()),
                path: path.clone(),
                ..Fields::default()
            },
            Self::Unlanded { path, .. } => Fields::at(path),
            Self::NotFound { id } | Self::Forged { id, .. } | Self::Unsigned { id, .. } => Fields {
                effect: Some(id.clone()),
                ..Fields::default()
            },
            Self::OwnerUnsigned { path, .. } => Fields {
                path: path.parse().ok(),
                ..Fields::default()
            },
            Self::Open { source } => source.fields(),
            Self::Write { source } => source.fields(),
            Self::Resolve { source } => source.fields(),
            Self::Read { .. }
            | Self::NoPersonal { .. }
            | Self::TooLarge { .. }
            | Self::Unshown { .. } => Fields::default(),
        }
    }

    pub fn refusal(&self) -> Refusal {
        let refusal = Refusal::new(self.code(), self, &self.fields());
        let shown = [Code::JsonTooLarge, Code::RecordNotARecord].contains(&self.code());
        match self {
            Self::Unshown { .. } if shown => refusal.with_hint(format!("a decide shows the owner a JSON object, or UTF-8 text, of at most {MAX_DECIDE_BYTES} bytes; give the content the op will write")),
            _ => refusal,
        }
    }
}

/// The record at `relative` in `board`'s working tree, if it reads as JSON
/// of type `record_type`.
fn read_record(
    board: &Board,
    relative: &str,
    record_type: &str,
) -> Result<Option<Value>, EffectError> {
    Ok(read_held(board, relative, record_type)?.map(|(value, _)| value))
}

/// [`read_record`], with the blob the file holds.
fn read_held(
    board: &Board,
    relative: &str,
    record_type: &str,
) -> Result<Option<(Value, ObjectId)>, EffectError> {
    let read = board.repository().read_working(format!("{relative}.json"));
    let failed = |source| EffectError::Read {
        directory: relative.to_owned(),
        source,
    };
    Ok(match read.map_err(failed)? {
        Working::Read(contents) => parse_json(&contents.bytes)
            .ok()
            .filter(|value| value.get("$type").and_then(Value::as_str) == Some(record_type))
            .map(|value| (value, contents.blob)),
        Working::Missing => None,
        Working::TooLarge => {
            return Err(EffectError::TooLarge {
                path: format!("/{relative}"),
            });
        }
    })
}

impl Asker {
    /// The personal board, opened, which `what` needs.
    pub(crate) fn personal(&self, what: impl FnOnce() -> String) -> Result<Board, EffectError> {
        let root =
            (self.personal.as_ref()).ok_or_else(|| EffectError::NoPersonal { what: what() })?;
        // It reads and writes pending effects and answers, which no index
        // decision holds, so no build decides for it.
        Board::open(root, self.home.clone(), "").map_err(|source| EffectError::Open { source })
    }
}

/// The owner's answer to `effect`, if they gave one. An approval counts
/// only with the owner's signature of it, checked against `allowed`.
fn seen(
    personal: &Board,
    effect: &Effect,
    allowed: Option<&Path>,
) -> Result<Option<Answer>, EffectError> {
    let id = effect.id();
    let value = read_record(personal, &format!("{SEEN}/{id}"), SEEN_TYPE)?;
    let Some(seen) = value.and_then(|value| Seen::from_value(&value).ok()) else {
        return Ok(None);
    };
    if seen.answer == Answer::Approve {
        let signature = seen.signature.as_deref().unwrap_or_default();
        verified(effect, allowed, signature)?;
    }
    Ok(Some(seen.answer))
}

/// Checks that `signature` is the owner's signature of `effect`'s
/// approval, by a key the allowed signers file lists.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module is the one caller of the verifier"
)]
fn verified(effect: &Effect, allowed: Option<&Path>, signature: &str) -> Result<(), EffectError> {
    let unsigned = |why: String| EffectError::Unsigned {
        id: effect.id(),
        why,
    };
    if signature.is_empty() {
        return Err(unsigned("the answer holds no signature".to_owned()));
    }
    let allowed = allowed.ok_or_else(|| unsigned("no allowed signers file is named".to_owned()))?;
    board_plugin::verify(allowed, &effect.approval(), signature)
        .map_err(|error| unsigned(error.to_string()))
}

/// Writes `effect` at its id in the personal board, unless an effect is
/// already there or the owner has answered it, which they may have done
/// since the answer was read.
fn hold(personal: &Board, effect: &Effect) -> Result<String, EffectError> {
    let id = effect.id();
    let answered = read_record(personal, &format!("{SEEN}/{id}"), SEEN_TYPE)?.is_some();
    if !answered && read_record(personal, &format!("{PENDING}/{id}"), EFFECT_TYPE)?.is_none() {
        let path: LogicalPath = format!("/{PENDING}/{id}")
            .parse()
            .expect("an effect id is 64 hex digits, so the path is well formed");
        let value = serde_json::to_value(effect).expect("an effect serializes");
        personal
            .put(&path, Expect::Any, canonical_json(&value).as_bytes())
            .map_err(|source| EffectError::Write {
                source: Box::new(source),
            })?;
    }
    Ok(id)
}

/// What a checked put writes: a record, a file put with `--file`, or a
/// schema a batch line adds, which `prepare_type` has already refused if it
/// changes one the board holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shape {
    Record,
    File,
    Schema,
}

/// What a checked write does to the file it lands in: puts `canonical`,
/// admitted as `admitted` in `shape`, or removes the file.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Change<'a> {
    Put(&'a [u8], &'a Admitted, Shape),
    Remove(Removes),
}

impl Change<'_> {
    const fn op(self) -> &'static str {
        match self {
            Self::Put(..) => PUT_OP,
            Self::Remove { .. } => RM_OP,
        }
    }

    /// The shape of the file at `on_disk`: a removed file is a record when
    /// its name is one.
    fn shape(self, on_disk: &str) -> Shape {
        match self {
            Self::Put(_, _, shape) => shape,
            Self::Remove { .. } if board_model::is_json_file(on_disk) => Shape::Record,
            Self::Remove { .. } => Shape::File,
        }
    }

    const fn verb(self) -> &'static str {
        match self {
            Self::Put(..) => "write",
            Self::Remove { .. } => "remove",
        }
    }
}

/// What a plugin's put or removal would do that only the owner does,
/// given the path it was given, the file it lands in and its logical path:
/// change a path that is not ASCII, change where [`protected`] says, put or
/// remove a link or mount, or put an effect or seen record in any board.
fn owners(
    (given, on_disk, path): (&str, &str, &LogicalPath),
    change: Change<'_>,
) -> Option<String> {
    let verb = change.verb();
    if !given.is_ascii() {
        return Some(format!("{verb} /{given}, whose path is not ASCII"));
    }
    if let Some(area) = protected(on_disk, change.shape(on_disk)) {
        return Some(format!("{verb} {path}, in {area}"));
    }
    let (admitted, shape) = match change {
        Change::Put(_, admitted, shape) => (admitted, shape),
        Change::Remove(removes) => {
            return (removes == Removes::Pointer)
                .then(|| format!("remove the link or mount {path}"));
        }
    };
    if admitted.pointer {
        return Some(format!("put the link or mount {path}"));
    }
    let kind = admitted.value.get("$type").and_then(Value::as_str);
    kind.filter(|kind| shape == Shape::Record && EFFECT_TYPES.contains(kind))
        .map(|kind| format!("put {path}, a {kind} record"))
}

/// The logical path of the file at `on_disk`, a working-tree path, put as
/// `shape`: a record's file without its `.json`.
fn landed(on_disk: &str, shape: Shape) -> String {
    let record = on_disk
        .strip_suffix(".json")
        .filter(|_| shape != Shape::File);
    format!("/{}", record.unwrap_or(on_disk))
}

/// Whether the owner's put of the file given as `given`, which lands at
/// `on_disk`, both working-tree paths, changes the pending effects or
/// answers, and so needs their signature: a file under `effects/`, in any
/// case, or one whose first directory is not ASCII, which a file system may
/// fold to `effects`. It holds in every board, because which board is the
/// personal one is named by a file the caller chooses.
fn owner_signs(given: &str, on_disk: &str) -> bool {
    [given, on_disk].into_iter().any(|file| {
        let file = file.to_ascii_lowercase();
        let mut parts = file.split('/');
        let first = parts.next().unwrap_or_default();
        (first == EFFECTS || !first.is_ascii()) && parts.next().is_some()
    })
}

/// Whether `change`, to the file given as `given` that lands at `on_disk`,
/// removes the board's config, `.board/board.json`, in any case. Without
/// it the board no longer opens.
fn removes_config(change: &Change<'_>, given: &str, on_disk: &str) -> bool {
    matches!(change, Change::Remove(_))
        && [given, on_disk]
            .iter()
            .any(|file| file.eq_ignore_ascii_case(".board/board.json"))
}

/// Where only the owner writes, when the file at `file`, a working-tree
/// path, lies there: `.board/` in any board, which holds its config,
/// types and plugins, apart from a new schema a batch line adds to
/// `.board/types/`, and `effects/` in any board, as [`owner_signs`] matches
/// it, because which board is the personal one is named by a file the caller
/// chooses. `None` elsewhere.
fn protected(file: &str, shape: Shape) -> Option<&'static str> {
    let lower = file.to_ascii_lowercase();
    let under = |directory: &str| lower.starts_with(&format!("{directory}/"));
    if under(".board") && !(shape == Shape::Schema && under(".board/types")) {
        Some(".board")
    } else if owner_signs(file, file) {
        Some("effects, which holds the owner's pending effects and answers")
    } else {
        None
    }
}

impl Board {
    /// Who asks, when a plugin does.
    pub fn ask_as(&mut self, asker: Asker) {
        self.asker = asker;
    }

    /// Checks `change`, a put or removal of `target`, the file it lands in.
    /// Under `BOARD_PLUGIN` it is refused where only the owner writes, which
    /// is decided on disk, so a path spelled in another case reaches the file
    /// it writes. Without a plugin the change is made, once the owner signs
    /// it if [`owner_signs`].
    pub(crate) fn gate(&self, target: &Target, change: Change<'_>) -> Result<(), EffectError> {
        let Some(plugin) = &self.asker.plugin else {
            return self.owner_put(target, change);
        };
        let on_disk = self.repository().landing(&target.path);
        let landed = landed(&on_disk, change.shape(&on_disk));
        let Ok(path) = landed.parse::<LogicalPath>() else {
            return Err(EffectError::FromPlugin {
                plugin: plugin.clone(),
                path: None,
                what: format!("{} {landed}, which is not a logical path", change.verb()),
            });
        };
        let landing = (target.path.as_str(), on_disk.as_str(), &path);
        match owners(landing, change) {
            Some(what) => Err(EffectError::FromPlugin {
                plugin: plugin.clone(),
                path: Some(path),
                what,
            }),
            None => Ok(()),
        }
    }

    /// Checks the owner's own `change` to `target`. One that
    /// [`owner_signs`] or [`removes_config`] needs the owner to sign
    /// [`Effect::owner_write`] now,
    /// with Touch ID, while the put holds the lock; Board keeps no
    /// signature of it.
    #[expect(
        clippy::disallowed_methods,
        reason = "board-ops's effects module builds the owner-write statement it signs"
    )]
    fn owner_put(&self, target: &Target, change: Change<'_>) -> Result<(), EffectError> {
        if !self.asker.sign_owner_writes {
            return Ok(());
        }
        let on_disk = self.repository().landing(&target.path);
        if !owner_signs(&target.path, &on_disk) && !removes_config(&change, &target.path, &on_disk)
        {
            return Ok(());
        }
        let landed = landed(&on_disk, change.shape(&on_disk));
        let unsigned = |why: String| EffectError::OwnerUnsigned {
            op: change.op(),
            path: landed.clone(),
            why,
        };
        let (mut record, mut text) = (None, None);
        if let Change::Put(canonical, admitted, shape) = change {
            if shape == Shape::File {
                text = Some(std::str::from_utf8(canonical).map_err(|_| {
                    unsigned("its bytes are not UTF-8, so no statement can show them".to_owned())
                })?);
            } else {
                record = Some(&admitted.value);
            }
        }
        let root = root(self);
        let statement = Effect::owner_write(&root, change.op(), &landed, record, text);
        decide::signature(&self.asker.signing, Path::new(&root), &statement)
            .map(|_| ())
            .map_err(unsigned)
    }
}

/// The root of a board, as an effect names it: its canonical directory.
fn root(board: &Board) -> String {
    board.repository().path().display().to_string()
}

mod decide;

pub use decide::{AnswerReport, DecideReport, Shown, answer};

#[cfg(test)]
mod tests;
