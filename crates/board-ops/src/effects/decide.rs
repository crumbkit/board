//! `board decide` and `board answer`.

use board_model::{
    Answer, Authority, Decision, Digest, Effect, LogicalPath, PUT_OP, Pattern, Preference, Request,
    Seen, canonical_json, is_effect_id,
};
use serde_json::json;

use board_tree::WriteError;

use super::{
    Asker, EFFECT_TYPE, EffectError, PENDING, PREFERENCES, SEEN, hold, read_record, root, seen,
};
use crate::board::Board;
use crate::put::PutReport;

/// The authority for an op at a path, the rules that gave it, and the
/// pending effect or answer that changes it.
#[derive(Debug)]
pub struct DecideReport {
    pub op: String,
    pub path: LogicalPath,
    /// What the rules decide. Its authority is replaced by the owner's
    /// answer, when `seen` holds one.
    pub decision: Decision,
    /// The pending effect an ask wrote or found.
    pub effect: Option<String>,
    /// The owner's answer, which gave the authority.
    pub seen: Option<Answer>,
    /// Whether the owner's rules were read: false when no personal board
    /// is named.
    pub owner_rules: bool,
}

/// What `board answer` wrote in the personal board.
#[derive(Debug)]
pub struct AnswerReport {
    pub id: String,
    pub answer: Answer,
    /// The preference saved, by its logical path.
    pub rule: Option<String>,
    pub put: PutReport,
}

impl Board {
    /// The authority that `op` at `path` has for whoever asks, decided as a
    /// put to `path` would be: at the path where it lands, after links, and
    /// through a mount in the mounted board. Under a plugin, for an op other
    /// than put, an answer the owner gave decides, and an ask with no answer
    /// writes its pending effect. A put asks with its content, so only `put`
    /// itself writes its pending effect.
    pub fn authority(&self, op: &str, path: &LogicalPath) -> Result<DecideReport, EffectError> {
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
                let mut report = mounted.authority(op, &inside.path)?;
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

    /// [`Board::authority`] for `op` at `landed`, the path in this board
    /// where `path` lands.
    fn decide_at(
        &self,
        op: &str,
        path: &LogicalPath,
        landed: &str,
    ) -> Result<DecideReport, EffectError> {
        let loaded = self.load()?;
        let plugin = self.asker.plugin.as_deref();
        let board = root(self);
        let request = Request {
            op,
            path: landed,
            plugin,
            board: &board,
        };
        let mut report = DecideReport {
            op: op.to_owned(),
            path: path.clone(),
            decision: board_model::decide(&loaded.rules, &request),
            effect: None,
            seen: None,
            owner_rules: loaded.personal.is_some(),
        };
        let Some(plugin) = plugin else {
            return Ok(report);
        };
        if let Some(file) = loaded.broken {
            return Err(EffectError::BadRule {
                plugin: plugin.to_owned(),
                path: path.clone(),
                file,
            });
        }
        let authority = report.decision.authority;
        if op == PUT_OP || matches!(authority, Authority::Act | Authority::Refuse) {
            return Ok(report);
        }
        let personal = loaded
            .personal
            .as_ref()
            .ok_or_else(|| EffectError::NoPersonal {
                what: format!("{op} {path} needs a pending effect in the personal board"),
            })?;
        let effect = Effect::new(board, plugin.to_owned(), op.to_owned(), landed.to_owned());
        report.seen = seen(personal, &effect.id())?;
        match report.seen {
            Some(answer) => report.decision.authority = answer.authority(),
            None if authority == Authority::Ask => report.effect = Some(hold(personal, &effect)?),
            None => {}
        }
        Ok(report)
    }
}

/// Writes the owner's `answer` to the pending effect `id` in the personal
/// board, with a preference for the effect's op, exact path, plugin and
/// board when `save_rule` is set, in one batch. A plugin may not answer.
pub fn answer(
    asker: &Asker,
    id: &str,
    answer: Answer,
    reason: Option<String>,
    save_rule: bool,
) -> Result<AnswerReport, EffectError> {
    if let Some(plugin) = &asker.plugin {
        return Err(EffectError::FromPlugin {
            plugin: plugin.clone(),
            path: None,
            what: format!("answer pending effect {id}"),
        });
    }
    let personal = asker.personal(|| "an answer is written in the personal board".to_owned())?;
    let not_found = || EffectError::NotFound { id: id.to_owned() };
    if !is_effect_id(id) {
        return Err(not_found());
    }
    let value = read_record(&personal, &format!("{PENDING}/{id}"), EFFECT_TYPE)?;
    let forged = |why: String| EffectError::Forged {
        id: id.to_owned(),
        why,
    };
    let effect = Effect::from_value(&value.ok_or_else(not_found)?)
        .map_err(|error| forged(error.to_string()))?;
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
    if save_rule && answer == Answer::Approve && effect.op == PUT_OP {
        return Err(EffectError::CannotWiden { id: id.to_owned() });
    }
    let rule = save_rule
        .then(|| {
            let exact = |pattern: &Pattern| pattern.is_exact();
            match effect.path.parse::<Pattern>() {
                Ok(pattern) if exact(&pattern) => Ok(pattern),
                Ok(_) => Err("it holds * or ?".to_owned()),
                Err(why) => Err(why),
            }
            .map_err(|why| EffectError::BadPattern {
                id: id.to_owned(),
                path: effect.path.clone(),
                why,
            })
        })
        .transpose()?;
    let seen = Seen::new(format!("/{PENDING}/{id}"), answer, reason);
    let mut lines = vec![json!({"path": format!("/{SEEN}/{id}"), "record": seen})];
    let saved = rule.map(|pattern| {
        let mut preference = Preference::new(effect.op.clone(), pattern, answer.authority());
        preference.plugin = Some(effect.plugin.clone());
        preference.board = Some(effect.board.clone());
        let at = format!("/{PREFERENCES}/{id}");
        lines.push(json!({"path": at, "record": preference}));
        at
    });
    let batch: String = lines.iter().map(|line| line.to_string() + "\n").collect();
    let put = personal
        .put_batch(&mut batch.as_bytes())
        .map_err(|source| EffectError::Write {
            source: Box::new(source),
        })?;
    Ok(AnswerReport {
        id: id.to_owned(),
        answer,
        rule: saved,
        put,
    })
}
