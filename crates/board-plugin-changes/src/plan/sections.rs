//! The warning for a plan that lacks a section its flow's checks read.
//!
//! The `sections` checks of a flow run only when a change leaves the phase
//! that holds them, so a plan without the land phase's test sections was
//! found only at land. `changes plan` reads every phase's `sections` checks
//! when it writes the plan, and names each heading the text lacks. It
//! reads no check's `when`, so a check that would not apply to the change
//! may still be named.

use crate::Warning;
use crate::flow::{Flow, Kind};
use crate::gates::sections;

/// `changes::plan::sections_missing` naming each heading a `sections` check
/// of `flow` requires and `text` holds no text under, once, in the order the
/// flow lists them; none when the text has each.
pub(super) fn missing(flow: &Flow, text: &str) -> Option<Warning> {
    let found = sections(text);
    let mut wanted: Vec<&str> = Vec::new();
    let mut checks: Vec<String> = Vec::new();
    for phase in &flow.phases {
        for check in &phase.checks {
            if check.kind.known() != Some(&Kind::Sections) {
                continue;
            }
            let lacked: Vec<&str> = (check.sections.iter().flatten().map(String::as_str))
                .filter(|heading| {
                    found
                        .get(&heading.to_lowercase())
                        .is_none_or(String::is_empty)
                })
                .collect();
            if lacked.is_empty() {
                continue;
            }
            checks.push(format!("{} ({})", check.id, phase.name));
            for heading in lacked {
                if !wanted.contains(&heading) {
                    wanted.push(heading);
                }
            }
        }
    }
    if wanted.is_empty() {
        return None;
    }
    Some(Warning {
        code: "changes::plan::sections_missing",
        detail: format!(
            "the plan has no text under: {}, which the flow {}'s checks {} require; add each \
             section before the change leaves that phase, or `advance` refuses",
            wanted.join(", "),
            flow.name,
            checks.join(", ")
        ),
    })
}
