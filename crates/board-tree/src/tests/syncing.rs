//! Syncing below a board: a clone with no commit yet, and git's answer to
//! a push (docs/architecture.md, Writes).

use super::*;
use crate::transport::rejected;
use std::time::{Duration, Instant};

use crate::{Deadline, GIT_DEADLINE, LOCK_WAIT, Synced};

#[test]
fn a_clone_without_a_commit_checks_out_the_remote_branch_without_pushing() {
    let remote = Scenario::new();
    let tip = remote.commit(&[("a.json", "{}"), ("d/b.json", "[]")]);
    let clone = Scenario::clone_of(&remote);
    clone.write(".git/refs/heads/main", Item::Deleted);
    clone.write(".git/index", Item::Deleted);
    clone.write("a.json", Item::Deleted);
    clone.write("d/b.json", Item::Deleted);
    let repository = clone.open();
    let synced = repository
        .sync(true, Deadline::after(GIT_DEADLINE), Duration::ZERO)
        .unwrap();
    assert_eq!(
        synced,
        Synced {
            upstream: repository.upstream().unwrap(),
            fetched: tip,
            old: None,
            new: tip,
            replayed: Vec::new(),
            pushed: false,
            attempts: 1,
            kept: Vec::new(),
        }
    );
    assert_eq!(clone.head().unwrap().id, tip);
    assert_eq!(clone.index().len(), 2);
    assert_eq!(
        std::fs::read_to_string(clone.workdir().join("d/b.json")).unwrap(),
        "[]"
    );
}

/// An index that cannot be read stands in for one another writer is
/// rewriting: a sync that read it before taking the lock fails on it.
#[test]
fn a_held_index_lock_stops_a_sync_before_the_index_is_read_and_a_free_one_is_admitted() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    let before = clone.head().unwrap().id;
    remote.commit(&[("a.json", "{}"), ("b.json", "[]")]);
    // git fetch reads the index only to find submodules to fetch.
    clone
        .write(
            ".git/config",
            Item::Appended("[fetch]\n\trecurseSubmodules = false\n"),
        )
        .write(".git/index", Item::File("not an index"))
        .write(".git/index.lock", Item::File(""));
    let repository = clone.open();
    let error = repository
        .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
        .unwrap_err();
    assert_eq!(error.code(), Code::PutLocked, "{error}");
    assert_eq!(clone.head().unwrap().id, before);
    clone
        .write(".git/index", Item::Deleted)
        .write(".git/index.lock", Item::Deleted);
    assert!(
        repository
            .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
            .is_ok()
    );
}

#[test]
fn a_push_git_reports_as_rejected_names_the_reason_and_a_failed_one_has_none() {
    let refused = b"To /remote\n!\tabc123:refs/heads/main\t[rejected] (fetch first)\nDone\n";
    assert_eq!(
        rejected(refused),
        Some("[rejected] (fetch first)".to_owned())
    );
    assert_eq!(rejected(b"fatal: could not read from remote\n"), None);
}

/// A sync removes a file the remote deleted, is done when it is already
/// gone, and names the path it cannot remove.
#[test]
fn removing_a_file_is_done_when_it_is_gone_and_names_a_path_it_cannot_remove() {
    let scenario = Scenario::new();
    scenario.write("a.json", Item::File("{}"));
    scenario.write("d/b.json", Item::File("[]"));
    let repository = scenario.open();
    let lock = repository.lock(LOCK_WAIT).unwrap();
    repository.remove_file(&lock, "a.json").unwrap();
    assert!(!scenario.workdir().join("a.json").exists());
    repository.remove_file(&lock, "a.json").unwrap();
    let refused = repository.remove_file(&lock, "d").unwrap_err();
    assert!(
        matches!(&refused, crate::WriteError::Io { path, .. } if path.ends_with("d")),
        "{refused:?}"
    );
    assert!(scenario.workdir().join("d/b.json").exists());
}

