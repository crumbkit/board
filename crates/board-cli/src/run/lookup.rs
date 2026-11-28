//! `board get` and `board refs`: the reads that answer each path they are given.

use std::cell::RefCell;

use board_model::{LogicalPath, Refusal};
use board_ops::{Absent, Asked, At, Board, GetReport, MAX_LIMIT, ReadError};

use super::{Given, Page};
use crate::config::Context;
use crate::render::{self, Printer};
use crate::report::Output;
use crate::values::pointer;

/// `refs`'s arguments.
#[derive(Debug, clap::Args)]
pub(super) struct Refs {
    /// Each entry's logical path, such as /projects/p, or p from /projects; several share one check of the board.
    #[arg(required = true, value_name = "PATH", num_args = 1..=MAX_LIMIT)]
    pub(super) paths: Vec<Given>,
    /// When nothing is at a path yet, report where a file there would be instead of refusing.
    #[arg(long)]
    missing: bool,
    /// A JSON pointer, such as /status, whose value in each reference's target to show; can be repeated.
    #[arg(long = "field", value_name = "POINTER", value_parser = pointer)]
    fields: Vec<String>,
    #[command(flatten)]
    page: Page,
}

/// `get` of `paths`, in the working tree or in the commit `at` names, with
/// each attachment's bytes when `fetch` is set, passing each refusal
/// through `behind`. Each record a built-in plugin's note names is kept, and
/// the paths beside them are read in one more read at the same commit, for
/// the lines on stderr (docs/architecture/records/get.md, Notes beside a
/// record).
pub(super) fn get(
    board: &Board,
    printer: Printer,
    (paths, at): (&[LogicalPath], Option<At>),
    fetch: bool,
    behind: &dyn Fn(Refusal) -> Refusal,
) -> Output {
    let asked: RefCell<Vec<Asked>> = RefCell::new(Vec::new());
    let kept = |path: &LogicalPath, answer: Result<GetReport, ReadError>| {
        if let Ok(report) = &answer
            && let Some(record) = board.asked(path, report)
        {
            asked.borrow_mut().push(record);
        }
        match answer {
            Ok(report) if fetch => board.fetched(report),
            answer => answer,
        }
    };
    let notes = || board.noted(at, &asked.borrow());
    match at {
        None => {
            let answers = (board.get_each(paths)).map(|each| {
                paths
                    .iter()
                    .zip(each)
                    .map(|(path, answer)| kept(path, answer))
            });
            render::get(printer, paths, answers, behind, notes)
        }
        Some(at) => {
            let answers = (board.get_each_at(at, paths)).map(|each| {
                paths
                    .iter()
                    .zip(each)
                    .map(|(path, answer)| kept(path, answer))
            });
            render::get(printer, paths, answers, behind, notes)
        }
    }
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
