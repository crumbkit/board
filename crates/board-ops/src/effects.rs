//! Effects: what only the owner may write, and the pending effects and
//! answers the personal board holds (docs/architecture/writes/effects.md).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use board_model::{
    Act, Answer, EFFECT_TYPE, EFFECT_TYPES, Edit, Effect, LogicalPath, Need, ObjectId, PUT_OP,
    PluginName, RM_OP, Refusal, SEEN_TYPE, Seen, Signers, SigningSetting, SigningState,
    canonical_json, parse_json,
};
use board_plugin::Listed;
use board_tree::{Removes, Target, Working};
use serde_json::Value;

use crate::board::Board;
use crate::put::Expect;
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
    /// `board put --approved`: the path of the owner's `plans/answer/v1`,
    /// in the board the put writes, that approves an edit of `signing`
    /// while signing is off.
    pub approved: Option<String>,
    /// Why the personal board the known boards file names cannot be
    /// read, kept to refuse a write that signing governs with.
    pub unopened: Option<Box<Refusal>>,
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
    /// Signing's state with `personal`'s setting, or the default setting
    /// with no personal board: on when the allowed signers file or the
    /// attention one has an `owner` line. A file Board does not trust reads
    /// as on, so the acts it lists are refused as unsigned, and one whose
    /// presence Board cannot tell refuses them as missing.
    pub fn state(&self, personal: Option<&Board>) -> board_model::Signing {
        let setting = personal.map_or_else(SigningSetting::default, |personal| {
            personal.config().signing.clone()
        });
        self.read(setting, personal)
    }

    /// Signing's state with `setting`, where `holder` is the board whose
    /// signed records keep signing configured.
    fn read(&self, setting: SigningSetting, holder: Option<&Board>) -> board_model::Signing {
        let signers = match listed(self.allowed.as_deref()) {
            Signers::Missing | Signers::NoOwner
                if listed(self.attention_allowed.as_deref()) == Signers::Owner =>
            {
                Signers::Owner
            }
            signers => signers,
        };
        // A board that holds a signed record matters only when no owner
        // line is there, so only then is it read.
        let held = signers != Signers::Owner
            && setting.configured.is_none()
            && holder.is_some_and(holds_signature);
        board_model::Signing::new(signers, setting, held)
    }

    /// The allowed signers file as a refusal names it.
    fn file(&self) -> String {
        (self.allowed.as_ref()).map_or_else(
            || "Board's allowed signers file".to_owned(),
            |file| file.display().to_string(),
        )
    }
}

/// What the allowed signers file `file` says of the owner.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module is the one reader of the allowed signers file"
)]
fn listed(file: Option<&Path>) -> Signers {
    match file.map(board_plugin::owner_listed) {
        None | Some(Ok(Listed::Missing)) => Signers::Missing,
        Some(Ok(Listed::NoOwner)) => Signers::NoOwner,
        Some(Ok(Listed::Unreadable)) => Signers::Unreadable,
        Some(Ok(Listed::Owner) | Err(_)) => Signers::Owner,
    }
}

/// The directories of pending effects, answers and answered effects in the
/// personal board.
const PENDING: &str = "effects/pending";
const SEEN: &str = "effects/seen";
const ANSWERED: &str = "effects/answered";
/// The directory that holds them, in any board.
const EFFECTS: &str = "effects";

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

    /// Signing's state with the personal board's setting for a write to
    /// `written`. When the known boards file names no personal board, it
    /// is the default setting, and a signed record in `written` keeps
    /// signing configured, so a caller who points the known boards file
    /// elsewhere does not read the personal board's signed records as gone.
    /// A personal board that is named but cannot be read is refused here,
    /// so only a write that signing governs is refused for it.
    pub(crate) fn signing_now(&self, written: &Board) -> Result<board_model::Signing, EffectError> {
        if let Some(refusal) = &self.unopened {
            return Err(EffectError::Personal {
                refusal: refusal.clone(),
            });
        }
        Ok(match &self.personal {
            Some(_) => self.signing.state(Some(&self.personal(String::new)?)),
            None => (self.signing).read(SigningSetting::default(), Some(written)),
        })
    }
}