/// A file the remote deleted leaves the working tree and the index, and
/// the other files stay in both.
#[test]
fn a_sync_removes_a_file_the_remote_deleted_and_keeps_the_rest() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}"), ("d/b.json", "[]")]);
    let clone = Scenario::clone_of(&remote);
    let tip = remote.commit(&[("d/b.json", "[]")]);
    let repository = clone.open();
    assert_eq!(
        repository
            .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
            .unwrap()
            .new,
        tip
    );
    assert!(!clone.workdir().join("a.json").exists());
    assert!(clone.workdir().join("d/b.json").exists());
    assert_eq!(
        clone.index().into_keys().collect::<Vec<_>>(),
        vec!["d/b.json".to_owned()]
    );
}

/// A path the sync must check but cannot read stops it, naming the path.
#[test]
fn a_sync_stops_at_a_path_it_cannot_read() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    remote.commit(&[("a.json", "{}"), ("d", "x")]);
    clone.write("d/x.json", Item::File("{}"));
    let repository = clone.open();
    let refused = repository
        .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
        .unwrap_err();
    assert!(
        matches!(&refused, crate::SyncError::Git(crate::git::GitError::File { path, .. }) if path.ends_with("d")),
        "{refused:?}"
    );
    assert!(clone.workdir().join("d/x.json").exists());
}

/// A sync that a write refuses names the path the write names.
#[test]
fn a_sync_a_write_refuses_names_the_writes_path() {
    let path: LogicalPath = "/x/y".parse().unwrap();
    let refused = crate::SyncError::Write(WriteError::Symlink {
        path: path.clone(),
        symlink: "x".to_owned(),
    });
    assert_eq!(refused.fields(), board_model::Fields::at(&path));
}

/// A remote that has not answered when the deadline passes stops the sync
/// with `board::sync::timeout`: Board stops git and returns soon after the
/// deadline, so the caller lets the lock go. The same remote answering
/// within the deadline syncs.
#[test]
fn a_remote_that_stalls_past_the_deadline_times_out_and_one_that_answers_syncs() {
    let deadline = Duration::from_secs(5);
    let sync = |delay| {
        let remote = Scenario::new();
        remote.commit(&[("a.json", "{}")]);
        let clone = Scenario::clone_of(&remote);
        let tip = remote.commit(&[("a.json", "[]")]);
        clone.delayed_origin(delay);
        let repository = clone.open();
        let started = Instant::now();
        let synced = repository.sync(false, Deadline::after(deadline), Duration::ZERO);
        let took = started.elapsed();
        (synced, took, tip)
    };
    let (refused, took, _) = sync(30);
    let refused = refused.unwrap_err();
    assert_eq!(refused.code(), board_model::Code::SyncTimeout, "{refused}");
    assert!(took < Duration::from_secs(10), "{took:?}");
    let (admitted, _, tip) = sync(0);
    assert_eq!(admitted.unwrap().new, tip);
}

/// The ids of the stand-in for ssh and of the `sleep` it starts, which it
/// writes to `.git/delayed-ssh.pid` before it stalls for `delay` seconds.
fn recording_origin(clone: &Scenario, delay: u32) {
    clone.delayed_origin(delay);
    let script = format!(
        "#!/bin/sh\necho $$ > \"$0.pid\"\nsleep {delay} &\necho $! >> \"$0.pid\"\nwait\n\
         for last; do :; done\nexec sh -c \"$last\"\n"
    );
    clone.write(".git/delayed-ssh", Item::Executable(&script));
}

/// The two ids the stand-in wrote to `path`, waiting up to ten seconds for
/// both lines, since a loaded machine may start it late.
fn recorded(path: &std::path::Path) -> Vec<i32> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let ids: Vec<i32> = text.lines().filter_map(|id| id.parse().ok()).collect();
        if (ids.len() == 2 && text.ends_with('\n')) || Instant::now() > until {
            assert_eq!(ids.len(), 2, "the stand-in wrote {text:?}");
            return ids;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether the process `id` still exists, as `kill -0` answers.
fn alive(id: i32) -> bool {
    rustix::process::Pid::from_raw(id)
        .is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok())
}

