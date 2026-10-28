//! `board decide` and `board answer`.

use board_model::{
    Answer, BINARY_NAME, Digest, Effect, LogicalPath, Seen, canonical_json, is_effect_id,
};
use serde_json::json;

use board_tree::{LOCK_WAIT, WriteError};

use std::path::Path;

use super::{Asker, EFFECT_TYPE, EffectError, PENDING, SEEN, Signing, hold, read_held, root, seen};
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
    /// and it is refused as asked. The owner's own op is admitted.
    pub fn decide_op(&self, op: &str, path: &LogicalPath) -> Result<DecideReport, EffectError> {
        let mut view = (self.repositories().snapshot()).map_err(|source| EffectError::Read {
            directory: path.to_string(),
            source,
        })?;
        let landed = match view.target(path, false) {
            Ok(target) => {
                let on_disk = self.repository().landing(&target.path);
                Some(on_disk.strip_suffix(".json").unwrap_or(&on_disk).to_owned())
            }
            Err(WriteError::ThroughMount {
                inside: Some(inside),
                ..
            }) => {
                let home = self.repositories().home().map(Into::into);
                let mut mounted = Board::open(&inside.repository, home)
                    .map_err(|source| EffectError::Open { source })?;
                mounted.ask_as(self.asker.clone());
                let mut report = mounted.decide_op(op, &inside.path)?;
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
                .map(|location| location.path),
        };
        let landed = landed.map_or_else(|| path.to_string(), |file| format!("/{file}"));
        self.decide_at(op, path, &landed)
    }

    /// [`Board::decide`] for `op` at `landed`, the path in this board where
    /// `path` lands.
    fn decide_at(
        &self,
        op: &str,
        path: &LogicalPath,
        landed: &str,
    ) -> Result<DecideReport, EffectError> {
        let mut report = DecideReport {
            op: op.to_owned(),
            path: path.clone(),
            effect: None,
        };
        let Some(plugin) = self.asker.plugin.as_deref() else {
            return Ok(report);
        };
        let personal = self
            .asker
            .personal(|| format!("{op} {path} needs a pending effect in the personal board"))?;
        let effect = Effect::new(
            root(self),
            plugin.to_owned(),
            op.to_owned(),
            landed.to_owned(),
        );
        let id = effect.id();
        let (plugin, op, path) = (plugin.to_owned(), op.to_owned(), path.clone());
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

/// Writes the owner's `answer` to the pending effect `id` in the personal
/// board and removes the pending effect, in one commit. A plugin may not
/// answer.
pub fn answer(
    asker: &Asker,
    id: &str,
    answer: Answer,
    reason: Option<String>,
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
    let mut seen = Seen::new(format!("/{PENDING}/{id}"), answer, reason);
    if answer == Answer::Approve {
        seen.signature = Some(signed(asker, &personal, &effect)?);
    }
    let records = [(format!("/{SEEN}/{id}"), json!(seen), Expect::Absent)];
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
    let shown = match (&effect.record, &effect.text) {
        (Some(record), None) => Some(Digest::of(canonical_json(record).as_bytes())),
        (None, Some(text)) => Some(Digest::of(text.as_bytes())),
        _ => None,
    };
    if effect.digest.is_some() && shown != effect.digest {
        return Err(forged(
            "its digest is not that of the record or text it shows".to_owned(),
        ));
    }
    Ok((effect, blob))
}

/// The owner's signature of `effect`'s approval, made by the signer after
/// Touch ID and checked before it is written.
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
pub(super) fn signature(signing: &Signing, root: &Path, statement: &str) -> Result<String, String> {
    let signer =
        (signing.signer.as_ref()).ok_or("no signer is found: the owner's home is not known")?;
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
        records: &[(String, serde_json::Value, Expect)],
        pending: &Removal,
        message: &str,
    ) -> Result<PutReport, PutError> {
        let lock = self.repository().lock(LOCK_WAIT)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let mut schemas = Schemas::default();
        let mut prepared = Vec::with_capacity(records.len());
        for (at, record, expect) in records {
            let path: LogicalPath = at.parse().expect("an answer writes well-formed paths");
            let bytes = canonical_json(record);
            prepared.push(self.prepare(
                &lock,
                &mut view,
                &mut schemas,
                &path,
                *expect,
                bytes.as_bytes(),
            )?);
        }
        self.write_all(lock, &prepared, std::slice::from_ref(pending), message)
            .map_err(Failure::one)
    }
}
