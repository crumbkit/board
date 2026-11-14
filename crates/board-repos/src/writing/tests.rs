//! A new file takes the first free name, a write whose HEAD moved is the
//! write, and a write lands by a push and then a pin. notes and tasks
//! relied on the former Python library's `writing.py` this way; their pytest
//! suites tested it through the plugins.

use board_model::{Code, Fields, Lined, Refusal};
use board_ops::PutError;
use serde_json::{Value, json};

use super::*;
use crate::fake::{Cloned, Local, Recorded, local};

fn own() -> Repo {
    Repo {
        root: "/".to_owned(),
        clone: None,
    }
}

fn mounted() -> Repo {
    Repo {
        root: "/repos/work".to_owned(),
        clone: Some("/clones/work".to_owned()),
    }
}

fn writer<'a>(host: &'a Local, clones: &'a Recorded) -> Writer<'a> {
    Writer {
        host,
        clones,
        plugin: "tasks",
        deadline: std::time::Duration::from_secs(10),
    }
}

/// The paths each put wrote to.
fn put_paths(host: &Local) -> Vec<String> {
    (host.puts.borrow().iter())
        .map(|lines| lines[0]["path"].as_str().unwrap().to_owned())
        .collect()
}

fn create_in(
    host: &Local,
    clones: &Recorded,
    repo: &Repo,
    names: usize,
) -> Result<(String, Option<String>), board_ops::PluginError> {
    create(
        writer(host, clones),
        repo,
        "tasks/call-the-bank",
        "---\n---\n",
        names,
        "tasks::task::busy",
    )
}

fn head_moved() -> PutError {
    PutError::HeadMoved { writes: Vec::new() }
}

fn stale() -> PutError {
    let line = Lined::new(1, Code::PutStale, "it holds a blob", Fields::default());
    PutError::Batch {
        code: Code::PutStale,
        refusals: vec![line],
    }
}

#[test]
fn a_batch_reports_its_one_lines_refusal_and_any_other_error_its_own_code() {
    assert_eq!(refused(&stale()), Code::PutStale);
    assert_eq!(refused(&head_moved()), Code::PutHeadMoved);
    let two = PutError::Batch {
        code: Code::PutInvalid,
        refusals: vec![
            Lined::new(1, Code::PutStale, "a", Fields::default()),
            Lined::new(2, Code::MountStale, "b", Fields::default()),
        ],
    };
    assert_eq!(refused(&two), Code::PutInvalid);
}

#[test]
fn a_new_file_takes_its_stem_and_then_the_next_numbered_name_when_one_is_there() {
    let host = local(&[("tasks/call-the-bank.md", "taken\n")]);
    host.scenario
        .commit(&[("tasks/call-the-bank.md", "taken\n")]);
    let clones = Recorded::default();
    let (path, commit) = create_in(&host, &clones, &own(), 10).unwrap();
    assert_eq!(path, "tasks/call-the-bank-2.md");
    assert!(commit.is_some());
    assert_eq!(
        put_paths(&host),
        ["/tasks/call-the-bank.md", "/tasks/call-the-bank-2.md"]
    );
    assert!(
        clones.calls.borrow().is_empty(),
        "the board's own tree has no pin to move"
    );
}

#[test]
fn a_new_file_is_refused_as_busy_once_every_name_it_tries_is_taken_and_written_when_one_is_free() {
    let taken = [
        ("tasks/call-the-bank.md", "a\n"),
        ("tasks/call-the-bank-2.md", "b\n"),
    ];
    let host = local(&taken);
    host.scenario.commit(&taken);
    let clones = Recorded::default();
    let refused = create_in(&host, &clones, &own(), 2).unwrap_err();
    assert_eq!(refused.code, "tasks::task::busy");
    assert_eq!(refused.fields["path"], json!("/tasks/call-the-bank.md"));
    assert_eq!(
        refused.message,
        "/tasks/call-the-bank.md and the 1 names after it are taken; nothing was written; call again"
    );
    let (path, _) = create_in(&host, &clones, &own(), 3).unwrap();
    assert_eq!(path, "tasks/call-the-bank-3.md");
}

#[test]
fn a_put_to_the_boards_own_tree_whose_head_moved_wrote_the_file_uncommitted() {
    let host = local(&[]);
    host.refusing.borrow_mut().push_back(head_moved());
    let clones = Recorded::default();
    let written = create_in(&host, &clones, &own(), 10).unwrap();
    assert_eq!(written, ("tasks/call-the-bank.md".to_owned(), None));
    assert_eq!(host.puts.borrow().len(), 1);
}

#[test]
fn a_put_through_a_mount_whose_head_moved_is_tried_again_at_the_same_name_after_a_new_pin() {
    let host = local(&[]);
    host.refusing.borrow_mut().extend([head_moved()]);
    let clones = Recorded::default();
    let (path, commit) = create_in(&host, &clones, &own_as_mount(), 10).unwrap();
    assert_eq!(path, "tasks/call-the-bank.md");
    assert!(commit.is_some());
    assert_eq!(
        put_paths(&host),
        ["/tasks/call-the-bank.md", "/tasks/call-the-bank.md"]
    );
    assert_eq!(
        *clones.calls.borrow(),
        [Cloned::Pin("/".to_owned()), Cloned::Pin("/".to_owned())]
    );
}

