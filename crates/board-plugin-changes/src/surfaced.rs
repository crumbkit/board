//! The records `start` surfaces for a new change: the open tasks, the
//! decisions and the evals on its mount, which the plan then cites or
//! dismisses (the `considered` check).
//!
//! The open tasks are those of the notes repo that holds the change's own
//! task, and those under `/projects/<mount name>/`, the change's own task
//! left out. The decisions are those under `/projects/<mount name>/` and
//! those whose `refs` name the mount or a path under it, newest first. The
//! evals are those whose `mounts` name the mount and those under
//! `/projects/<mount name>/`. Each list holds at most ten and says how many
//! it left out.

use board_ops::{Host, PluginError, literal};
use schemars::JsonSchema;
use serde::Serialize;

use crate::board::{rows, text, tree_codes};
use crate::change::name_in;
use crate::records::TASK_TYPE;
use crate::tasks::split_task;

/// The most records each list holds (STANDARDS 23).
const SHOWN: usize = 10;
const DECISION_TYPE: &str = "plans/decision/v1";
const EVAL_TYPE: &str = "evals/eval/v1";

/// Some records of one kind: at most ten paths, how many there are and how
/// many the list left out.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub(crate) struct Listing {
    pub(crate) paths: Vec<String>,
    pub(crate) total: usize,
    pub(crate) left_out: usize,
}

impl Listing {
    fn of(mut paths: Vec<String>) -> Self {
        let total = paths.len();
        paths.truncate(SHOWN);
        Self {
            left_out: total - paths.len(),
            total,
            paths,
        }
    }
}

/// The open tasks, decisions and evals on a mount.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub(crate) struct Surfaced {
    pub(crate) tasks: Listing,
    pub(crate) decisions: Listing,
    pub(crate) evals: Listing,
}

impl Surfaced {
    /// Every path the lists hold, which the change records.
    pub(crate) fn paths(&self) -> Vec<String> {
        (self.tasks.paths.iter())
            .chain(&self.decisions.paths)
            .chain(&self.evals.paths)
            .cloned()
            .collect()
    }
}

/// The logical paths of the readable records of `kind` that `filter`, an SQL
/// condition on `logical` and `body`, admits, in `order`.
fn paths(
    host: &dyn Host,
    kind: &str,
    filter: &str,
    order: &str,
) -> Result<Vec<String>, PluginError> {
    let sql = format!(
        "SELECT logical FROM records WHERE type = {} AND (code IS NULL OR code IN ({})) AND \
         ({filter}) ORDER BY {order}",
        literal(kind),
        tree_codes()
    );
    Ok((rows(host, &sql)?.iter())
        .filter_map(|row| text(row.first()).map(str::to_owned))
        .collect())
}

/// An SQL condition that `logical` starts with `prefix`.
fn under(prefix: &str) -> String {
    format!(
        "substr(logical, 1, {}) = {}",
        prefix.chars().count(),
        literal(prefix)
    )
}

/// The records on `mount` that a change doing the task at `task` surfaces.
pub(crate) fn surfaced(host: &dyn Host, mount: &str, task: &str) -> Result<Surfaced, PluginError> {
    let repo = name_in(mount);
    // A board that is its own mount, `/`, has no project of its own.
    let project = if repo.is_empty() {
        "0".to_owned()
    } else {
        under(&format!("/projects/{repo}/"))
    };
    let notes = match split_task(task) {
        Ok(("/", _)) => under("/tasks/"),
        Ok((notes, _)) => under(&format!("{notes}/tasks/")),
        Err(_) => "0".to_owned(),
    };
    let tasks = paths(
        host,
        TASK_TYPE,
        &format!(
            "({notes} OR {project}) AND json_extract(body, '$.status') = 'open' AND logical != {}",
            literal(task)
        ),
        "logical",
    )?;
    let named = if repo.is_empty() {
        "0".to_owned()
    } else {
        format!(
            "EXISTS (SELECT 1 FROM json_each(body, '$.refs') AS r WHERE r.value = {0} OR \
             substr(r.value, 1, {1}) = {2})",
            literal(mount),
            mount.chars().count() + 1,
            literal(&format!("{mount}/"))
        )
    };
    let decisions = paths(
        host,
        DECISION_TYPE,
        &format!("{project} OR {named}"),
        "json_extract(body, '$.decided') DESC, logical",
    )?;
    let evals = paths(
        host,
        EVAL_TYPE,
        &format!(
            "{project} OR EXISTS (SELECT 1 FROM json_each(body, '$.mounts') AS m WHERE m.value = \
             {})",
            literal(mount)
        ),
        "logical",
    )?;
    Ok(Surfaced {
        tasks: Listing::of(tasks),
        decisions: Listing::of(decisions),
        evals: Listing::of(evals),
    })
}
