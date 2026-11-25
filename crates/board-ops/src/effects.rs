//! Effects: what only the owner may write, and the pending effects and
//! answers the personal board holds (docs/architecture/writes/effects.md).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use board_model::{
    Act, Answer, Code, EFFECT_TYPE, EFFECT_TYPES, Effect, Fields, LogicalPath, MAX_DECIDE_BYTES,
    MAX_RECORD_BYTES, Need, ObjectId, PUT_OP, PathError, PluginName, RM_OP, Refusal, SEEN_TYPE,
    Seen, Signers, SigningSetting, canonical_json, parse_json,
};
use board_plugin::Listed;
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
    /// The program run as `<signer> approve <statement-file>`, or `<signer>
    /// sign <statement-file>` for an order, which shows the statement and
    /// prints an SSH signature after Touch ID.
    pub signer: Option<OsString>,
    /// The allowed signers file that lists the owner's key for approvals,
    /// owner's writes and orders.
    pub allowed: Option<PathBuf>,
    /// The attention allowed signers file, which an order is checked
    /// against when Board's own file does not admit its key, for one more
    /// release.
    pub attention_allowed: Option<PathBuf>,
    /// The cache, where the statement file is written.
    pub cache: Option<PathBuf>,
    /// All the signer sees of the environment.
    pub environment: Vec<(&'static str, OsString)>,
}

impl Signing {
    /// Signing's state with `setting`, the personal board's: on when the
    /// allowed signers file has an `owner` line. A file Board does not trust
    /// reads as on, so the acts it lists are refused as unsigned.
    #[expect(
        clippy::disallowed_methods,
        reason = "board-ops's effects module is the one reader of the allowed signers file"
    )]
    pub fn state(&self, setting: SigningSetting) -> board_model::Signing {
        let signers = match self.allowed.as_deref().map(board_plugin::owner_listed) {
            None | Some(Ok(Listed::Missing)) => Signers::Missing,
            Some(Ok(Listed::NoOwner)) => Signers::NoOwner,
            Some(Ok(Listed::Owner) | Err(_)) => Signers::Owner,
        };
        board_model::Signing::new(signers, setting)
    }

    /// The allowed signers file as a refusal names it.
    fn file(&self) -> String {
        (self.allowed.as_ref()).map_or_else(
            || "Board's allowed signers file".to_owned(),
            |file| file.display().to_string(),
        )
    }
}

/// The directories of pending effects, answers and answered effects in the
/// personal board.
const PENDING: &str = "effects/pending";
const SEEN: &str = "effects/seen";
const ANSWERED: &str = "effects/answered";
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
    #[error(
        "{what} needs the owner's signature and {file} is missing; Board has verified a signature before, so it does not take an answer instead"
    )]
    FileMissing { what: String, file: String },
    #[error("{what} is not approved: {why}")]
    NotApproved { what: String, why: String },
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
            Self::FileMissing { .. } => Code::SigningFileMissing,
            Self::NotApproved { .. } => Code::SigningNotApproved,
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
            | Self::FileMissing { .. }
            | Self::NotApproved { .. }
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
        Board::open(root, self.home.clone()).map_err(|source| EffectError::Open { source })
    }

    /// Signing's state with the personal board's setting, or the default
    /// setting when the known boards file names no personal board.
    pub(crate) fn signing_now(&self) -> Result<board_model::Signing, EffectError> {
        let setting = match &self.personal {
            Some(_) => self.personal(String::new)?.config().signing.clone(),
            None => SigningSetting::default(),
        };
        Ok(self.signing.state(setting))
    }
}

/// The personal board's config with `signing.configured` set, and the
/// blob it replaces, which records that Board has verified one of the
/// owner's signatures, so that from now on a missing allowed signers file
/// refuses the acts signing lists. None when it is set already or the
/// config cannot be read.
pub(crate) fn configured(personal: &Board) -> Option<(ObjectId, Value)> {
    let (blob, mut value, config) = crate::board::config_file(personal.repositories()).ok()?;
    if config.signing.configured {
        return None;
    }
    let signing = (value.as_object_mut()?)
        .entry("signing")
        .or_insert_with(|| serde_json::json!({}));
    signing
        .as_object_mut()?
        .insert("configured".to_owned(), Value::Bool(true));
    Some((blob, value))
}

/// The logical path of a board's config.
pub(crate) fn config_path() -> LogicalPath {
    "/.board/board"
        .parse()
        .expect("the config's path is a logical path")
}

/// Writes [`configured`] in its own commit. A failure leaves signing on,
/// and the next verified signature writes it.
pub(crate) fn mark_configured(personal: &Board) {
    if let Some((blob, value)) = configured(personal) {
        let bytes = canonical_json(&value);
        let _ = personal.put(&config_path(), Expect::Blob(blob), bytes.as_bytes());
    }
}

