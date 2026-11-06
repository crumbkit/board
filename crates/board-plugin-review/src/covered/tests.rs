//! A report covers commits only with a verdict that approves or comments,
//! and a finding blocks unless its severity is known not to.

use serde_json::json;

use super::*;

/// The report of a range holding `verdict` and one finding of `severity`.
fn read(verdict: &str, severity: &str) -> Covering {
    let report = json!({
        "$type": "review/report/v1",
        "range": format!("{}..{}", "a".repeat(40), "b".repeat(40)),
        "findings": [{"severity": severity, "text": "A finding."}],
        "verdict": verdict,
    });
    covering("/reviews/r1/report".to_owned(), &report.to_string()).unwrap()
}

/// A verdict a newer Board added does not cover the commits, as `changes`
/// does not, and a severity a newer Board added blocks, as `high` does.
/// Their twins `comment` and `low` cover and do not block.
#[test]
fn a_verdict_and_severity_a_newer_board_added_do_not_cover_and_block_like_changes_and_high() {
    for (verdict, severity) in [("changes", "high"), ("defer", "severe")] {
        let report = read(verdict, severity);
        assert!(report.changes, "{verdict}");
        assert!(report.blocking, "{severity}");
    }
    let report = read("comment", "low");
    assert!(!report.changes && !report.blocking);
}
