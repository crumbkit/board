//! Which refusals while sync reads one change leave only that change
//! unread, and which refuse the whole sync.

use board_ops::PluginError;

use crate::change::tests::{GIT_FAILS, STOPPED, host};
use crate::records::ChangeRecord;
use crate::sync::unread_or_refused;

/// A change in build whose newest pull request is `pull`.
fn pulling(pull: &str) -> ChangeRecord {
    let mut change = ChangeRecord::new("t".to_owned(), "/repos/x".to_owned(), "build");
    change.pulls = Some(vec![pull.to_owned()]);
    change
}

/// The refusal sync met reading the change, as the strict reads give it.
fn met() -> PluginError {
    PluginError {
        code: "changes::board::refused".to_owned(),
        message: "board refs refused".to_owned(),
        fields: serde_json::Map::new(),
        hint: None,
    }
}

#[test]
fn a_change_whose_pull_request_is_beneath_a_mount_that_cannot_be_read_is_unread_with_no_command() {
    let entry = unread_or_refused(&host(), "/changes/t", &pulling(STOPPED), met()).unwrap();
    let entry = serde_json::to_value(entry).unwrap();
    assert_eq!(entry["code"], "changes::sync::unread", "{entry}");
    assert_eq!(entry["command"], serde_json::Value::Null, "{entry}");
    let detail = entry["detail"].as_str().unwrap();
    assert!(detail.contains("board::mount::repo_missing"), "{detail}");
}

#[test]
fn a_change_whose_read_fails_in_git_refuses_the_whole_sync_with_the_refusal_it_met() {
    let refused = unread_or_refused(&host(), "/changes/t", &pulling(GIT_FAILS), met()).unwrap_err();
    assert_eq!(refused.code, "changes::board::refused");
    assert_eq!(refused.message, "board refs refused");
}
