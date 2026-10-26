//! Effects: the authority a plugin has for a put or another op, from the
//! preferences in the personal board and in the written board's
//! `.board/settings/`, and the pending effects and answers the personal
//! board holds (docs/architecture/writes/effects.md).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use board_model::{
    Answer, Authority, Code, Digest, EFFECT_TYPE, EFFECT_TYPES, Effect, Fields, Layer, LogicalPath,
    MAX_RECORD_BYTES, ObjectId, PREFERENCE_TYPE, PUT_OP, Preference, Refusal, Request, Rule,
    SEEN_TYPE, Seen, canonical_json, parse_json,
};
use board_tree::{GitError, Repository, Target, Working, WriteError};
use serde_json::Value;

use crate::board::{Board, OpenError};
use crate::put::{Expect, PutError, PutReport};
use crate::verdict::Admitted;

/// Who asks, and where their pending effects and answers go.
#[derive(Clone, Debug, Default)]
pub struct Asker {
    /// `BOARD_PLUGIN`: the plugin a Board runs inside, if any.
    pub plugin: Option<String>,
    /// The personal board's root, if the known boards file names one. With
    /// none there are no owner rules, and an ask or propose is refused.
    pub personal: Option<PathBuf>,
    /// What a mount's `~/` means in the personal board.
    pub home: Option<PathBuf>,
    /// `put --or-propose`: a put whose authority is propose writes a pending
    /// effect in place of the record.
    pub or_propose: bool,
    /// How an approval is signed and checked.
    pub signing: Signing,
    /// Whether the owner's own put to a rule or an effect needs their
    /// signature (docs/architecture/writes/effects.md). `board put` sets
    /// it; Board's own writes of pending effects and answers do not.
    pub sign_owner_writes: bool,
}

/// How the owner signs an approval, and the keys a signature must verify
/// against (docs/architecture/writes/effects.md, Signed approvals).
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

/// The directory of a board's own rules.
const SETTINGS: &str = ".board/settings";
/// The directory of the owner's rules in the personal board.
const PREFERENCES: &str = "preferences";
/// The directories of pending effects and answers in the personal board.
const PENDING: &str = "effects/pending";
const SEEN: &str = "effects/seen";

/// What the effects check reads once per put or batch in one board: the
/// personal board, whether it is the board written, the rules, and the
/// first file among them that does not read as a preference.
#[derive(Debug)]
pub(crate) struct Loaded {
    personal: Option<Board>,
    rules: Vec<Rule>,
    broken: Option<String>,
}

