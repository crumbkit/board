//! Syncing clones of one remote.

use board_model::{Code, Fields, LogicalPath, ObjectId};
use board_scenario::{Item, Scenario};
use board_tree::{ATTEMPTS, Replayed, Synced};

use crate::sync::SyncError;
use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, open};

const CONFIG: &str = r#"{"$type": "board/config/v1"}"#;
const PROJECT: &str = r#"{"$type": "lr/project/v1"}"#;

/// The files every commit on the remote holds.
const BASE: [(&str, &str); 4] = [
    (".board/board.json", CONFIG),
    (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
    (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
    ("projects/p.json", PROJECT),
];

/// A remote holding the test board in one commit, which accepts pushes.
fn remote() -> Scenario {
    let remote = Scenario::new();
    remote.commit(&BASE);
    remote.accepting_pushes();
    remote
}

/// A commit on `remote` of the base files and `extra`, as another clone
/// pushing would make.
fn remote_commit(remote: &Scenario, extra: &[(&str, &str)]) -> ObjectId {
    let files: Vec<(&str, &str)> = BASE.iter().chain(extra).copied().collect();
    remote.commit(&files)
}

/// A clone of `remote` with an identity to commit as. Each clone commits
/// as someone else, so two puts of one record in one second differ.
fn clone(remote: &Scenario) -> Scenario {
    static CLONES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let number = CLONES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let clone = Scenario::clone_of(remote);
    clone.write(
        ".git/config",
        Item::Appended(&format!(
            "[user]\n\tname = Board Test {number}\n\temail = board@example.com\n"
        )),
    );
    clone
}

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

/// Puts a project record at `at` in `scenario`'s board and returns its
/// commit.
fn put(scenario: &Scenario, at: &str) -> ObjectId {
    open(scenario)
        .put(&path(at), crate::Expect::Absent, PROJECT.as_bytes())
        .unwrap()
        .commit
        .unwrap()
}

fn sync(scenario: &Scenario, push: bool) -> Result<Synced, SyncError> {
    open(scenario).sync(push, board_tree::GIT_DEADLINE)
}

fn refused(scenario: &Scenario, push: bool) -> (Code, String) {
    let error = sync(scenario, push).unwrap_err();
    (error.code(), error.to_string())
}

#[test]
fn two_clones_that_each_put_a_record_both_hold_both_after_syncing() {
    let remote = remote();
    let (a, b) = (clone(&remote), clone(&remote));
    let from_a = put(&a, "/projects/a");
    let from_b = put(&b, "/projects/b");
    let synced_a = sync(&a, true).unwrap();
    assert_eq!(synced_a.new, from_a);
    assert!(
        synced_a.pushed && synced_a.replayed.is_empty(),
        "{synced_a:?}"
    );
    assert_eq!(remote.head().unwrap().id, from_a);
    let synced_b = sync(&b, true).unwrap();
    let replayed = synced_b.replayed[0].to.unwrap();
    assert_eq!(
        synced_b.replayed,
        [Replayed {
            from: from_b,
            to: Some(replayed)
        }]
    );
    assert_eq!(
        (synced_b.fetched, synced_b.old, synced_b.new),
        (from_a, Some(from_b), replayed)
    );
    assert!(synced_b.pushed, "{synced_b:?}");
    let head = b.head().unwrap();
    assert_eq!((head.id, head.parents), (replayed, vec![from_a]));
    assert_eq!(head.message, "board put /projects/b");
    assert!(
        head.files.contains_key("projects/a.json") && head.files.contains_key("projects/b.json")
    );
    assert_eq!(remote.head().unwrap().id, replayed);
    let synced_a = sync(&a, true).unwrap();
    assert_eq!(
        (synced_a.old, synced_a.new, synced_a.pushed),
        (Some(from_a), replayed, false)
    );
    assert_eq!(
        std::fs::read(a.workdir().join("projects/b.json")).unwrap(),
        std::fs::read(b.workdir().join("projects/b.json")).unwrap()
    );
    assert_eq!(
        a.index().get("projects/b.json"),
        b.index().get("projects/b.json")
    );
}

#[test]
fn a_record_whose_bytes_the_remote_already_holds_makes_no_commit() {
    let remote = remote();
    let (a, b) = (clone(&remote), clone(&remote));
    put(&a, "/projects/same");
    let from_b = put(&b, "/projects/same");
    sync(&a, true).unwrap();
    let synced = sync(&b, true).unwrap();
    assert_eq!(
        synced.replayed,
        [Replayed {
            from: from_b,
            to: None
        }]
    );
    assert_eq!((synced.new, synced.pushed), (synced.fetched, false));
    assert_eq!(b.head().unwrap().id, synced.fetched);
}

#[test]
fn a_sync_without_push_moves_head_and_pushes_nothing() {
    let remote = remote();
    let a = clone(&remote);
    let moved = remote_commit(&remote, &[("projects/r.json", PROJECT)]);
    let from_a = put(&a, "/projects/a");
    let synced = sync(&a, false).unwrap();
    assert_eq!(
        (synced.fetched, synced.old, synced.pushed),
        (moved, Some(from_a), false)
    );
    assert_eq!(a.head().unwrap().parents, [moved]);
    assert_eq!(remote.head().unwrap().id, moved);
    assert!(a.workdir().join("projects/r.json").exists());
    let again = sync(&a, false).unwrap();
    assert_eq!(
        (again.old, again.new, again.replayed.len()),
        (Some(synced.new), synced.new, 0)
    );
}

#[test]
fn a_file_changed_here_and_on_the_remote_is_refused_and_one_changed_here_alone_is_admitted() {
    let remote = remote();
    let a = clone(&remote);
    let b = clone(&remote);
    put(&a, "/projects/a");
    remote_commit(
        &remote,
        &[("projects/a.json", r#"{"$type": "lr/project/v1", "x": 1}"#)],
    );
    let (code, message) = refused(&a, true);
    assert_eq!(code, Code::SyncConflict);
    assert_eq!(
        message,
        "projects/a.json changed on the remote and in a commit here that is not pushed"
    );
    let refusal = sync(&a, true).unwrap_err().refusal();
    assert_eq!(refusal.fields["path"], "/projects/a");
    put(&b, "/projects/b");
    assert!(sync(&b, true).unwrap().pushed);
}

#[test]
fn a_file_edited_here_that_the_remote_changed_stops_the_sync_before_it_writes() {
    let remote = remote();
    let a = clone(&remote);
    let before = a.head().unwrap().id;
    remote_commit(
        &remote,
        &[("projects/p.json", r#"{"$type": "lr/project/v1", "x": 1}"#)],
    );
    a.write("projects/p.json", Item::File("edited"));
    let (code, message) = refused(&a, true);
    assert_eq!(code, Code::SyncEdited);
    assert_eq!(
        message,
        "projects/p.json is edited here and changed on the remote"
    );
    assert_eq!(a.head().unwrap().id, before);
    assert_eq!(
        std::fs::read_to_string(a.workdir().join("projects/p.json")).unwrap(),
        "edited"
    );
}

#[test]
fn a_branch_without_an_upstream_is_refused() {
    let remote = remote();
    let a = clone(&remote);
    a.write(
        ".git/config",
        Item::Appended("[branch \"main\"]\n\tremote = \n"),
    );
    let (code, message) = refused(&a, true);
    assert_eq!(
        (code, message.as_str()),
        (Code::SyncNoRemote, "main follows no remote branch")
    );
}

#[test]
fn a_remote_git_cannot_reach_fails_the_sync_with_gits_stderr() {
    let remote = remote();
    let a = clone(&remote);
    a.unreachable_origin();
    let error = sync(&a, true).unwrap_err();
    assert_eq!(error.code(), Code::SyncFailed);
    assert!(
        error
            .to_string()
            .starts_with("git could not fetch origin (exit status"),
        "{error}"
    );
    let stderr = String::from_utf8(error.refusal().stderr.unwrap()).unwrap();
    assert!(stderr.contains("/nowhere/at/all"), "{stderr}");
}

#[test]
fn a_remote_that_refuses_every_push_gives_up_after_three_and_keeps_the_commits() {
    let remote = remote();
    remote.write(
        ".git/hooks/pre-receive",
        Item::Executable("#!/bin/sh\nexit 1\n"),
    );
    let a = clone(&remote);
    let from_a = put(&a, "/projects/a");
    let (code, message) = refused(&a, true);
    assert_eq!(code, Code::SyncGaveUp);
    assert_eq!(
        message,
        format!(
            "the remote refused the push {ATTEMPTS} times, the last time with [remote rejected] (pre-receive hook declined)"
        )
    );
    assert_eq!(a.head().unwrap().id, from_a);
    let synced = sync(&a, false).unwrap();
    assert_eq!(
        (synced.new, synced.pushed, synced.attempts),
        (from_a, false, 1)
    );
}

/// Makes `notes` in `clone` a symlink to `outside`, a directory outside the
/// clone, when `linked`, and a directory holding `kept.txt` otherwise.
fn notes(clone: &Scenario, outside: &Scenario, linked: bool) {
    if linked {
        if clone.workdir().join("notes").exists() {
            clone.write("notes", Item::Deleted);
        }
        clone.write("notes", Item::Symlink(outside.workdir().to_str().unwrap()));
    } else {
        clone.write("notes/kept.txt", Item::File("kept"));
    }
}

#[test]
fn a_file_the_remote_adds_beneath_a_symlinked_directory_stops_the_sync_and_one_beneath_a_directory_is_admitted()
 {
    for linked in [true, false] {
        let remote = remote();
        let a = clone(&remote);
        let outside = Scenario::new();
        notes(&a, &outside, linked);
        let before = a.head().unwrap().id;
        remote_commit(&remote, &[("notes/n.json", PROJECT)]);
        if linked {
            let (code, message) = refused(&a, true);
            assert_eq!(code, Code::PutSymlink);
            assert_eq!(
                message,
                "notes/n.json cannot be written: notes is a symlink"
            );
            assert_eq!(a.head().unwrap().id, before);
            assert!(!outside.workdir().join("n.json").exists());
        } else {
            sync(&a, true).unwrap();
            assert_eq!(
                std::fs::read_to_string(a.workdir().join("notes/n.json")).unwrap(),
                PROJECT
            );
        }
    }
}

#[test]
fn a_file_the_remote_turns_into_a_directory_is_replaced_and_a_symlinked_directory_is_refused() {
    for linked in [true, false] {
        let remote = remote();
        remote_commit(&remote, &[("notes", "plain")]);
        let a = clone(&remote);
        let outside = Scenario::new();
        if linked {
            notes(&a, &outside, true);
        }
        remote_commit(&remote, &[("notes/n.json", PROJECT)]);
        if linked {
            let (code, _) = refused(&a, true);
            assert_eq!(code, Code::PutSymlink);
            assert!(!outside.workdir().join("n.json").exists());
        } else {
            sync(&a, true).unwrap();
            assert_eq!(
                std::fs::read_to_string(a.workdir().join("notes/n.json")).unwrap(),
                PROJECT
            );
        }
    }
}

#[test]
fn a_file_the_remote_removes_beneath_a_symlinked_directory_stops_the_sync_and_one_beneath_a_directory_is_admitted()
 {
    for linked in [true, false] {
        let remote = remote();
        remote_commit(&remote, &[("notes/n.json", PROJECT)]);
        let a = clone(&remote);
        let outside = Scenario::new();
        outside.write("n.json", Item::File(PROJECT));
        notes(&a, &outside, linked);
        remote_commit(&remote, &[]);
        if linked {
            let (code, message) = refused(&a, true);
            assert_eq!(code, Code::PutSymlink);
            assert_eq!(
                message,
                "notes/n.json cannot be written: notes is a symlink"
            );
            assert!(outside.workdir().join("n.json").exists());
        } else {
            sync(&a, true).unwrap();
            assert!(!a.workdir().join("notes/n.json").exists());
        }
    }
}

#[test]
fn a_sync_beside_a_symlink_at_a_files_temporary_path_writes_the_file_and_leaves_the_symlinks_target_alone()
 {
    for planted in [true, false] {
        let remote = remote();
        let a = clone(&remote);
        let outside = Scenario::new();
        outside.write("victim.txt", Item::File("kept"));
        let victim = outside.workdir().join("victim.txt");
        if planted {
            a.write(
                "notes/.n.json.board-tmp",
                Item::Symlink(victim.to_str().unwrap()),
            );
        }
        remote_commit(&remote, &[("notes/n.json", PROJECT)]);
        sync(&a, true).unwrap();
        assert_eq!(
            std::fs::read_to_string(a.workdir().join("notes/n.json")).unwrap(),
            PROJECT
        );
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "kept");
    }
}

#[test]
fn a_file_the_remote_changes_that_is_a_symlink_on_disk_stops_the_sync_and_one_that_is_a_file_is_admitted()
 {
    for linked in [true, false] {
        let remote = remote();
        remote_commit(&remote, &[("notes/n.json", PROJECT)]);
        let a = clone(&remote);
        let outside = Scenario::new();
        outside.write("victim.json", Item::File(PROJECT));
        let victim = outside.workdir().join("victim.json");
        if linked {
            a.write("notes/n.json", Item::Deleted);
            a.write("notes/n.json", Item::Symlink(victim.to_str().unwrap()));
        }
        let changed = r#"{"$type": "lr/project/v1", "title": "changed"}"#;
        remote_commit(&remote, &[("notes/n.json", changed)]);
        if linked {
            let (code, message) = refused(&a, true);
            assert_eq!(code, Code::PutSymlink);
            assert_eq!(
                message,
                "notes/n.json cannot be written: notes/n.json is a symlink"
            );
            assert_eq!(std::fs::read_to_string(&victim).unwrap(), PROJECT);
        } else {
            sync(&a, true).unwrap();
            assert_eq!(
                std::fs::read_to_string(a.workdir().join("notes/n.json")).unwrap(),
                changed
            );
        }
    }
}

/// A sync holds no lock across its fetch: `clone`'s fetch stops at a
/// stand-in for ssh that writes `.git/delayed-ssh.started` and waits for
/// `.git/delayed-ssh.release`. Returns once a sync can be started.
fn holding_fetch(clone: &Scenario) {
    clone.delayed_origin(0);
    clone.write(
        ".git/delayed-ssh",
        Item::Executable(
            "#!/bin/sh\ntouch \"$0.started\"\nwhile [ ! -e \"$0.release\" ]; do sleep 0.05; done\n\
             for last; do :; done\nexec sh -c \"$last\"\n",
        ),
    );
}

/// Starts a sync of `clone` on another thread and waits until its fetch is
/// held open.
fn syncing_in_background(
    clone: &Scenario,
    push: bool,
) -> std::thread::JoinHandle<Result<Synced, SyncError>> {
    let dir = clone.workdir().to_path_buf();
    let started = dir.join(".git/delayed-ssh.started");
    let handle = std::thread::spawn(move || {
        let mut board = crate::Board::open(&dir, None).unwrap();
        board.built_by("test");
        board.sync(push, board_tree::GIT_DEADLINE)
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !started.exists() {
        assert!(std::time::Instant::now() < until, "the fetch never began");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    handle
}

fn release(clone: &Scenario) {
    clone.write(".git/delayed-ssh.release", Item::File(""));
}

#[test]
fn a_put_finishes_within_a_second_while_a_fetch_is_held_open_and_the_sync_keeps_it() {
    let remote = remote();
    let a = clone(&remote);
    remote_commit(&remote, &[("projects/r.json", PROJECT)]);
    holding_fetch(&a);
    let syncing = syncing_in_background(&a, true);
    let started = std::time::Instant::now();
    let made = put(&a, "/projects/during");
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(1), "{took:?}");
    release(&a);
    let synced = syncing.join().unwrap().unwrap();
    assert!(synced.pushed, "{synced:?}");
    assert_eq!(synced.old, Some(made));
    let head = a.head().unwrap();
    assert!(
        head.files.contains_key("projects/during.json")
            && head.files.contains_key("projects/r.json")
    );
    assert_eq!(remote.head().unwrap().id, synced.new);
}

#[test]
fn a_second_sync_while_one_runs_exits_at_once_and_one_after_it_is_admitted() {
    let remote = remote();
    let a = clone(&remote);
    holding_fetch(&a);
    let syncing = syncing_in_background(&a, false);
    let started = std::time::Instant::now();
    let (code, message) = refused(&a, false);
    let took = started.elapsed();
    assert_eq!(code, Code::SyncBusy);
    assert!(message.contains("sync.lock"), "{message}");
    assert!(took < std::time::Duration::from_secs(1), "{took:?}");
    release(&a);
    syncing.join().unwrap().unwrap();
    assert!(sync(&a, false).is_ok());
}

#[test]
fn a_rejected_push_fetches_again_and_the_second_push_succeeds() {
    let remote = remote();
    let refusal = remote.workdir().join("refused-once");
    remote.write(
        ".git/hooks/pre-receive",
        Item::Executable(&format!(
            "#!/bin/sh\n[ -e '{0}' ] && exit 0\ntouch '{0}'\nexit 1\n",
            refusal.display()
        )),
    );
    let a = clone(&remote);
    let made = put(&a, "/projects/a");
    let synced = sync(&a, true).unwrap();
    assert_eq!((synced.pushed, synced.attempts), (true, 2));
    assert_eq!(synced.new, made);
    assert_eq!(remote.head().unwrap().id, made);
}

/// A file edited during the push is not the sync's to keep: the push
/// comes after the working tree moved, so the edit stays beside the new
/// HEAD.
#[test]
fn a_file_edited_during_the_push_stays_as_edited() {
    let remote = remote();
    let a = clone(&remote);
    put(&a, "/projects/a");
    remote_commit(
        &remote,
        &[("projects/p.json", r#"{"$type": "lr/project/v1", "x": 1}"#)],
    );
    a.write(
        ".git/config",
        Item::Appended("[core]\n\thooksPath = .git/hooks\n"),
    )
    .write(
        ".git/hooks/pre-push",
        Item::Executable("#!/bin/sh\nprintf '%s' 'edited' > projects/p.json\n"),
    );
    let synced = sync(&a, true).unwrap();
    assert!(synced.pushed && synced.kept.is_empty(), "{synced:?}");
    assert_eq!(
        std::fs::read_to_string(a.workdir().join("projects/p.json")).unwrap(),
        "edited"
    );
}

/// A file the remote added that is edited after the sync compared it and
/// before it writes it is kept as edited. The edit is made while the sync
/// is held at an earlier file: a FIFO stands where its temporary file goes,
/// so opening it for writing waits for this test to read it.
#[test]
fn a_file_edited_inside_the_integrate_step_is_kept_and_reported() {
    let remote = remote();
    let a = clone(&remote);
    remote_commit(
        &remote,
        &[("projects/a.json", PROJECT), ("projects/z.json", PROJECT)],
    );
    a.write("projects/.a.json.board-tmp", Item::Fifo);
    let dir = a.workdir().to_path_buf();
    let syncing = std::thread::spawn({
        let dir = dir.clone();
        move || {
            let mut board = crate::Board::open(&dir, None).unwrap();
            board.built_by("test");
            board.sync(false, board_tree::GIT_DEADLINE)
        }
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !dir.join(".git/index.lock").exists() {
        assert!(std::time::Instant::now() < until, "the sync never began");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    a.write("projects/z.json", Item::File("edited"));
    let mut sink = Vec::new();
    std::io::Read::read_to_end(
        &mut std::fs::File::open(dir.join("projects/.a.json.board-tmp")).unwrap(),
        &mut sink,
    )
    .unwrap();
    let synced = syncing.join().unwrap().unwrap();
    assert_eq!(synced.kept, [Fields::at(&path("/projects/z"))]);
    assert_eq!(
        std::fs::read_to_string(dir.join("projects/z.json")).unwrap(),
        "edited"
    );
    assert_eq!(a.index()["projects/z.json"], a.blob(PROJECT));
    assert_eq!(a.head().unwrap().id, synced.new);
}

#[test]
fn a_second_sync_waits_for_the_first_within_its_wait_and_is_refused_busy_after_it() {
    let remote = remote();
    let a = clone(&remote);
    holding_fetch(&a);
    let syncing = syncing_in_background(&a, false);
    let dir = a.workdir().to_path_buf();
    let waiting = |wait| {
        let started = std::time::Instant::now();
        let mut board = crate::Board::open(&dir, None).unwrap();
        board.built_by("test");
        let result = board.sync_waiting(false, board_tree::GIT_DEADLINE, wait);
        (result, started.elapsed())
    };
    let (refused, took) = waiting(std::time::Duration::from_millis(300));
    assert_eq!(refused.unwrap_err().code(), Code::SyncBusy);
    assert!(took >= std::time::Duration::from_millis(300), "{took:?}");
    std::thread::scope(|scope| {
        let admitted = scope.spawn(|| waiting(std::time::Duration::from_secs(10)).0);
        std::thread::sleep(std::time::Duration::from_millis(200));
        release(&a);
        assert!(admitted.join().unwrap().is_ok());
    });
    syncing.join().unwrap().unwrap();
}