/// A sync stopped at its deadline stops git's whole process group: the
/// stand-in for ssh git started and the `sleep` that started are gone too,
/// so a stalled remote leaves nothing running.
#[test]
fn a_sync_past_its_deadline_leaves_nothing_the_stalled_remote_started_running() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    remote.commit(&[("a.json", "[]")]);
    recording_origin(&clone, 30);
    let repository = clone.open();
    let refused = repository
        .sync(
            false,
            Deadline::after(Duration::from_secs(1)),
            Duration::ZERO,
        )
        .unwrap_err();
    assert_eq!(refused.code(), Code::SyncTimeout, "{refused}");
    let ids = recorded(&clone.workdir().join(".git/delayed-ssh.pid"));
    let until = Instant::now() + Duration::from_secs(5);
    while ids.iter().any(|&id| alive(id)) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let running: Vec<_> = ids.iter().filter(|&&id| alive(id)).collect();
    assert_eq!(running, Vec::<&i32>::new(), "still running after the sync");
}

/// One deadline covers a sync's fetch and its push. A fetch that takes four
/// seconds leaves a six-second sync too little for a push that also takes
/// four, so the push is stopped and the remote keeps its commit; a
/// twenty-second sync, otherwise the same, pushes. Each margin is two
/// seconds or more, for a loaded machine.
#[test]
fn a_push_after_a_slow_fetch_is_stopped_by_what_is_left_of_the_deadline_and_a_longer_one_pushes() {
    let push = |deadline| {
        let remote = Scenario::new();
        let first = remote.commit(&[("a.json", "{}")]);
        remote.accepting_pushes();
        let clone = Scenario::clone_of(&remote);
        let made = clone.commit(&[("a.json", "[]")]);
        clone.write("a.json", Item::File("[]")).track(made);
        clone.delayed_origin(4);
        let repository = clone.open();
        let synced = repository.sync(true, Deadline::after(deadline), Duration::ZERO);
        (synced, remote.head().unwrap().id, first, made)
    };
    let (refused, tip, first, _) = push(Duration::from_secs(6));
    let refused = refused.unwrap_err();
    assert!(
        matches!(
            &refused,
            crate::SyncError::TimedOut {
                operation: "push",
                ..
            }
        ),
        "{refused}"
    );
    assert!(
        refused.to_string().contains("of its 6 seconds"),
        "{refused}"
    );
    assert_eq!(tip, first);
    let (pushed, tip, _, made) = push(Duration::from_secs(20));
    assert!(pushed.unwrap().pushed);
    assert_eq!(tip, made);
}

/// A stand-in for OpenSSH that records its arguments and git's
/// `GIT_TERMINAL_PROMPT`, then, when `.git/needs-prompt` exists, fails as
/// ssh does in batch mode, or waits as ssh asking for a passphrase does.
/// Without that file it reaches the remote.
const ASKING: &str = "#!/bin/sh\nprintf '%s\\n' \"$*\" \"GIT_TERMINAL_PROMPT=$GIT_TERMINAL_PROMPT\" \
                      > \"$0.seen\"\nif [ -e \"$0.prompt\" ]; then\n  case \" $* \" in\n    \
                      *' BatchMode=yes '*) echo 'Permission denied (publickey).' >&2; exit 255 ;;\n  \
                      esac\n  sleep 30\nfi\nfor last; do :; done\nexec sh -c \"$last\"\n";

/// A clone whose origin is reached over the recording stand-in for
/// OpenSSH, set as the owner's `core.sshCommand`, and whose remote moved.
fn asking(prompts: bool) -> (Scenario, Scenario, board_model::ObjectId) {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    let tip = remote.commit(&[("a.json", "[]")]);
    clone.delayed_origin(0);
    let config = std::fs::read_to_string(clone.workdir().join(".git/config")).unwrap();
    let config = config.replace("variant = simple", "variant = ssh");
    clone.write(".git/config", Item::File(&config));
    clone.write(".git/delayed-ssh", Item::Executable(ASKING));
    if prompts {
        clone.write(".git/delayed-ssh.prompt", Item::File(""));
    }
    (remote, clone, tip)
}