/// A request refused by the effects check, or one it could not finish.
#[derive(Debug, thiserror::Error)]
pub enum EffectError {
    #[error("plugin {plugin} may not put {path} until the owner answers pending effect {id}")]
    Asked {
        plugin: String,
        path: LogicalPath,
        id: String,
    },
    #[error("plugin {plugin} may not put {path}: {}", match (rule, effect) {
        (Some(rule), _) => format!("the preference at {rule} refuses it"),
        (None, Some(id)) => format!("the owner refused pending effect {id}"),
        (None, None) => "the owner refused it".to_owned(),
    })]
    Refused {
        plugin: String,
        path: LogicalPath,
        rule: Option<String>,
        effect: Option<String>,
    },
    #[error("plugin {plugin} may only propose a put to {path}")]
    ProposeOnly { plugin: String, path: LogicalPath },
    /// The put wrote pending effect `id` in place of the record; the put
    /// reports it and refuses nothing.
    #[error("the put proposed pending effect {id}")]
    Proposed { id: String },
    #[error("plugin {plugin} may not {what}; only the owner may, outside a plugin")]
    FromPlugin {
        plugin: String,
        path: Option<LogicalPath>,
        what: String,
    },
    #[error("{what}, but the known boards file names no personal board")]
    NoPersonal { what: String },
    #[error("plugin {plugin} may not put {path} until the owner fixes the rule file {file}")]
    BadRule {
        plugin: String,
        path: LogicalPath,
        file: String,
    },
    #[error("plugin {plugin} may not put {path}: a pending effect cannot hold it, because {why}")]
    CannotHold {
        plugin: String,
        path: LogicalPath,
        why: String,
    },
    #[error("pending effect {id} is a put, and a saved rule cannot approve a put")]
    CannotWiden { id: String },
    #[error("the record at {path} in the personal board is over {MAX_RECORD_BYTES} bytes")]
    TooLarge { path: String },
    #[error("{source}")]
    Resolve { source: Box<WriteError> },
    #[error("no pending effect {id} is in the personal board")]
    NotFound { id: String },
    #[error("the record at /{PENDING}/{id} is not the pending effect {id} names: {why}")]
    Forged { id: String, why: String },
    #[error("pending effect {id} is for {path}, which a saved rule cannot name exactly: {why}")]
    BadPattern {
        id: String,
        path: String,
        why: String,
    },
    #[error("pending effect {id} has no good approval signature: {why}")]
    Unsigned { id: String, why: String },
    #[error(
        "the owner's put of {path} changes a rule or an effect and has no good signature: {why}"
    )]
    OwnerUnsigned { path: String, why: String },
    #[error("the rules in {} cannot be read: {source}", directory)]
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
            Self::ProposeOnly { .. } | Self::Proposed { .. } => Code::SettingsProposeOnly,
            Self::FromPlugin { .. } => Code::SettingsFromPlugin,
            Self::NoPersonal { .. } => Code::SettingsNoPersonal,
            Self::BadRule { .. } => Code::SettingsBadRule,
            Self::CannotHold { .. } => Code::SettingsCannotHold,
            Self::CannotWiden { .. } => Code::SettingsCannotWiden,
            Self::TooLarge { .. } => Code::JsonTooLarge,
            Self::Resolve { source } => source.code(),
            Self::NotFound { .. } | Self::Forged { .. } => Code::SettingsNotFound,
            Self::BadPattern { .. } => Code::SettingsBadPattern,
            Self::Unsigned { .. } | Self::OwnerUnsigned { .. } => Code::SettingsUnsigned,
            Self::Read { source, .. } => source.code(),
            Self::Open { source } => source.code(),
            Self::Write { source } => source.code(),
        }
    }

    pub fn fields(&self) -> Fields {
        match self {
            Self::Asked { plugin, path, id }
            | Self::Refused {
                plugin,
                path,
                effect: Some(id),
                ..
            } => Fields {
                plugin: Some(plugin.clone()),
                effect: Some(id.clone()),
                ..Fields::at(path)
            },
            Self::Refused { plugin, path, .. }
            | Self::ProposeOnly { plugin, path }
            | Self::BadRule { plugin, path, .. }
            | Self::CannotHold { plugin, path, .. } => Fields {
                plugin: Some(plugin.clone()),
                ..Fields::at(path)
            },
            Self::FromPlugin { plugin, path, .. } => Fields {
                plugin: Some(plugin.clone()),
                path: path.clone(),
                ..Fields::default()
            },
            Self::NotFound { id }
            | Self::Forged { id, .. }
            | Self::BadPattern { id, .. }
            | Self::CannotWiden { id }
            | Self::Unsigned { id, .. }
            | Self::Proposed { id } => Fields {
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
            Self::Read { .. } | Self::NoPersonal { .. } | Self::TooLarge { .. } => {
                Fields::default()
            }
        }
    }

    pub fn refusal(&self) -> Refusal {
        Refusal::new(self.code(), self, &self.fields())
    }
}

