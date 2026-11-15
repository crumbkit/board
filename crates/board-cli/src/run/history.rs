//! `board log`: the commits that changed a path.

use board_model::{LogicalPath, Refusal};
use board_ops::{At, Board};

use super::{Given, Page, Prefix, read};
use crate::render::{self, Printer};
use crate::report::Output;
use crate::values::commit;

/// `log`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Logging {
    /// The logical path, such as /tasks/a, or a from /tasks, whose file or directory's history to list; the board's root when left out.
    pub(super) path: Option<Given>,
    /// The commit to start the walk at: HEAD, the default, or a full commit id.
    #[arg(long, value_name = "COMMIT", value_parser = commit)]
    pub(super) at: Option<At>,
    #[command(flatten)]
    pub(super) page: Page,
}

/// `log` as `asked`, reading its path from `prefix` and passing each
/// refusal through `behind`.
pub(super) fn log(
    board: &Board,
    printer: Printer,
    prefix: Option<&Prefix>,
    asked: &Logging,
    behind: &dyn Fn(Refusal) -> Refusal,
) -> Output {
    let path =
        (asked.path.as_ref()).map_or(Ok(LogicalPath::root()), |path| read(prefix, path, "log"));
    let path = match path {
        Ok(path) => path,
        Err(refusal) => return render::refused(printer, *refusal),
    };
    let at = asked.at.unwrap_or(At::Head);
    match board.log(&path, at, asked.page.limit()) {
        Ok(report) => render::log(printer, &report),
        Err(error) => render::refused(printer, behind(error.refusal("log"))),
    }
}