/// ssh runs as the owner set it, with `-o BatchMode=yes` added, and git with
/// `GIT_TERMINAL_PROMPT=0`. A remote that would ask for a passphrase fails
/// at once with `board::sync::failed`, whose hint names ssh-agent, rather
/// than waiting for the deadline; the same remote asking nothing syncs.
#[test]
fn a_remote_that_needs_a_prompt_fails_at_once_and_one_that_asks_nothing_syncs() {
    let sync = |prompts| {
        let (_remote, clone, tip) = asking(prompts);
        let repository = clone.open();
        let started = Instant::now();
        let synced = repository.sync(
            false,
            Deadline::after(Duration::from_secs(20)),
            Duration::ZERO,
        );
        let seen = std::fs::read_to_string(clone.workdir().join(".git/delayed-ssh.seen")).unwrap();
        (synced, started.elapsed(), seen, tip)
    };
    let (refused, took, seen, _) = sync(true);
    let refused = refused.unwrap_err();
    assert_eq!(refused.code(), Code::SyncFailed, "{refused}");
    assert!(took < Duration::from_secs(10), "{took:?}");
    assert!(
        refused
            .code()
            .hint(&board_model::Fields::default())
            .contains("ssh-agent"),
        "{refused}"
    );
    let mut lines = seen.lines();
    let arguments = lines.next().unwrap();
    assert!(arguments.starts_with("-o BatchMode=yes "), "{arguments}");
    assert_eq!(lines.next(), Some("GIT_TERMINAL_PROMPT=0"));
    let (synced, _, seen, tip) = sync(false);
    assert_eq!(synced.unwrap().new, tip);
    assert!(seen.starts_with("-o BatchMode=yes "), "{seen}");
}

/// The command git runs for ssh keeps the owner's, and adds `BatchMode` only
/// for OpenSSH: not for a `simple` variant or for `PuTTY`'s `plink`.
#[test]
fn the_ssh_command_keeps_the_owners_and_adds_batch_mode_only_for_openssh() {
    let ssh = |config: &str| {
        let scenario = Scenario::new();
        scenario.write(".git/config", Item::Appended(config));
        scenario.open().ssh()
    };
    let command = |text: &str| crate::transport::Ssh(text.to_owned());
    assert_eq!(ssh(""), command("ssh -o BatchMode=yes"));
    assert_eq!(
        ssh("[core]\n\tsshCommand = ssh -i ~/.ssh/work\n"),
        command("ssh -i ~/.ssh/work -o BatchMode=yes")
    );
    assert_eq!(
        ssh("[core]\n\tsshCommand = /bin/my-ssh\n[ssh]\n\tvariant = simple\n"),
        command("/bin/my-ssh")
    );
    assert_eq!(
        ssh("[core]\n\tsshCommand = plink -batch\n"),
        command("plink -batch")
    );
}

/// A `sync.lock` file left by a process that was killed holds nothing: the
/// kernel dropped its lock with the process.
#[test]
fn a_sync_lock_file_left_by_a_dead_process_does_not_make_the_next_sync_busy() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    clone.write(".git/sync.lock", Item::File("999999\n"));
    let synced = clone
        .open()
        .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
        .unwrap();
    assert_eq!(synced.attempts, 1);
}

/// A lock held by a live sync refuses a second at once and is admitted
/// once the first lets go.
#[test]
fn a_held_sync_lock_refuses_a_second_sync_busy_until_it_is_released() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    let held = clone.flocked(".git/sync.lock");
    let busy = clone
        .open()
        .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
        .unwrap_err();
    assert!(matches!(busy, crate::SyncError::Busy { .. }), "{busy:?}");
    drop(held);
    assert!(
        clone
            .open()
            .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
            .is_ok()
    );
}

/// A board lock held past a spent deadline is the sync timing out.
#[test]
fn a_sync_whose_deadline_is_spent_while_a_put_holds_the_board_lock_times_out() {
    let remote = Scenario::new();
    remote.commit(&[("a.json", "{}")]);
    let clone = Scenario::clone_of(&remote);
    let repository = clone.open();
    let _held = repository.lock(LOCK_WAIT).unwrap();
    let error = repository
        .sync(false, Deadline::after(Duration::ZERO), Duration::ZERO)
        .unwrap_err();
    assert_eq!(error.code(), Code::SyncTimeout, "{error}");
}
