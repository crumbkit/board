//! When a repeating task comes back, and what completing a task writes
//! (plugins/tasks/README.md, Writing tasks).

use board_repos::Date;
use serde_json::{Map, Value};

use crate::dates::horizon;

const WEEK: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// The first date after `after` that `rule` names. A month day past the
/// month's last is its last day. `rule` matches the schema's `repeat`
/// pattern; any other rule is daily.
pub(crate) fn following(rule: &str, after: Date) -> Date {
    let (kind, value) = rule.split_once('-').unwrap_or((rule, ""));
    let number = |text: &str| text.parse::<u8>().unwrap_or(1);
    match kind {
        "weekdays" => after.plus(match after.weekday() {
            4 => 3,
            5 => 2,
            _ => 1,
        }),
        "weekly" => {
            let day = (WEEK.iter().position(|each| *each == value))
                .map_or(0, |at| i64::try_from(at).unwrap_or(0));
            after.plus((day - after.weekday() - 1).rem_euclid(7) + 1)
        }
        "monthly" => {
            let (year, month) = (after.year, after.month);
            let next = (year + i64::from(month / 12), month % 12 + 1);
            [(year, month), next]
                .into_iter()
                .map(|(year, month)| Date::clamped(year, month, number(value)))
                .find(|date| *date > after)
                .unwrap_or(after.plus(1))
        }
        "annually" => {
            let (month, day) = value.split_once('-').unwrap_or(("1", "1"));
            [after.year, after.year + 1]
                .into_iter()
                .map(|year| Date::clamped(year, number(month), number(day)))
                .find(|date| *date > after)
                .unwrap_or(after.plus(1))
        }
        _ => after.plus(1),
    }
}

/// The fields completing the task `record` sets on `today`. A task without
/// `repeat` is done. A repeating one stays open, deferred to its next
/// occurrence, and never to an earlier date than its `defer`. The
/// occurrence counts from the date `defer` ends, so a missed one comes back:
/// a date as it is, or a horizon counted from `reviewed`, as
/// `tasks.available` counts it. Without either, it counts from today. An
/// `ephemeral` task counts from the later of that date and today, so a
/// missed occurrence is not kept.
pub(crate) fn completed(
    record: &Map<String, Value>,
    today: Date,
) -> Vec<(&'static str, Option<String>)> {
    let Some(rule) = record.get("repeat").and_then(Value::as_str) else {
        return vec![("status", Some("done".to_owned()))];
    };
    let defer = record.get("defer").and_then(Value::as_str).unwrap_or("");
    let reviewed = record.get("reviewed").and_then(Value::as_str).unwrap_or("");
    let ended = match horizon(defer) {
        Some(days) => Date::parse(reviewed).map(|date| date.plus(days)),
        None => defer.get(..10).and_then(Date::parse),
    };
    let mut start = ended.unwrap_or(today);
    if record.get("ephemeral") == Some(&Value::Bool(true)) {
        start = start.max(today);
    }
    vec![("defer", Some(following(rule, start).to_string()))]
}

#[cfg(test)]
mod tests;
