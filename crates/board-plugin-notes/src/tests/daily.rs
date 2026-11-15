//! `daily` reads a note through a real mounted clone, including a clone in
//! the middle of a git operation.

use board_scenario::Item;
use serde_json::json;

use super::mount::{Mounted, Race};

/// A read never fails because the clone is in the middle of a git
/// operation: the note comes back as the clone holds it, with the sync's
/// code in `unsynced`.
#[test]
fn a_daily_note_is_read_while_the_notes_clone_is_in_a_merge() {
    let host = Mounted::new(Race::Alone, "");
    host.clone.commit(&[("daily/2026-11-04.md", "Held.\n")]);
    host.clone.write(".git/MERGE_HEAD", Item::File("x\n"));
    let given = json!({"repo": "/repos/notes", "date": "2026-11-04"});
    let note = crate::daily::daily(&host, &given).unwrap();
    assert_eq!(note["unsynced"], json!("board::sync::merging"), "{note}");
    assert_eq!(note["exists"], json!(true), "{note}");
    assert_eq!(note["text"], json!("Held.\n"), "{note}");
    assert_eq!(note["path"], json!("/repos/notes/daily/2026-11-04.md"));
}

#[test]
fn a_daily_note_read_from_a_clone_with_no_git_operation_in_progress_is_synced() {
    let host = Mounted::new(Race::Alone, "");
    let given = json!({"repo": "/repos/notes", "date": "2026-11-04"});
    let note = crate::daily::daily(&host, &given).unwrap();
    assert_eq!(note["unsynced"], json!(null), "{note}");
}
