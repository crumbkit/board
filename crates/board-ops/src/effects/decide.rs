//! `board decide` and `board answer`.

use board_model::{
    Answer, BINARY_NAME, Code, Effect, LogicalPath, MAX_DECIDE_BYTES, PathError, Seen,
    canonical_json, is_effect_id, parse_json, shown_digest, utc,
};
use serde_json::{Value, json};

use board_tree::{LOCK_WAIT, WriteError};

use std::path::Path;
use std::time::SystemTime;

use super::{
    ANSWERED, Asker, EFFECT_TYPE, EffectError, PENDING, SEEN, Signing, hold, read_held, root, seen,
    verified,
};
use crate::board::Board;
use crate::put::{Expect, Failure, PutError, PutReport, Removal, Schemas};

/// A plugin's op at a path that `board decide` admitted, and the pending
/// effect whose approval admitted it, if the op needed one.
#[derive(Debug)]
pub struct DecideReport {
    pub op: String,
    pub path: LogicalPath,
    pub effect: Option<String>,
}

/// What `board answer` wrote in the personal board.
#[derive(Debug)]
pub struct AnswerReport {
    pub id: String,
    pub answer: Answer,
    pub put: PutReport,
}

impl Board {
    /// Whether the plugin `BOARD_PLUGIN` names may run its own `op` at
    /// `path`, named as a put to `path` would land: after links, and through
    /// a mount in the mounted board. The owner's approval admits it, their
    /// refusal refuses it, and with no answer its pending effect is written
    /// and it is refused as asked. The owner's own op is admitted. With
    /// `shown`, the effect shows that content and its id covers its digest,
    /// so an approval admits only the same content.
    pub fn decide_op(
        &self,
        op: &str,
        path: &LogicalPath,
        shown: Option<&Shown>,
    ) -> Result<DecideReport, EffectError> {
        let (record, text) = shows(shown, MAX_DECIDE_BYTES)?;
        let mut view = (self.repositories().snapshot()).map_err(|source| EffectError::Read {
            directory: path.to_string(),
            source,
        })?;
        let landed = match view.target(path, false) {
            Ok(target) => {
                let on_disk = self.repository().landing(&target.path);
                Some(landed(path, LogicalPath::of_physical(&on_disk))?)
            }
            Err(WriteError::ThroughMount {
                inside: Some(inside),
                ..
            }) => {
                let home = self.repositories().home().map(Into::into);
                let mut mounted = Board::open(&inside.repository, home)
                    .map_err(|source| EffectError::Open { source })?;
                mounted.ask_as(self.asker.clone());
                let mut report = mounted.decide_op(op, &inside.path, shown)?;
                report.path = path.clone();
                return Ok(report);
            }
            Err(error @ WriteError::ThroughMount { .. }) => {
                return Err(EffectError::Resolve {
                    source: Box::new(error),
                });
            }
            // A directory or another file is where the path resolves.
            Err(_) => view
                .resolve(path)
                .ok()
                .filter(|location| location.board.source == board_model::Source::WorkingTree)
                .map(|location| landed(path, format!("/{}", location.path).parse()))
                .transpose()?,
        };
        let landed = landed.unwrap_or_else(|| path.clone());
        self.decide_at(op, path, landed, (record, text))
    }

    /// [`Board::decide_op`] for `op` at `landed`, the path in this board
    /// where `path` lands, showing `shown`.
    fn decide_at(
        &self,
        op: &str,
        path: &LogicalPath,
        landed: LogicalPath,
        (record, text): (Option<Value>, Option<String>),
    ) -> Result<DecideReport, EffectError> {
        let mut report = DecideReport {
            op: op.to_owned(),
            path: path.clone(),
            effect: None,
        };
        let Some(plugin) = &self.asker.plugin else {
            return Ok(report);
        };
        let personal = self
            .asker
            .personal(|| format!("{op} {path} needs a pending effect in the personal board"))?;
        let mut effect = Effect::new(root(self), plugin.clone(), op.to_owned(), landed.into())
            .showing(record, text);
        effect.asked = Some(utc(SystemTime::now()));
        let id = effect.id();
        let (plugin, op, path) = (plugin.clone(), op.to_owned(), path.clone());
        match seen(&personal, &effect, self.asker.signing.allowed.as_deref())? {
            Some(Answer::Approve) => {
                report.effect = Some(id);
                Ok(report)
            }
            Some(Answer::Refuse) => Err(EffectError::Refused {
                plugin,
                op,
                path,
                id,
            }),
            None => {
                hold(&personal, &effect)?;
                Err(EffectError::Asked {
                    plugin,
                    op,
                    path,
                    id,
                })
            }
        }
    }
}

