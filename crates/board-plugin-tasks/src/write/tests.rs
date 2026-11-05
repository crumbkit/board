//! The edit loop's retries against a real mount, as the port's task asked:
//! a put refused because another Board changed the task between the read
//! and the put, a put refused because HEAD moved after Board wrote the file
//! and left it uncommitted in the clone, and a task that keeps changing.
//! These were the `Racing` unit test of `test_write.py`, which scripted
//! Board's refusals.

use super::*;
use crate::tests::real::{Pinning, Race, Real};

const REPORT: &str = "---\n$type: tasks/task/v2\ntitle: Report\nstatus: open\n---\n\nBody.\n";
const LOGICAL: &str = "/repos/work/tasks/report.md";

/// Marks the task at `logical` done through `host`, as `done` does once it
/// has synced.
fn done(host: &Real, logical: &str) -> Result<Edited, PluginError> {
    let found = listed(host, PLUGIN).unwrap();
    let repo = held(&found, logical);
    let clones = Pinning(host);
    let writer = Writer {
        host,
        clones: &clones,
        plugin: PLUGIN,
        deadline: board_tree::GIT_DEADLINE,
    };
    let change = |record: &Map<String, Value>, day| Ok(json_values(completed(record, day)));
    edit(writer, &repo, logical, &change, false)
}

#[test]
fn a_write_with_no_other_writer_lands_at_the_first_put() {
    let host = Real::new(Race::Alone, &[], &[("tasks/report.md", REPORT)]);
    let edited = done(&host, LOGICAL).unwrap();
    assert!(edited.commit.is_some(), "the put committed");
    assert_eq!(edited.record["status"], "done");
    assert_eq!(host.puts.borrow().len(), 1);
    assert!(host.text("tasks/report.md").contains("status: \"done\""));
}

#[test]
fn a_put_another_writer_made_stale_is_tried_again_on_a_new_read_and_keeps_their_change() {
    let host = Real::new(Race::Changes(1), &[], &[("tasks/report.md", REPORT)]);
    let edited = done(&host, LOGICAL).unwrap();
    assert!(edited.commit.is_some(), "the second put committed");
    assert_eq!(
        host.puts.borrow().len(),
        2,
        "one put was refused as stale, then one landed"
    );
    let text = host.text("tasks/report.md");
    assert!(text.contains("status: \"done\""), "{text}");
    assert!(text.ends_with("Body.\nTheirs.\n"), "{text}");
}

#[test]
fn a_put_whose_head_moved_is_committed_by_the_retry_from_the_file_board_left_uncommitted() {
    let host = Real::new(Race::HeadMoves, &[], &[("tasks/report.md", REPORT)]);
    let edited = done(&host, LOGICAL).unwrap();
    assert!(
        edited.commit.is_some(),
        "the retry committed the file Board had left"
    );
    let puts = host.puts.borrow();
    assert_eq!(puts.len(), 2);
    assert_eq!(
        puts[0]["text"], puts[1]["text"],
        "the retry sent the same text"
    );
    let head = host.clone.head().unwrap();
    let committed = String::from_utf8(head.files["tasks/report.md"].clone()).unwrap();
    assert!(committed.contains("status: \"done\""), "{committed}");
    assert!(
        head.files.contains_key("other.txt"),
        "the other writer's commit is kept"
    );
}

#[test]
fn a_task_that_changes_before_every_put_is_busy_and_names_the_last_refusal() {
    let host = Real::new(Race::Changes(ATTEMPTS), &[], &[("tasks/report.md", REPORT)]);
    let Err(refused) = done(&host, LOGICAL) else {
        panic!("the task kept changing")
    };
    assert_eq!(refused.code, "tasks::task::busy");
    assert_eq!(refused.fields["code"], "board::put::stale");
    assert!(
        refused.message.contains("kept changing"),
        "{}",
        refused.message
    );
    assert_eq!(host.puts.borrow().len(), ATTEMPTS);
    let text = host.text("tasks/report.md");
    assert!(!text.contains("status: \"done\""), "{text}");
}

/// In the board's own tree, a put refused because HEAD moved wrote the
/// file; the new read finds the text it sent and takes it as the write,
/// which stays uncommitted.
#[test]
fn a_put_to_the_boards_own_tree_whose_head_moved_is_the_write_when_the_new_read_finds_its_text() {
    let host = Real::new(Race::HeadMoves, &[("tasks/own.md", REPORT)], &[]);
    let edited = done(&host, "/tasks/own.md").unwrap();
    assert_eq!(edited.commit, None);
    assert_eq!(edited.record["status"], "done");
    assert_eq!(host.puts.borrow().len(), 1, "the new read found the write");
    assert!(host.own("tasks/own.md").contains("status: \"done\""));
}
