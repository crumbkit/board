//! `decide` and `answer`.

use std::fmt::Write as _;

use board_ops::{AnswerReport, DecideReport};
use crumbkit::Document;
use serde_json::json;

use super::{Answer, Printer, prose, put_json, said};
use crate::report::Output;

pub(crate) const DECIDE: Document<Answer> = prose(
    "decide-report",
    "The op admitted at a path, and the pending effect whose approval admitted it.",
);
pub(crate) const ANSWER: Document<Answer> = prose(
    "answer-report",
    "The answer written in the personal board, and its commit.",
);

pub(crate) fn decide(printer: Printer, report: &DecideReport) -> Output {
    let value = json!({
        "op": report.op,
        "path": report.path.to_string(),
        "effect": report.effect,
    });
    let mut text = format!("admitted {} {}\n", report.op, report.path);
    if let Some(id) = &report.effect {
        let _ = writeln!(text, "approved {id}");
    }
    printer.print(DECIDE, Ok(said(value, text)))
}

pub(crate) fn answer(printer: Printer, report: &AnswerReport) -> Output {
    let answered = json!(report.answer);
    let mut text = format!(
        "answered {} {}\n",
        report.id,
        answered.as_str().unwrap_or_default()
    );
    if let Some(commit) = report.put.commit {
        let _ = writeln!(text, "committed {commit}");
    }
    let value = json!({
        "id": report.id,
        "answer": report.answer,
        "put": put_json(&report.put),
    });
    printer.print(ANSWER, Ok(said(value, text)))
}
