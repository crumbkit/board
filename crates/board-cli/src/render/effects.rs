//! `decide` and `answer`, and the line a put that proposed prints.

use std::fmt::Write as _;

use board_model::{Authority, PUT_OP};
use board_ops::{AnswerReport, DecideReport};
use crumbkit::Document;
use serde_json::{Value, json};

use super::{Answer, Printer, prose, put_json, said};
use crate::report::Output;

pub(crate) const DECIDE: Document<Answer> = prose(
    "decide-report",
    "The authority an op has at a path, the rules that gave it, and any pending effect or answer.",
);
pub(crate) const ANSWER: Document<Answer> = prose(
    "answer-report",
    "The answer and any rule written in the personal board, and its commit.",
);

/// How many matching rules decide lists, besides the deciding one.
const RULES_SHOWN: usize = 20;

pub(crate) fn decide(printer: Printer, report: &DecideReport) -> Output {
    let decision = &report.decision;
    let shown: Vec<_> = (decision.matching.iter().enumerate())
        .filter(|(at, _)| *at < RULES_SHOWN || Some(*at) == decision.deciding)
        .collect();
    let left = decision.matching.len() - shown.len();
    let rules: Vec<Value> = (shown.iter())
        .map(|(at, rule)| {
            json!({
                "layer": rule.layer,
                "at": rule.at,
                "path": rule.preference.path.as_str(),
                "authority": rule.preference.authority,
                "plugin": rule.preference.plugin,
                "deciding": Some(*at) == decision.deciding,
            })
        })
        .collect();
    let value = json!({
        "op": report.op,
        "path": report.path.to_string(),
        "authority": decision.authority,
        "tell": decision.tell(),
        "rules": rules,
        "rules_left": left,
        "owner_rules": report.owner_rules,
        "effect": report.effect,
        "seen": report.seen,
    });
    let mut text = format!("{} {} {}\n", decision.authority, report.op, report.path);
    for (at, rule) in &shown {
        let deciding = if Some(*at) == decision.deciding {
            " (deciding)"
        } else {
            ""
        };
        let layer = json!(rule.layer);
        let plugin = rule.preference.plugin.as_deref().unwrap_or("any plugin");
        let _ = writeln!(
            text,
            "rule {} {} {} {} for {plugin}{deciding}",
            layer.as_str().unwrap_or_default(),
            rule.at,
            rule.preference.authority,
            rule.preference.path.as_str()
        );
    }
    if let Some(seen) = report.seen {
        let _ = writeln!(
            text,
            "answered {}",
            json!(seen).as_str().unwrap_or_default()
        );
    }
    let mut answer = said(value, text);
    if left > 0 {
        answer = answer.note(format!(
            "{left} more matching rules are left out; the deciding rule is listed"
        ));
    }
    if !report.owner_rules {
        answer = answer.note(
            "the known boards file names no personal board, so the owner's rules were not read"
                .to_owned(),
        );
    }
    let answer = match (&report.effect, decision.authority) {
        (Some(id), Authority::Ask) => answer.note(format!(
            "next: the owner answers with `board answer {id} approve` or `board answer {id} refuse`; then ask again"
        )),
        (None, Authority::Ask | Authority::Propose) if report.op == PUT_OP => answer.note(
            "a put under a plugin writes its pending effect with its record, so decide writes none"
                .to_owned(),
        ),
        _ => answer,
    };
    printer.print(DECIDE, Ok(answer))
}

pub(crate) fn answer(printer: Printer, report: &AnswerReport) -> Output {
    let answered = json!(report.answer);
    let mut text = format!(
        "answered {} {}\n",
        report.id,
        answered.as_str().unwrap_or_default()
    );
    if let Some(rule) = &report.rule {
        let _ = writeln!(text, "saved rule {rule}");
    }
    if let Some(commit) = report.put.commit {
        let _ = writeln!(text, "committed {commit}");
    }
    let value = json!({
        "id": report.id,
        "answer": report.answer,
        "rule": report.rule,
        "put": put_json(&report.put),
    });
    printer.print(ANSWER, Ok(said(value, text)))
}
