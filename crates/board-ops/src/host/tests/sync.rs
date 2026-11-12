//! A built-in plugin syncs a notes clone on the host `board -C <clone>`
//! opens, then moves the clone's mount, as `board -C <clone> sync` and
//! `board mount update` do, each within a deadline.

use std::path::PathBuf;
use std::time::Instant;

use board_model::ObjectId;

use super::*;
use crate::Expect;
use crate::host::Elsewhere;
use crate::tests::PROJECT_SCHEMA;

const PROJECT: &str = r#"{"$type": "lr/project/v1"}"#;

/// The files every commit of the notes remote holds: a board that can put
/// a project, and a daily note.
const BASE: [(&str, &str); 3] = [
    (".board/board.json", r#"{"$type": "board/config/v1"}"#),
    (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
    ("daily/2026-11-04.md", "one"),
];

/// The notes remote, its clone, and the called board mounting the clone at
/// `/notes` at the commit both started at. Each commits as an identity of
/// its own.
struct Notes {
    remote: Scenario,
    clone: Scenario,
    board: Scenario,
    started: ObjectId,
}

impl Notes {
    fn new() -> Self {
        let remote = Scenario::new();
        let started = remote.commit(&BASE);
        remote.accepting_pushes();
        let clone = Scenario::clone_of(&remote);
        clone.identified();
        let board = super::board(&[("fixture", true)], &["fixture"], Holds::Same);
        board.identified();
        board.write(
            "notes.json",
            Item::File(&format!(
                r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{started}"}}"#,
                clone.path()
            )),
        );
        Self {
            remote,
            clone,
            board,
            started,
        }
    }

    /// A commit on the remote changing the daily note to `text`, as
    /// another clone pushing would make.
    fn remote_writes(&self, text: &str) -> ObjectId {
        self.remote
            .commit(&[BASE[0], BASE[1], ("daily/2026-11-04.md", text)])
    }

    /// Calls `fixture.due`, which syncs the clone at `clone`, pushing when
    /// `push`, then moves `/notes`, each within `deadline`. It answers the
    /// commit the sync left HEAD at and the commit the mount pins, or the
    /// refusal of the first that failed.
    fn call(&self, clone: PathBuf, push: bool, deadline: Duration) -> Result<Value, Box<Refusal>> {
        let mut board = open(&self.board);
        board.build_in(built(vec![syncing(clone, push, deadline)]));
        let called = call_base(&board, &Plugins::default());
        assert_eq!(board.take_trace().work.get("started"), Some(&0));
        called
            .map(|report| report.output)
            .map_err(|error| Box::new(error.refusal()))
    }

    fn sync(&self, deadline: Duration) -> Result<Value, Box<Refusal>> {
        self.call(self.clone.workdir().to_path_buf(), false, deadline)
    }
}

/// `fixture`, syncing the clone at `clone` and then updating `/notes`.
fn syncing(clone: PathBuf, push: bool, deadline: Duration) -> Fixture {
    answering(
        "fixture",
        Box::new(move |host| {
            let refused = |command, refusal: &Refusal| {
                PluginError::board_refused("fixture", command, refusal)
            };
            let elsewhere = Elsewhere::Directory(clone.clone());
            let notes = host
                .board(&elsewhere)
                .map_err(|refusal| refused("-C", &refusal))?;
            let synced =
                (notes.sync(push, deadline)).map_err(|error| refused("sync", &error.refusal()))?;
            let at = "/notes".parse().unwrap();
            let pinned = (host.mount_update(&at, deadline))
                .map_err(|error| refused("mount update", &error.refusal()))?;
            Ok(json!({
                "count": synced.replayed.len(),
                "synced": synced.new.to_string(),
                "pushed": synced.pushed,
                "pinned": pinned.new.to_string(),
            }))
        }),
    )
}

/// The code Board refused with, which the plugin passes on in `code`.
fn code(refusal: &Refusal) -> &str {
    assert_eq!(refusal.code.as_str(), "fixture::board::refused");
    refusal.fields["code"].as_str().unwrap()
}

#[test]
fn a_plugin_syncs_a_notes_clone_and_moves_its_mount_to_what_the_sync_fetched() {
    let notes = Notes::new();
    let fetched = notes.remote_writes("two");
    let output = notes.sync(Duration::from_secs(30)).unwrap();
    assert_eq!(output["synced"], json!(fetched.to_string()));
    assert_eq!(output["pinned"], json!(fetched.to_string()));
    let pin = std::fs::read_to_string(notes.board.workdir().join("notes.json")).unwrap();
    assert!(pin.contains(&fetched.to_string()), "{pin}");
    let note = notes.clone.workdir().join("daily/2026-11-04.md");
    assert_eq!(std::fs::read_to_string(note).unwrap(), "two");
    assert_ne!(notes.started, fetched);
}

#[test]
fn a_plugin_pushes_a_commit_made_in_the_clone_and_pins_it() {
    let notes = Notes::new();
    let made = open(&notes.clone)
        .put(
            &"/projects/a".parse().unwrap(),
            Expect::Absent,
            PROJECT.as_bytes(),
        )
        .unwrap()
        .commit
        .unwrap();
    let output = notes
        .call(
            notes.clone.workdir().to_path_buf(),
            true,
            Duration::from_secs(30),
        )
        .unwrap();
    assert_eq!(output["pushed"], json!(true));
    assert_eq!(output["synced"], json!(made.to_string()));
    assert_eq!(output["pinned"], json!(made.to_string()));
    assert_eq!(notes.remote.head().unwrap().id, made);
}

/// A remote that has not answered when the deadline passes stops the sync
/// with `board::sync::timeout`, soon after the deadline, with a hint that
/// names no fixed number of seconds. The same remote answering within the
/// deadline syncs.
#[test]
fn a_sync_past_its_deadline_is_refused_as_a_timeout_and_one_within_it_syncs() {
    let notes = Notes::new();
    notes.remote_writes("two");
    notes.clone.delayed_origin(30);
    let started = Instant::now();
    let refusal = notes.sync(Duration::from_secs(1)).unwrap_err();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(code(&refusal), "board::sync::timeout");
    assert!(
        refusal
            .message
            .contains("the sync had left of its 1 second,"),
        "{}",
        refusal.message
    );
    let hint = refusal.hint.as_deref().unwrap_or_default();
    assert!(hint.contains("within its deadline"), "{hint}");
    let pin = std::fs::read_to_string(notes.board.workdir().join("notes.json")).unwrap();
    assert!(
        pin.contains(&notes.started.to_string()),
        "the pin stays: {pin}"
    );
    let answering = Notes::new();
    let fetched = answering.remote_writes("two");
    answering.clone.delayed_origin(0);
    let output = answering.sync(Duration::from_secs(30)).unwrap();
    assert_eq!(output["pinned"], json!(fetched.to_string()));
}

/// A file changed in an unpushed commit of the clone and on the remote is
/// `board::sync::conflict`, and the mount stays; the same commit changing
/// another file syncs.
#[test]
fn a_sync_conflict_is_refused_with_its_code_and_a_commit_of_another_file_syncs() {
    let conflicting = Notes::new();
    open(&conflicting.clone)
        .put(
            &"/projects/a".parse().unwrap(),
            Expect::Absent,
            PROJECT.as_bytes(),
        )
        .unwrap();
    conflicting.remote.commit(&[
        BASE[0],
        BASE[1],
        BASE[2],
        ("projects/a.json", r#"{"$type": "lr/project/v1", "x": 1}"#),
    ]);
    let refusal = conflicting.sync(Duration::from_secs(30)).unwrap_err();
    assert_eq!(code(&refusal), "board::sync::conflict");
    let replaying = Notes::new();
    open(&replaying.clone)
        .put(
            &"/projects/a".parse().unwrap(),
            Expect::Absent,
            PROJECT.as_bytes(),
        )
        .unwrap();
    replaying.remote_writes("two");
    let output = replaying.sync(Duration::from_secs(30)).unwrap();
    assert_eq!(output["count"], json!(1));
}

/// A clone whose branch follows no remote branch is `board::sync::no_remote`;
/// the same clone still following `origin/main` syncs.
#[test]
fn a_clone_without_a_remote_is_refused_with_its_code_and_its_twin_with_one_syncs() {
    let sync = |followed: bool| {
        let notes = Notes::new();
        let fetched = notes.remote_writes("two");
        if !followed {
            notes.clone.write(
                ".git/config",
                Item::Appended("[branch \"main\"]\n\tremote = \n"),
            );
        }
        (notes.sync(Duration::from_secs(30)), fetched)
    };
    let (refused, _) = sync(false);
    assert_eq!(code(&refused.unwrap_err()), "board::sync::no_remote");
    let (synced, fetched) = sync(true);
    assert_eq!(synced.unwrap()["pinned"], json!(fetched.to_string()));
}

/// The wait for the clone's board lock comes out of the sync's deadline: a
/// lock held past a one-second deadline is `board::put::locked`, soon after
/// the deadline rather than after `board sync`'s five seconds, and the same
/// sync with the lock free syncs.
#[test]
fn a_held_lock_that_uses_up_the_deadline_is_refused_and_its_twin_with_the_lock_free_syncs() {
    let sync = |held: bool| {
        let notes = Notes::new();
        let fetched = notes.remote_writes("two");
        let clone = open(&notes.clone);
        let lock = held.then(|| clone.repository().lock(Duration::ZERO).unwrap());
        let started = Instant::now();
        let synced = notes.sync(Duration::from_secs(1));
        drop(lock);
        (synced, started.elapsed(), fetched)
    };
    let (refused, took, _) = sync(true);
    assert_eq!(code(&refused.unwrap_err()), "board::put::locked");
    assert!(took < Duration::from_secs(4), "{took:?}");
    let (synced, _, fetched) = sync(false);
    assert_eq!(synced.unwrap()["pinned"], json!(fetched.to_string()));
}

/// A mount update that cannot take the board lock before its deadline is
/// `board::put::locked` and moves nothing; once the lock is free it moves.
#[test]
fn a_mount_update_past_its_deadline_for_the_lock_is_refused_and_one_with_the_lock_free_moves() {
    let notes = Notes::new();
    let fetched = notes.remote_writes("two");
    open(&notes.clone)
        .sync(false, Duration::from_secs(30))
        .unwrap();
    let mut board = open(&notes.board);
    board.build_in(built(vec![answering(
        "fixture",
        Box::new(|host| {
            let at = "/notes".parse().unwrap();
            let pinned = (host.mount_update(&at, Duration::ZERO)).map_err(|error| {
                PluginError::board_refused("fixture", "mount update", &error.refusal())
            })?;
            Ok(json!({"count": 0, "pinned": pinned.new.to_string()}))
        }),
    )]));
    let held = open(&notes.board)
        .repository()
        .lock(Duration::ZERO)
        .unwrap();
    let refusal = call_base(&board, &Plugins::default())
        .unwrap_err()
        .refusal();
    assert_eq!(code(&refusal), "board::put::locked");
    drop(held);
    let output = call_base(&board, &Plugins::default()).unwrap().output;
    assert_eq!(output["pinned"], json!(fetched.to_string()));
}