/// The board's own tree written as if it were a mount's clone, so a real
/// put stands in for one through a mount.
fn own_as_mount() -> Repo {
    Repo {
        root: "/".to_owned(),
        clone: Some("/clones/own".to_owned()),
    }
}

#[test]
fn a_put_through_a_mount_found_stale_after_its_head_moved_is_the_write_already_made() {
    let host = local(&[]);
    host.refusing.borrow_mut().extend([head_moved(), stale()]);
    let clones = Recorded::default();
    let written = create_in(&host, &clones, &mounted(), 10).unwrap();
    assert_eq!(written, ("tasks/call-the-bank.md".to_owned(), None));
}

#[test]
fn a_put_through_a_mount_whose_head_moved_three_times_is_left_uncommitted() {
    let host = local(&[]);
    host.refusing
        .borrow_mut()
        .extend([head_moved(), head_moved(), head_moved()]);
    let clones = Recorded::default();
    let written = create_in(&host, &clones, &mounted(), 10).unwrap();
    assert_eq!(written, ("tasks/call-the-bank.md".to_owned(), None));
    assert_eq!(host.puts.borrow().len(), ATTEMPTS);
}

#[test]
fn a_put_refused_for_any_other_reason_is_board_refused_with_its_code() {
    let host = local(&[]);
    let edited = PutError::Batch {
        code: Code::PutInvalid,
        refusals: vec![Lined::new(1, EDITED, "edited", Fields::default())],
    };
    host.refusing.borrow_mut().push_back(edited);
    let clones = Recorded::default();
    let refused = create_in(&host, &clones, &mounted(), 10).unwrap_err();
    assert_eq!(refused.code, "tasks::board::refused");
    assert_eq!(refused.fields["code"], json!("board::put::invalid"));
}

fn refusal(code: Code) -> Refusal {
    Refusal::new(code, "refused", &Fields::default())
}

#[test]
fn a_push_of_the_boards_own_tree_is_not_synced_and_of_a_mount_syncs_its_clone() {
    let host = local(&[]);
    let clones = Recorded::default();
    assert_eq!(
        push(writer(&host, &clones), &own()).as_deref(),
        Some("tasks::repo::not_synced")
    );
    assert!(clones.calls.borrow().is_empty());
    assert_eq!(push(writer(&host, &clones), &mounted()), None);
    let synced = Cloned::Sync {
        clone: "/clones/work".to_owned(),
        push: true,
    };
    assert_eq!(*clones.calls.borrow(), [synced]);
}

#[test]
fn a_failed_push_gives_the_code_of_its_refusal() {
    let host = local(&[]);
    let clones = Recorded::default();
    clones
        .syncs
        .borrow_mut()
        .extend([refusal(Code::SyncEdited), refusal(Code::SyncFailed)]);
    assert_eq!(
        push(writer(&host, &clones), &mounted()).as_deref(),
        Some("board::sync::edited")
    );
    assert_eq!(
        push(writer(&host, &clones), &mounted()).as_deref(),
        Some("board::sync::failed")
    );
}

#[test]
fn a_sync_refused_with_head_left_in_place_gives_its_code_and_any_other_refusal_is_refused() {
    let clones = Recorded::default();
    clones
        .syncs
        .borrow_mut()
        .extend([refusal(Code::SyncEdited), refusal(Code::SyncFailed)]);
    let host = local(&[]);
    let writer = writer(&host, &clones);
    assert_eq!(
        mounted().sync(writer, false).unwrap(),
        Some(Code::SyncEdited)
    );
    let refused = mounted().sync(writer, false).unwrap_err();
    assert_eq!(refused.code, "tasks::board::refused");
    assert_eq!(refused.fields["code"], json!("board::sync::failed"));
    assert_eq!(mounted().sync(writer, false).unwrap(), None);
}

#[test]
fn a_landed_write_is_pushed_then_pinned_and_gives_the_clones_head() {
    let host = local(&[]);
    let clones = Recorded {
        head: "beef".to_owned(),
        ..Recorded::default()
    };
    let landed_at = landed(
        writer(&host, &clones),
        &mounted(),
        Some("c0ffee".to_owned()),
    )
    .unwrap();
    assert_eq!(landed_at, (Some("beef".to_owned()), None));
    let calls = clones.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert!(matches!(calls[0], Cloned::Sync { push: true, .. }));
    assert_eq!(calls[1], Cloned::Pin("/repos/work".to_owned()));
}

