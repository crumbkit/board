//! `board get` and `board refs`: the reads that answer each path they are given.

use board_model::{LogicalPath, Refusal};
use board_ops::{Absent, Board};

use super::Refs;
use crate::config::Context;
use crate::render::{self, Printer};
use crate::report::Output;

/// `get` of `paths`, with each attachment's bytes when `fetch` is set,
/// passing each refusal through `behind`.
pub(super) fn get(
    board: &Board,
    printer: Printer,
    paths: &[LogicalPath],
    fetch: bool,
    behind: &dyn Fn(Refusal) -> Refusal,
) -> Output {
    let answers = board.get_each(paths).map(|each| {
        each.map(move |answer| match answer {
            Ok(report) if fetch => board.fetched(report),
            answer => answer,
        })
    });
    render::get(printer, paths, answers, behind)
}

/// `refs` of `paths`, read from the paths asked, with `--missing` or without.
pub(super) fn refs(
    printer: Printer,
    board: &Board,
    context: &Context,
    asked: &Refs,
    paths: &[LogicalPath],
    behind: &dyn Fn(Refusal) -> Refusal,
) -> Output {
    let absent = if asked.missing {
        Absent::Locate
    } else {
        Absent::Refuse
    };
    let limit = asked.page.limit();
    let cache = context.cache.as_deref();
    match board.refs(paths, absent, &asked.fields, limit, cache) {
        Err(error) => render::refused(printer, error.refusal()),
        Ok(answers) => render::refs(printer, paths, answers, limit, behind),
    }
}