/// The preferences in the `.json` files beneath `directory` of the working
/// tree, read as they are, following no pointer, with a `board` named by
/// its canonical directory. A file that does not read as a preference is
/// no rule, and the first is added to `broken`.
fn rules_in(
    repository: &Repository,
    directory: &str,
    layer: Layer,
    broken: &mut Option<String>,
) -> Result<Vec<Rule>, EffectError> {
    let failed = |source| EffectError::Read {
        directory: directory.to_owned(),
        source,
    };
    let entries = repository
        .walk_under(&board_model::Source::WorkingTree, directory)
        .map_err(failed)?;
    let mut rules = Vec::new();
    for entry in entries {
        let Some(at) = entry.path.strip_suffix(".json") else {
            continue;
        };
        let read = match repository.read_working(&entry.path).map_err(failed)? {
            Working::Read(contents) => parse_json(&contents.bytes).ok(),
            Working::Missing | Working::TooLarge => None,
        };
        let preference = read
            .filter(|value| value.get("$type").and_then(Value::as_str) == Some(PREFERENCE_TYPE))
            .and_then(|value| Preference::from_value(&value).ok());
        let Some(mut preference) = preference else {
            let file = repository.path().join(&entry.path);
            broken.get_or_insert_with(|| file.display().to_string());
            continue;
        };
        if let Some(board) = &mut preference.board
            && let Ok(canonical) = std::path::Path::new(board.as_str()).canonicalize()
        {
            *board = canonical.display().to_string();
        }
        rules.push(Rule {
            layer,
            at: format!("/{at}"),
            preference,
        });
    }
    Ok(rules)
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
    /// The personal board, opened, if one is named.
    fn owner(&self) -> Result<Option<Board>, EffectError> {
        let open = |root: &PathBuf| Board::open(root, self.home.clone());
        let opened = self.personal.as_ref().map(open).transpose();
        opened.map_err(|source| EffectError::Open { source })
    }

    /// The personal board, opened, which `what` needs.
    pub(crate) fn personal(&self, what: impl FnOnce() -> String) -> Result<Board, EffectError> {
        self.owner()?
            .ok_or_else(|| EffectError::NoPersonal { what: what() })
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
        verified(effect, seen.rule.is_some(), allowed, signature)?;
    }
    Ok(Some(seen.answer))
}

/// Checks that `signature` is the owner's signature of `effect`'s
/// approval, by a key the allowed signers file lists.
fn verified(
    effect: &Effect,
    saves_rule: bool,
    allowed: Option<&Path>,
    signature: &str,
) -> Result<(), EffectError> {
    let unsigned = |why: String| EffectError::Unsigned {
        id: effect.id(),
        why,
    };
    if signature.is_empty() {
        return Err(unsigned("the answer holds no signature".to_owned()));
    }
    let allowed = allowed.ok_or_else(|| unsigned("no allowed signers file is named".to_owned()))?;
    board_plugin::verify(allowed, &effect.approval(saves_rule), signature)
        .map_err(|error| unsigned(error.to_string()))
}

/// Writes `effect` at its id in the personal board, unless an effect is
/// already there or the owner has answered it, which they may have done
/// since the put read the answer.
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

/// `effect` with the content it would write, which is `canonical` text
/// for a file and `admitted` for a record; refused when the content is not
/// UTF-8 or the effect would be too large to keep as a record.
fn contain(
    effect: &mut Effect,
    canonical: &[u8],
    (admitted, shape): (&Admitted, Shape),
    path: &LogicalPath,
) -> Result<(), EffectError> {
    let cannot_hold = |why: String| EffectError::CannotHold {
        plugin: effect.plugin.clone(),
        path: path.clone(),
        why,
    };
    if shape == Shape::File {
        let Ok(text) = std::str::from_utf8(canonical) else {
            return Err(cannot_hold("it is not UTF-8".to_owned()));
        };
        effect.text = Some(text.to_owned());
    } else {
        effect.record = Some(admitted.value.clone());
    }
    let held = serde_json::to_value(&*effect).expect("an effect serializes");
    if canonical_json(&held).as_bytes().len() > MAX_RECORD_BYTES {
        return Err(cannot_hold(format!(
            "the effect holding it would be over {MAX_RECORD_BYTES} bytes"
        )));
    }
    Ok(())
}