/// What a decide shows the owner: the bytes of a text, or of a JSON record.
#[derive(Clone, Debug)]
pub enum Shown {
    Text(Vec<u8>),
    Record(Vec<u8>),
}

/// The record or text `shown` holds, refused when its input or its shown
/// form, the record's canonical JSON, is over `limit` bytes, when text is
/// not UTF-8, or when a record is not one JSON object.
pub(super) fn shows(
    shown: Option<&Shown>,
    limit: usize,
) -> Result<(Option<Value>, Option<String>), EffectError> {
    let (what, bytes) = match shown {
        None => return Ok((None, None)),
        Some(Shown::Text(bytes)) => ("text", bytes),
        Some(Shown::Record(bytes)) => ("record", bytes),
    };
    let unshown = |code, why: &str| {
        let why = format!("the {what} to decide on {why}");
        EffectError::Unshown { code, why }
    };
    let over = format!("is over the limit of {limit} bytes");
    if bytes.len() > limit {
        return Err(unshown(Code::JsonTooLarge, &over));
    }
    if let Some(Shown::Text(bytes)) = shown {
        let text = String::from_utf8(bytes.clone()).map(|text| (None, Some(text)));
        return text.map_err(|_| unshown(Code::JsonNotUtf8, "is not UTF-8"));
    }
    match parse_json(bytes) {
        Ok(record) if !record.is_object() => {
            Err(unshown(Code::RecordNotARecord, "is not a JSON object"))
        }
        Ok(record) if canonical_json(&record).as_bytes().len() > limit => {
            Err(unshown(Code::JsonTooLarge, &over))
        }
        Ok(record) => Ok((Some(record), None)),
        Err(error) => Err(unshown(error.code(), &format!("is refused: {error}"))),
    }
}

/// `parsed`, the logical path of the file where `path` lands, or the
/// refusal when that file has none: the owner would otherwise approve a
/// statement that names another path than the one written.
pub(super) fn landed(
    path: &LogicalPath,
    parsed: Result<LogicalPath, PathError>,
) -> Result<LogicalPath, EffectError> {
    parsed.map_err(|source| EffectError::Unlanded {
        path: path.clone(),
        source,
    })
}

/// Writes the owner's `answer` to the pending effect `id` in the personal
/// board and moves the effect from `effects/pending/` to
/// `effects/answered/`, in one commit. An approval holds
/// `signature`, made elsewhere, or else one the signer makes now; either is
/// checked before it is written. A plugin may not answer.
pub fn answer(
    asker: &Asker,
    id: &str,
    answer: Answer,
    reason: Option<String>,
    signature: Option<String>,
) -> Result<AnswerReport, EffectError> {
    if let Some(plugin) = &asker.plugin {
        return Err(EffectError::FromPlugin {
            plugin: plugin.clone(),
            path: None,
            what: format!("answer pending effect {id}"),
        });
    }
    let personal = asker.personal(|| "an answer is written in the personal board".to_owned())?;
    let (effect, blob) = read_pending(&personal, id)?;
    let not_found = || EffectError::NotFound { id: id.to_owned() };
    let mut seen = Seen::new(format!("/{ANSWERED}/{id}"), answer, reason);
    if answer == Answer::Approve {
        seen.signature = Some(match signature {
            Some(signature) => {
                verified(&effect, asker.signing.allowed.as_deref(), &signature)?;
                signature
            }
            None => signed(asker, &personal, &effect)?,
        });
    }
    let at = format!("/{SEEN}/{id}").parse().map_err(|_| not_found())?;
    let kept = format!("/{ANSWERED}/{id}")
        .parse()
        .map_err(|_| not_found())?;
    // An effect asked again after the owner removed its answer replaces the
    // copy kept from the first answer.
    let records = [
        (at, json!(seen), Expect::Absent),
        (kept, json!(effect), Expect::Any),
    ];
    let pending = Removal {
        path: format!("/{PENDING}/{id}")
            .parse()
            .map_err(|_| not_found())?,
        file: format!("{PENDING}/{id}.json"),
        blob,
    };
    let put = personal
        .write_answer(&records, &pending, &format!("{BINARY_NAME} answer {id}"))
        .map_err(|source| EffectError::Write {
            source: Box::new(source),
        })?;
    Ok(AnswerReport {
        id: id.to_owned(),
        answer,
        put,
    })
}