#[test]
fn an_uncommitted_write_lands_with_no_commit_and_the_boards_own_tree_keeps_its_commit() {
    let host = local(&[]);
    let clones = Recorded::default();
    let uncommitted = landed(writer(&host, &clones), &mounted(), None).unwrap();
    assert_eq!(uncommitted, (None, None));
    let own_commit = landed(writer(&host, &clones), &own(), Some("c0ffee".to_owned())).unwrap();
    let not_synced = Some("tasks::repo::not_synced".to_owned());
    assert_eq!(own_commit, (Some("c0ffee".to_owned()), not_synced));
}

#[test]
fn a_sync_without_push_pulls_the_clone_and_with_push_pushes_it() {
    let host = local(&[]);
    let clones = Recorded::default();
    assert_eq!(synced(writer(&host, &clones), &mounted(), false), None);
    assert_eq!(synced(writer(&host, &clones), &mounted(), true), None);
    let sync = |push| Cloned::Sync {
        clone: "/clones/work".to_owned(),
        push,
    };
    assert_eq!(*clones.calls.borrow(), [sync(false), sync(true)]);
    assert_eq!(
        synced(writer(&host, &clones), &own(), false).as_deref(),
        Some("tasks::repo::not_synced")
    );
}

#[test]
fn a_write_landed_without_push_is_pinned_and_not_pushed_and_its_twin_is_pushed() {
    let host = local(&[]);
    let clones = Recorded {
        head: "beef".to_owned(),
        ..Recorded::default()
    };
    let kept = landed_with(
        writer(&host, &clones),
        &mounted(),
        Some("c0ffee".to_owned()),
        false,
    )
    .unwrap();
    let not_pushed = Some("tasks::repo::not_pushed".to_owned());
    assert_eq!(kept, (Some("beef".to_owned()), not_pushed));
    assert_eq!(
        *clones.calls.borrow(),
        [Cloned::Pin("/repos/work".to_owned())]
    );
    let pushed = landed_with(
        writer(&host, &clones),
        &mounted(),
        Some("c0ffee".to_owned()),
        true,
    )
    .unwrap();
    assert_eq!(pushed, (Some("beef".to_owned()), None));
    let own_commit = landed_with(
        writer(&host, &clones),
        &own(),
        Some("c0ffee".to_owned()),
        false,
    )
    .unwrap();
    let not_synced = Some("tasks::repo::not_synced".to_owned());
    assert_eq!(own_commit, (Some("c0ffee".to_owned()), not_synced));
}

#[test]
fn the_moved_codes_are_stale_and_head_moved() {
    let codes: Vec<&str> = MOVED.iter().map(|code| code.as_str()).collect();
    assert_eq!(codes, ["board::put::stale", "board::put::head_moved"]);
    assert_eq!(Value::from(EDITED.as_str()), json!("board::mount::stale"));
}

#[test]
fn the_conflict_records_that_name_a_written_file_are_listed_once_with_their_logical_paths() {
    let host = local(&[]);
    let clones = Recorded::default();
    let conflicted = |path: &str, record: &str| board_ops::Conflicted {
        path: path.to_owned(),
        kind: board_model::ConflictKind::BothChanged,
        record: record.to_owned(),
    };
    clones.found.borrow_mut().extend([
        conflicted("daily/d.md", "conflicts/b.json"),
        conflicted("daily/other.md", "conflicts/c.json"),
        conflicted("daily/d.md", "conflicts/a.json"),
        conflicted("daily/d.md", "conflicts/b.json"),
    ]);
    let writer = writer(&host, &clones);
    assert_eq!(
        writer.conflicts_at(&mounted(), &["daily/d.md"]),
        [
            "/repos/work/conflicts/a.json",
            "/repos/work/conflicts/b.json"
        ]
    );
    assert_eq!(
        writer.conflicts_at(&mounted(), &["daily/d.agent.md"]),
        Vec::<String>::new()
    );
}

#[test]
fn a_landed_write_lists_only_the_conflicts_of_the_syncs_after_the_write() {
    let host = local(&[]);
    let clones = Recorded::default();
    let conflicted = |record: &str| board_ops::Conflicted {
        path: "daily/d.md".to_owned(),
        kind: board_model::ConflictKind::BothChanged,
        record: record.to_owned(),
    };
    // The sync before the write found one conflict at the note.
    clones
        .next
        .borrow_mut()
        .push(conflicted("conflicts/before.json"));
    let writer = writer(&host, &clones);
    assert_eq!(synced(writer, &mounted(), true), None);
    assert_eq!(
        writer.conflicts_at(&mounted(), &["daily/d.md"]),
        ["/repos/work/conflicts/before.json"]
    );
    landed(writer, &mounted(), Some("c0ffee".to_owned())).unwrap();
    assert_eq!(
        writer.conflicts_at(&mounted(), &["daily/d.md"]),
        Vec::<String>::new()
    );
    // The sync that lands the write found another one.
    clones
        .next
        .borrow_mut()
        .push(conflicted("conflicts/after.json"));
    landed(writer, &mounted(), Some("c0ffee".to_owned())).unwrap();
    assert_eq!(
        writer.conflicts_at(&mounted(), &["daily/d.md"]),
        ["/repos/work/conflicts/after.json"]
    );
}