/// What a plugin's put would do that only the owner does, given the path
/// it was given, the file it lands in and its logical path: write a path
/// that is not ASCII, write where [`protected`] says, put a link or mount,
/// or put a preference, effect or seen record in any board.
fn owners(
    (given, on_disk, path): (&str, &str, &LogicalPath),
    (admitted, shape): (&Admitted, Shape),
) -> Option<String> {
    if !given.is_ascii() {
        return Some(format!("write /{given}, whose path is not ASCII"));
    }
    if let Some(area) = protected(on_disk, shape) {
        return Some(format!("write {path}, in {area}"));
    }
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
/// `on_disk`, both working-tree paths, changes what the effects check
/// decides, and so needs their signature: a file under `.board/settings/`,
/// `preferences/` or `effects/`, in any case, or one whose first directory,
/// or second under `.board/`, is not ASCII, which a file system may fold to
/// one of those. It holds in every board, because which board is the
/// personal one is named by a file the caller chooses.
fn owner_signs(given: &str, on_disk: &str) -> bool {
    [given, on_disk].into_iter().any(|file| {
        let file = file.to_ascii_lowercase();
        let mut parts = file.split('/');
        let first = parts.next().unwrap_or_default();
        let second = parts.next();
        match first {
            _ if !first.is_ascii() => second.is_some(),
            ".board" => second.is_some_and(|second| second == "settings" || !second.is_ascii()),
            PREFERENCES | "effects" => second.is_some(),
            _ => false,
        }
    })
}

/// Where only the owner writes, when the file at `file`, a working-tree
/// path, lies there: `.board/` in any board, which holds its settings,
/// types and plugins, apart from a new schema a batch line adds to
/// `.board/types/`, and `preferences/` and `effects/` in any board, as
/// [`owner_signs`] matches them, because which board is the personal one is
/// named by a file the caller chooses. `None` elsewhere.
fn protected(file: &str, shape: Shape) -> Option<&'static str> {
    let lower = file.to_ascii_lowercase();
    let under = |directory: &str| lower.starts_with(&format!("{directory}/"));
    if under(".board") && !(shape == Shape::Schema && under(".board/types")) {
        Some(".board")
    } else if owner_signs(file, file) {
        Some("preferences or effects, which hold the owner's rules and answers")
    } else {
        None
    }
}

impl Board {
    /// Who asks, when a plugin does.
    pub fn ask_as(&mut self, asker: Asker) {
        self.asker = asker;
    }

    /// The personal board and the rules for a request in this board: the
    /// owner's, then the board's.
    pub(crate) fn load(&self) -> Result<Loaded, EffectError> {
        let personal = self.asker.owner()?;
        let mut broken = None;
        let mut rules = match &personal {
            Some(personal) => rules_in(
                personal.repository(),
                PREFERENCES,
                Layer::Owner,
                &mut broken,
            )?,
            None => Vec::new(),
        };
        rules.extend(rules_in(
            self.repository(),
            SETTINGS,
            Layer::Board,
            &mut broken,
        )?);
        Ok(Loaded {
            personal,
            rules,
            broken,
        })
    }

    /// Checks a put under `BOARD_PLUGIN` of `canonical`, admitted as
    /// `admitted`, to `target`, the file it lands in, against the rules
    /// `loaded` holds, read once. Where it lands is decided on disk, so a
    /// path spelled in another case reaches the rules of the file it
    /// writes. An ask or propose gathers its pending effect for
    /// [`Board::holding`] to write once the lock is released. Without a
    /// plugin the put writes, once the owner signs it if [`owner_signs`].
    pub(crate) fn gate(
        &self,
        loaded: &mut Option<Loaded>,
        target: &Target,
        canonical: &[u8],
        (admitted, shape): (&Admitted, Shape),
    ) -> Result<(), EffectError> {
        let Some(plugin) = &self.asker.plugin else {
            return self.owner_put(target, canonical, (admitted, shape));
        };
        let on_disk = self.repository().landing(&target.path);
        let landed = landed(&on_disk, shape);
        let Ok(path) = landed.parse::<LogicalPath>() else {
            return Err(EffectError::FromPlugin {
                plugin: plugin.clone(),
                path: None,
                what: format!("write {landed}, which is not a logical path"),
            });
        };
        let loaded = match loaded {
            Some(loaded) => loaded,
            None => loaded.insert(self.load()?),
        };
        let landing = (target.path.as_str(), on_disk.as_str(), &path);
        if let Some(what) = owners(landing, (admitted, shape)) {
            return Err(EffectError::FromPlugin {
                plugin: plugin.clone(),
                path: Some(path),
                what,
            });
        }
        if let Some(file) = &loaded.broken {
            return Err(EffectError::BadRule {
                plugin: plugin.clone(),
                path,
                file: file.clone(),
            });
        }
        let board = root(self);
        let request = Request {
            op: PUT_OP,
            path: &landed,
            plugin: Some(plugin),
            board: &board,
        };
        let decision = board_model::decide(&loaded.rules, &request);
        let refused = |rule, effect| EffectError::Refused {
            plugin: plugin.clone(),
            path: path.clone(),
            rule,
            effect,
        };
        match decision.authority {
            Authority::Act => return Ok(()),
            Authority::Refuse => {
                let rule = decision.deciding.map(|at| decision.matching[at].at.clone());
                return Err(refused(rule, None));
            }
            Authority::Ask | Authority::Propose => {}
        }
        let personal = loaded
            .personal
            .as_ref()
            .ok_or_else(|| EffectError::NoPersonal {
                what: format!("put {path} needs a pending effect in the personal board"),
            })?;
        let mut effect = Effect::new(board, plugin.clone(), PUT_OP.to_owned(), landed.clone());
        effect.authority = decision.authority;
        effect.digest = Some(Digest::of(canonical));
        effect.prior = self.prior(&target.path)?;
        contain(&mut effect, canonical, (admitted, shape), &path)?;
        let id = effect.id();
        match seen(personal, &effect, self.asker.signing.allowed.as_deref())? {
            Some(Answer::Approve) => return Ok(()),
            Some(Answer::Refuse) => return Err(refused(None, Some(id))),
            None => {}
        }
        if decision.authority == Authority::Propose && !self.asker.or_propose {
            return Err(EffectError::ProposeOnly {
                plugin: plugin.clone(),
                path,
            });
        }
        self.pending.borrow_mut().push(effect);
        Err(match decision.authority {
            Authority::Ask => EffectError::Asked {
                plugin: plugin.clone(),
                path,
                id,
            },
            _ => EffectError::Proposed { id },
        })
    }