/// The pending effect `id` and the blob that holds it, refused unless its
/// request has that id and its digest is that of the content it shows.
fn read_pending(
    personal: &Board,
    id: &str,
) -> Result<(Effect, board_model::ObjectId), EffectError> {
    let not_found = || EffectError::NotFound { id: id.to_owned() };
    if !is_effect_id(id) {
        return Err(not_found());
    }
    let held = read_held(personal, &format!("{PENDING}/{id}"), EFFECT_TYPE)?;
    let (value, blob) = held.ok_or_else(not_found)?;
    let forged = |why: String| EffectError::Forged {
        id: id.to_owned(),
        why,
    };
    let effect = Effect::from_value(&value).map_err(|error| forged(error.to_string()))?;
    if effect.id() != id {
        return Err(forged(format!("its request has the id {}", effect.id())));
    }
    let shown = shown_digest(effect.record.as_ref(), effect.text.as_deref());
    let both = effect.record.is_some() && effect.text.is_some();
    if shown != effect.digest || both {
        return Err(forged(
            "its digest is not that of the record or text it shows".to_owned(),
        ));
    }
    Ok((effect, blob))
}

/// The owner's signature of `effect`'s approval, made by the signer after
/// Touch ID and checked before it is written.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module builds the approval statement it signs"
)]
fn signed(asker: &Asker, personal: &Board, effect: &Effect) -> Result<String, EffectError> {
    let root = root(personal);
    let statement = effect.approval();
    signature(&asker.signing, Path::new(&root), &statement).map_err(|why| EffectError::Unsigned {
        id: effect.id(),
        why,
    })
}

/// The owner's signature of `statement`, which the signer shows them from a
/// file in the cache under `root`'s name, checked against the allowed
/// signers file. `Err` says why there is none.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module is the one caller of the signer and the verifier"
)]
pub(super) fn signature(signing: &Signing, root: &Path, statement: &str) -> Result<String, String> {
    let signer = (signing.signer.as_ref())
        .ok_or("no signer is found: the owner's home is not known, or board is a debug build")?;
    let cache = (signing.cache.as_ref()).ok_or("no cache directory is named for the statement")?;
    let allowed = (signing.allowed.as_deref()).ok_or("no allowed signers file is named")?;
    let file = board_index::write_statement(cache, root, statement.as_bytes())
        .map_err(|error| format!("the statement cannot be written: {error}"))?;
    let signature = board_plugin::sign(signer, &file, &signing.environment)
        .map_err(|error| error.to_string())?;
    board_plugin::verify(allowed, statement, &signature).map_err(|error| error.to_string())?;
    Ok(signature)
}

impl Board {
    /// Writes each of `records`, by logical path, if its file is as it
    /// expects, and removes `pending`, if it still holds its blob, under one
    /// lock and in one commit.
    fn write_answer(
        &self,
        records: &[(LogicalPath, serde_json::Value, Expect)],
        pending: &Removal,
        message: &str,
    ) -> Result<PutReport, PutError> {
        let lock = self.repository().lock(LOCK_WAIT)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let mut schemas = Schemas::default();
        let mut prepared = Vec::with_capacity(records.len());
        for (path, record, expect) in records {
            let bytes = canonical_json(record);
            prepared.push(self.prepare(
                &lock,
                &mut view,
                &mut schemas,
                path,
                *expect,
                bytes.as_bytes(),
            )?);
        }
        self.write_all(lock, &prepared, std::slice::from_ref(pending), message)
            .map_err(Failure::one)
    }
}
