//! `put`'s report, which `mount add`, `type add` and `plugin enable` print
//! too: each write, removal and commit, and a warning for each field a
//! command-line put dropped.

use std::fmt::Write as _;

use board_model::Mount;
use board_ops::PutReport;
use serde_json::json;

use super::kit::{Note, Printer};
use super::{PUT, detail_text, location, said};
use crate::report::Output;

/// `put`: as text, one line per write naming its display path and new
/// blob, and the commit when one was made. A write left degraded gets a
/// note on stderr in either format, and `note` follows as text.
pub(crate) fn put(printer: Printer, report: &PutReport, note: Option<String>) -> Output {
    put_as(printer, report, put_text(report), note)
}

/// `mount add`: put's report, whose text names the mount's repository,
/// the ref it follows and the commit it is read at, then the commit the
/// put made, when it made one.
pub(crate) fn mount_add(printer: Printer, mount: &Mount, report: &PutReport) -> Output {
    let path = report
        .writes
        .first()
        .map(|written| written.display.to_string());
    let mut text = format!(
        "mounted {}: {}, following {}, at {}\n",
        path.unwrap_or_default(),
        mount.repo,
        mount.follow,
        mount.commit
    );
    if let Some(commit) = report.commit {
        let _ = writeln!(text, "commit {commit}");
    }
    put_as(printer, report, text, None)
}

/// Put's report as `text`, with a note on stderr for each write left
/// degraded or mounting a temporary directory, then `note` as text.
fn put_as(printer: Printer, report: &PutReport, text: String, note: Option<String>) -> Output {
    let notes = (report.writes.iter()).filter_map(|written| {
        let (code, detail) = written.degraded.as_ref()?;
        let (display, why) = (&written.display, detail_text(detail));
        let note = format!("note: {display} is written but degraded: {code} {display}{why}");
        Some(Note::Always(note))
    });
    let temporary = (report.writes.iter()).filter_map(|written| {
        let repo = written.temporary.as_ref()?;
        Some(Note::Always(format!(
            "note: {} mounts the repository {repo}, which is under a temporary directory; \
             the mount stops working when that directory goes, so keep the clone elsewhere",
            written.display
        )))
    });
    let dropped = (report.mended.iter()).flat_map(|mended| {
        let at = match mended.mends.line {
            Some(line) => format!("line {line}, {}", mended.path),
            None => mended.path.to_string(),
        };
        (mended.mends.dropped.iter()).map(move |dropped| {
            let near = (dropped.near.as_ref())
                .map_or_else(String::new, |near| format!("; the type has {near}"));
            Note::Always(format!(
                "warning: {at}: dropped {}, which its type does not name{near}",
                dropped.pointer
            ))
        })
    });
    let answer = said(json!(put_data(report)), text);
    let notes = (dropped.chain(notes).chain(temporary)).chain(note.map(Note::Text));
    printer.noted(PUT, answer, notes.collect())
}

pub(super) fn put_text(report: &PutReport) -> String {
    let mut text = String::new();
    for written in &report.writes {
        let _ = writeln!(text, "wrote {} {}", written.display, written.blob);
    }
    for removed in &report.removed {
        let _ = writeln!(text, "removed {} {}", removed.path, removed.blob);
    }
    for absent in &report.absent {
        let _ = writeln!(text, "absent {absent}");
    }
    if let Some(commit) = report.commit {
        let _ = writeln!(text, "committed {commit}");
    }
    text
}

/// What a put wrote and its commit, as `put --json` and `plugin enable
/// --json` say it.
pub(super) fn put_data(report: &PutReport) -> board_types::PutReport {
    let write = |written: &board_ops::Written| board_types::Write {
        blob: written.blob.to_string(),
        degraded: (written.degraded.as_ref()).map(|(code, detail)| board_types::Degraded {
            code: code.as_str().to_owned(),
            failures: detail.failures,
            pointer: detail.pointer.clone(),
            reason: detail.reason.clone(),
        }),
        display: written.display.to_string(),
        location: location(&written.location, &written.display.to_string()),
        previous: written.previous.map(|blob| blob.to_string()),
    };
    let line = |mended: &board_ops::Mended| mended.mends.line.map(|line| line as u64);
    let filled = (report.mended.iter()).flat_map(|mended| {
        (mended.mends.filled.iter()).map(move |pointer| board_types::Filled {
            line: line(mended),
            path: mended.path.to_string(),
            pointer: pointer.clone(),
        })
    });
    let dropped = (report.mended.iter()).flat_map(|mended| {
        (mended.mends.dropped.iter()).map(move |dropped| board_types::Dropped {
            line: line(mended),
            near: dropped.near.clone(),
            path: mended.path.to_string(),
            pointer: dropped.pointer.clone(),
        })
    });
    board_types::PutReport {
        absent: report.absent.iter().map(ToString::to_string).collect(),
        commit: report.commit.map(|commit| commit.to_string()),
        dropped: dropped.collect(),
        filled: filled.collect(),
        removed: (report.removed.iter())
            .map(|removed| board_types::Removed {
                blob: removed.blob.to_string(),
                path: removed.path.to_string(),
            })
            .collect(),
        writes: report.writes.iter().map(write).collect(),
    }
}