/// Whether `personal` holds a signed record: an answer under
/// `effects/seen/` or an order under `orders/` with a signature. A board
/// that does has had signing configured, so a missing allowed signers file
/// refuses the acts signing lists. A directory Board cannot list reads as
/// holding one.
fn holds_signature(personal: &Board) -> bool {
    let signed = |directory: &str, record_type: &str| {
        let Ok(names) = (personal.repository()).names(&board_model::Source::WorkingTree, directory)
        else {
            return true;
        };
        (names.iter().filter(|name| !name.directory)).any(|name| {
            let at = format!("{directory}/{}", name.segment);
            let record = read_record(personal, &at, record_type);
            record.map_or(true, |record| {
                record.is_some_and(|record| {
                    (record.get("signature").and_then(Value::as_str))
                        .is_some_and(|signature| !signature.is_empty())
                })
            })
        })
    };
    signed(SEEN, SEEN_TYPE) || signed(ORDERS, board_model::ORDER_TYPE)
}

/// The directory of orders in the personal board.
const ORDERS: &str = "orders";

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
        let now = signing.state(Some(personal));
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
        if self.unsigns(&change, &on_disk)
            && self.asker.signing_now(self)?.need_to_edit(Edit::Require) != Need::Answer
        {
            return Err(EffectError::FromPlugin {
                plugin: plugin.clone(),
                path: Some(path),
                what: format!(
                    "{} {landed}, a signed record that keeps signing configured",
                    change.verb()
                ),
            });
        }
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
        // Removing or unsigning a signed record could turn signing off, so
        // it needs what an edit of signing needs.
        let setting = self.edits_signing(&change, &target.path, &on_disk);
        let edit = (setting.as_ref().map(|(edit, _)| *edit))
            .or_else(|| self.unsigns(&change, &on_disk).then_some(Edit::Require));
        if !write && edit.is_none() {
            return Ok(());
        }
        let landed = landed(&on_disk, change.shape(&on_disk));
        let now = self.asker.signing_now(self)?;
        let need = [
            write.then(|| now.need(Act::OwnerWrite)),
            edit.map(|edit| now.need_to_edit(edit)),
        ]
        .into_iter()
        .flatten()
        .max_by_key(|need| match need {
            Need::Answer => 0,
            Need::Signature => 1,
            Need::FileMissing => 2,
        });
        let unsigned = |why: String| EffectError::OwnerUnsigned {
            op: change.op(),
            path: landed.clone(),
            why,
        };
        match need {
            // Only an edit of `configured` needs a signature with signing
            // off, and no signature can be checked then.
            Some(Need::Signature) if now.state == SigningState::Off => {
                return Err(unsigned(
                    "signing is off, so no signature can be checked, and an edit of \
                     `signing.configured` needs one; the owner adds their key to the allowed \
                     signers file first"
                        .to_owned(),
                ));
            }
            Some(Need::Signature) => {}
            Some(Need::FileMissing) => {
                return Err(EffectError::FileMissing {
                    what: format!("the owner's {} of {landed}", change.op()),
                    file: self.asker.signing.file(),
                });
            }
            Some(Need::Answer) => {
                if let Some((Edit::Require, setting)) = &setting {
                    self.approves_edit(setting.as_ref(), &landed)?;
                }
                return Ok(());
            }
            None => return Ok(()),
        }
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

    /// Whether `change` removes the signed record at `on_disk`, a seen
    /// record or an order that [`holds_signature`] counts, or puts one
    /// without a signature in its place. A record Board cannot read counts
    /// as signed.
    fn unsigns(&self, change: &Change<'_>, on_disk: &str) -> bool {
        let lower = on_disk.to_ascii_lowercase();
        let counted = [SEEN, ORDERS]
            .iter()
            .any(|directory| lower.starts_with(&format!("{directory}/")));
        if !counted || !board_model::is_json_file(on_disk) {
            return false;
        }
        let signed = |value: &Value| {
            (value.get("signature").and_then(Value::as_str)).is_some_and(|text| !text.is_empty())
        };
        let held = match self.repository().read_working(on_disk) {
            Ok(Working::Read(contents)) => parse_json(&contents.bytes)
                .ok()
                .is_none_or(|value| signed(&value)),
            Ok(Working::Missing) => false,
            Ok(Working::TooLarge) | Err(_) => true,
        };
        held && match change {
            Change::Put(_, admitted, _) => !signed(&admitted.value),
            Change::Remove(_) => true,
        }
    }

    /// The edit `change`, to the file given as `given` that lands at
    /// `on_disk`, makes of `signing` in this board's config, and the
    /// setting it leaves: a put whose `signing` differs from the one the
    /// working tree holds, or the config's removal while it holds one. It
    /// edits `configured` when that field differs, and `require` otherwise.
    fn edits_signing(
        &self,
        change: &Change<'_>,
        given: &str,
        on_disk: &str,
    ) -> Option<(Edit, Option<Value>)> {
        let config = [given, on_disk]
            .iter()
            .any(|file| file.eq_ignore_ascii_case(".board/board.json"));
        if !config {
            return None;
        }
        let current = crate::board::config_file(self.repositories())
            .ok()
            .and_then(|(_, value, _)| value.get("signing").cloned());
        let next = match change {
            Change::Put(_, admitted, _) => admitted.value.get("signing").cloned(),
            Change::Remove(_) => None,
        };
        if next == current {
            return None;
        }
        let configured = |setting: &Option<Value>| {
            (setting.as_ref()).and_then(|setting| setting.get("configured").cloned())
        };
        let edit = if configured(&next) == configured(&current) {
            Edit::Require
        } else {
            Edit::Configured
        };
        Some((edit, next))
    }

    /// Refuses unless the answer `board put --approved` names, in this
    /// board, approves the edit that leaves `signing` as `setting`: its
    /// question names [`board_model::setting_digest`] of it, and it was
    /// answered in the last [`board_model::PROMPT_DAYS`] days.
    fn approves_edit(&self, setting: Option<&Value>, landed: &str) -> Result<(), EffectError> {
        let digest = board_model::setting_digest(setting);
        let what = format!("the owner's edit of signing in {landed}");
        let Some(approved) = &self.asker.approved else {
            return Err(EffectError::NotApproved {
                what,
                why: format!(
                    "signing is off, so an edit of `signing.require` names the owner's answer \
                     with --approved, to a question that names {digest}"
                ),
            });
        };
        let not_approved = |why| EffectError::NotApproved {
            what: what.clone(),
            why,
        };
        approved_by(self, approved, &digest).map_err(not_approved)?;
        // Board keeps no record of the edits an answer has approved, so an
        // answer is not single-use; it must be recent instead.
        let answer = read_record(self, approved.trim_start_matches('/'), "plans/answer/v1")?;
        (answer.as_ref())
            .map_or(Ok(()), |answer| {
                board_model::recent(answer, SystemTime::now())
            })
            .map_err(|why| {
                not_approved(format!(
                    "the answer at {approved} does not approve it: {why}"
                ))
            })
    }
}

/// The root of a board, as an effect names it: its canonical directory.
fn root(board: &Board) -> String {
    board.repository().path().display().to_string()
}

mod decide;
mod error;
mod order;
mod write;

pub use decide::{AnswerReport, DecideReport, Shown, answer};
pub use error::EffectError;
pub use order::{SignOrderError, VerifyOrderError};
pub(crate) use order::{legacy, order_signers, sign_order, verify_order};
pub use write::OwnerWriteError;
pub(crate) use write::{sign_owner_write, verify_owner_write};

#[cfg(test)]
mod tests;
