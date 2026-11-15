//! The stale lists of the consolidation prompt's step 5. Each is a pure
//! function of the live facts and today's day.

use jiff::civil::Date;
use serde::Serialize;

/// Tasks open this many days with no change are stale.
pub(crate) const TASK_DAYS: i32 = 14;
/// Changes in build and prototypes with no news this many days are stale.
pub(crate) const CHANGE_DAYS: i32 = 3;

/// An open change, as the stale rules read it.
#[derive(Clone, Debug)]
pub(crate) struct ChangeFacts {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) phase: String,
    /// Whether the change is a prototype.
    pub(crate) prototype: bool,
    /// A prototype's verdict, once it has one.
    pub(crate) verdict: Option<String>,
    /// The UTC day a changes capability last touched the change.
    pub(crate) touched: Option<String>,
    /// The task the change does.
    pub(crate) task: Option<String>,
}

/// An open task, as the stale rules read it.
#[derive(Clone, Debug)]
pub(crate) struct TaskFacts {
    pub(crate) path: String,
    pub(crate) title: String,
    /// The day the task was last looked at.
    pub(crate) reviewed: Option<String>,
}

/// One stale item: what it is, the day it was last touched and how many
/// days ago that was. A day that is absent or unreadable is none.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Item {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) since: Option<String>,
    pub(crate) days: Option<i32>,
}

/// The three stale lists, each sorted by path.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Stale {
    /// Open tasks that no open change does and that were last looked at
    /// more than 14 days ago, or never.
    pub(crate) tasks: Vec<Item>,
    /// Changes in build last touched more than 3 days ago.
    pub(crate) changes: Vec<Item>,
    /// Prototypes with no verdict, last touched more than 3 days ago.
    pub(crate) prototypes: Vec<Item>,
}

/// How many days before `today` the day `since` was, when it reads as a
/// day.
fn age(today: Date, since: Option<&str>) -> Option<i32> {
    let then: Date = since?.parse().ok()?;
    then.until(today).ok().map(|span| span.get_days())
}

fn item(path: &str, title: &str, since: Option<&str>, days: Option<i32>) -> Item {
    Item {
        path: path.to_owned(),
        title: title.to_owned(),
        since: since.map(str::to_owned),
        days,
    }
}

/// The stale work on `today`. A change or task with no readable day is
/// listed with `days` none, since nothing shows it is fresh.
pub(crate) fn stale(today: Date, changes: &[ChangeFacts], tasks: &[TaskFacts]) -> Stale {
    let mut found = Stale::default();
    for task in tasks {
        let named = changes
            .iter()
            .any(|change| change.task.as_deref() == Some(task.path.as_str()));
        let days = age(today, task.reviewed.as_deref());
        if !named && days.is_none_or(|days| days > TASK_DAYS) {
            found.tasks.push(item(
                &task.path,
                &task.title,
                task.reviewed.as_deref(),
                days,
            ));
        }
    }
    for change in changes {
        let days = age(today, change.touched.as_deref());
        let old = days.is_none_or(|days| days > CHANGE_DAYS);
        let listed = item(&change.path, &change.title, change.touched.as_deref(), days);
        if change.prototype {
            if old && change.verdict.is_none() {
                found.prototypes.push(listed);
            }
        } else if change.phase == "build" && old {
            found.changes.push(listed);
        }
    }
    for list in [&mut found.tasks, &mut found.changes, &mut found.prototypes] {
        list.sort_by(|a, b| a.path.cmp(&b.path));
    }
    found
}

#[cfg(test)]
mod tests;
