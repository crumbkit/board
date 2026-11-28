//! The warning `check` gives for a control Board marks
//! `board::ref::unreadable`: a control that is still owed though Board
//! could not read a file it names through a mount.

use board_model::{Code, LogicalPath};
use board_ops::{Absent, Host, PluginError, RefsError};
use serde_json::Value;

use super::Warning;
use crate::logical;

/// A warning for each control in `unread`, naming each reference Board
/// could not read and the mount it could not read, when Board names one.
/// Board codes a control `board::ref::unreadable` only when each of its
/// failing references is one, so each that resolves nowhere now is.
pub(super) fn unreadable_controls(
    host: &dyn Host,
    unread: &[(String, Value)],
) -> Result<Vec<Warning>, PluginError> {
    if unread.is_empty() {
        return Ok(Vec::new());
    }
    let paths = (unread.iter())
        .map(|(control, _)| logical(control))
        .collect::<Result<Vec<LogicalPath>, PluginError>>()?;
    let refused = |error: &RefsError| PluginError::board_refused("rules", "refs", &error.refusal());
    let answers = host
        .refs(&paths, Absent::Refuse, &[])
        .map_err(|e| refused(&e))?;
    let mut warnings = Vec::new();
    for ((control, _), answer) in unread.iter().zip(answers) {
        let report = answer.map_err(|e| refused(&e))?;
        let targets: Vec<LogicalPath> = (report.from.iter())
            .filter(|outgoing| outgoing.location.is_none())
            .map(|outgoing| outgoing.target.clone())
            .collect();
        let named: Vec<(String, Option<String>)> = (targets.iter().map(ToString::to_string))
            .zip(mounts(host, &targets))
            .collect();
        warnings.push(Warning {
            code: Code::RefUnreadable.as_str(),
            detail: unread_detail(control, &named),
        });
    }
    Ok(warnings)
}

/// For each of `targets`, the mount Board could not open or whose commit
/// it lacks on the way there, as a `refs` of the target names it; `None`
/// when it names none, or the target reads now.
fn mounts(host: &dyn Host, targets: &[LogicalPath]) -> Vec<Option<String>> {
    let mount_codes = [
        Code::MountRepoMissing.as_str(),
        Code::MountCommitMissing.as_str(),
    ];
    if targets.is_empty() {
        return Vec::new();
    }
    let Ok(answers) = host.refs(targets, Absent::Refuse, &[]) else {
        return vec![None; targets.len()];
    };
    (answers.into_iter())
        .map(|answer| {
            let refusal = answer.err()?.refusal();
            mount_codes.contains(&refusal.code.as_str()).then_some(())?;
            Some(refusal.fields.get("path")?.as_str()?.to_owned())
        })
        .collect()
}

/// What the warning for `control` says, given each reference it could not
/// read and the mount on its way, when known.
pub(crate) fn unread_detail(control: &str, targets: &[(String, Option<String>)]) -> String {
    if targets.is_empty() {
        return format!(
            "Board's index marks the control {control} board::ref::unreadable, but each \
             reference it names reads now, so it is still owed; do not delete it; run `board \
             check {control}`, then run the check again"
        );
    }
    let names: Vec<&str> = targets.iter().map(|(target, _)| target.as_str()).collect();
    let mut mounts: Vec<&str> = (targets.iter())
        .filter_map(|(_, mount)| mount.as_deref())
        .collect();
    mounts.sort_unstable();
    mounts.dedup();
    let repair = if mounts.is_empty() {
        format!("run `board check {control}`, which names the path Board could not read")
    } else {
        let each: Vec<String> = (mounts.iter())
            .map(|mount| format!("`board check {mount}` for the mount at {mount}"))
            .collect();
        format!("run {}, whose hint says how to repair it", each.join(", "))
    };
    format!(
        "the control {control} names {} through a mount Board could not read, so it is still \
         owed; do not delete it; run the check again, and if it still fails, {repair}",
        names.join(", ")
    )
}