    /// Checks the owner's own put of `canonical`, admitted as `admitted`,
    /// to `target`. One that [`owner_signs`] needs the owner to sign
    /// [`Effect::owner_write`] now, with Touch ID, while the put holds the
    /// lock; Board keeps no signature of it.
    fn owner_put(
        &self,
        target: &Target,
        canonical: &[u8],
        (admitted, shape): (&Admitted, Shape),
    ) -> Result<(), EffectError> {
        if !self.asker.sign_owner_writes {
            return Ok(());
        }
        let on_disk = self.repository().landing(&target.path);
        if !owner_signs(&target.path, &on_disk) {
            return Ok(());
        }
        let landed = landed(&on_disk, shape);
        let unsigned = |why: String| EffectError::OwnerUnsigned {
            path: landed.clone(),
            why,
        };
        let mut effect = Effect::new(root(self), String::new(), PUT_OP.to_owned(), landed.clone());
        effect.digest = Some(Digest::of(canonical));
        if shape == Shape::File {
            let text = std::str::from_utf8(canonical).map_err(|_| {
                unsigned("its bytes are not UTF-8, so no statement can show them".to_owned())
            })?;
            effect.text = Some(text.to_owned());
        } else {
            effect.record = Some(admitted.value.clone());
        }
        let root = root(self);
        decide::signature(&self.asker.signing, Path::new(&root), &effect.owner_write())
            .map(|_| ())
            .map_err(unsigned)
    }

    /// The blob in the working tree at `file`, in hex, or `None` for no file.
    fn prior(&self, file: &str) -> Result<Option<String>, EffectError> {
        let failed = |source| EffectError::Read {
            directory: file.to_owned(),
            source,
        };
        let repository = self.repository();
        Ok(match repository.read_working(file).map_err(failed)? {
            Working::Missing => None,
            Working::Read(contents) => Some(contents.blob.to_string()),
            Working::TooLarge => Some(repository.working_blob(file).map_err(failed)?.to_string()),
        })
    }

    /// `put`, once the pending effects its effects check gathered are
    /// written in the personal board, after the put released every lock. A
    /// put that proposed reports the effect it wrote.
    pub(crate) fn holding(&self, put: Result<PutReport, PutError>) -> Result<PutReport, PutError> {
        let pending = self.pending.take();
        if let Some(first) = pending.first() {
            let what = || {
                format!(
                    "put {} needs a pending effect in the personal board",
                    first.path
                )
            };
            let personal = self.asker.personal(what)?;
            for effect in &pending {
                hold(&personal, effect)?;
            }
        }
        match put {
            Err(PutError::Effect(error)) => match *error {
                EffectError::Proposed { id } => Ok(PutReport::proposed(id)),
                error => Err(error.into()),
            },
            put => put,
        }
    }
}

/// The root of a board, as an effect and a preference name it: its
/// canonical directory.
fn root(board: &Board) -> String {
    board.repository().path().display().to_string()
}

mod decide;

pub use decide::{AnswerReport, DecideReport, answer};

#[cfg(test)]
mod tests;