/// Whether the answer at `approved`, a logical path in `personal`, approves
/// the act on `target`, as [`board_model::approving`] says. `Err` says why
/// not.
pub(crate) fn approved_by(personal: &Board, approved: &str, target: &str) -> Result<(), String> {
    let relative = |path: &str| path.trim_start_matches('/').to_owned();
    let read = |path: &str, kind: &str| {
        read_record(personal, &relative(path), kind).map_err(|error| error.to_string())
    };
    let answer = read(approved, "plans/answer/v1")?
        .ok_or_else(|| format!("no plans/answer/v1 is at {approved} in the personal board"))?;
    let question = (answer.get("question").and_then(Value::as_str))
        .map(|question| read(question, "plans/question/v1"))
        .transpose()?
        .flatten();
    board_model::approving(&answer, question.as_ref(), target)
        .map(|_| ())
        .map_err(|why| format!("the answer at {approved} does not approve it: {why}"))
}

/// The owner's answer to `effect`, if they gave one. While signing lists
/// effects, an approval counts only with the owner's signature of it,
/// checked against the allowed signers file. Otherwise a signature or the
/// answer `approved` names counts.
fn seen(
    personal: &Board,
    effect: &Effect,
    signing: &Signing,
) -> Result<Option<Answer>, EffectError> {
    let id = effect.id();
    let value = read_record(personal, &format!("{SEEN}/{id}"), SEEN_TYPE)?;
    let Some(seen) = value.and_then(|value| Seen::from_value(&value).ok()) else {
        return Ok(None);
    };
    if seen.answer == Answer::Approve {
        let signature = seen.signature.as_deref().unwrap_or_default();
        let now = signing.state(personal.config().signing.clone());
        match (now.need(Act::Effect), &seen.approved) {
            (Need::FileMissing, _) => return Err(file_missing(signing, &id)),
            (Need::Answer, Some(approved)) if signature.is_empty() => {
                approved_by(personal, approved, &id).map_err(|why| EffectError::NotApproved {
                    what: format!("pending effect {id}"),
                    why,
                })?;
            }
            _ => verified(effect, signing.allowed.as_deref(), signature)?,
        }
    }
    Ok(Some(seen.answer))
}

/// The refusal of an approval of the pending effect `id` while signing
/// lists effects and the allowed signers file is gone.
fn file_missing(signing: &Signing, id: &str) -> EffectError {
    EffectError::FileMissing {
        what: format!("approving pending effect {id}"),
        file: signing.file(),
    }
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
    /// [`owner_signs`] or [`removes_config`] is an owner's write, and one
    /// that changes `signing` in a board's config is an edit of signing.
    /// Each needs what signing's state says: an owner's write needs the
    /// owner to sign [`Effect::owner_write`] now, with Touch ID, while the
    /// put holds the lock, when signing lists it; an edit of signing needs
    /// it whenever signing is on. Board keeps no signature of it.
    #[expect(
        clippy::disallowed_methods,
        reason = "board-ops's effects module builds the owner-write statement it signs"
    )]
    fn owner_put(&self, target: &Target, change: Change<'_>) -> Result<(), EffectError> {
        if !self.asker.sign_owner_writes {
            return Ok(());
        }
        let on_disk = self.repository().landing(&target.path);
        let write =
            owner_signs(&target.path, &on_disk) || removes_config(&change, &target.path, &on_disk);
        let edit = self.edits_signing(&change, &target.path, &on_disk);
        if !write && !edit {
            return Ok(());
        }
        let landed = landed(&on_disk, change.shape(&on_disk));
        let now = self.asker.signing_now()?;
        let need = [
            write.then(|| now.need(Act::OwnerWrite)),
            edit.then(|| now.need_to_edit()),
        ]
        .into_iter()
        .flatten()
        .max_by_key(|need| match need {
            Need::Answer => 0,
            Need::Signature => 1,
            Need::FileMissing => 2,
        });
        match need {
            Some(Need::Signature) => {}
            Some(Need::FileMissing) => {
                return Err(EffectError::FileMissing {
                    what: format!("the owner's {} of {landed}", change.op()),
                    file: self.asker.signing.file(),
                });
            }
            Some(Need::Answer) | None => return Ok(()),
        }
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

    /// Whether `change`, to the file given as `given` that lands at
    /// `on_disk`, changes `signing` in this board's config: a put whose
    /// `signing` differs from the one the working tree holds, or the
    /// config's removal while it holds one.
    fn edits_signing(&self, change: &Change<'_>, given: &str, on_disk: &str) -> bool {
        let config = [given, on_disk]
            .iter()
            .any(|file| file.eq_ignore_ascii_case(".board/board.json"));
        if !config {
            return false;
        }
        let current = crate::board::config_file(self.repositories())
            .ok()
            .and_then(|(_, value, _)| value.get("signing").cloned());
        match change {
            Change::Put(_, admitted, _) => admitted.value.get("signing") != current.as_ref(),
            Change::Remove(_) => current.is_some(),
        }
    }
}

/// The root of a board, as an effect names it: its canonical directory.
fn root(board: &Board) -> String {
    board.repository().path().display().to_string()
}

mod decide;
mod order;
mod write;

pub use decide::{AnswerReport, DecideReport, Shown, answer};
pub use order::{SignOrderError, VerifyOrderError};
pub(crate) use order::{legacy, order_signers, sign_order, verify_order};
pub use write::OwnerWriteError;
pub(crate) use write::{sign_owner_write, verify_owner_write};

#[cfg(test)]
mod tests;
