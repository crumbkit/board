use std::time::Duration;

use board_model::Code;
use board_scenario::{Item, Scenario};

use super::super::Synced;
use super::*;
use crate::{Deadline, GIT_DEADLINE};

/// A remote that takes pushes and two clones of it, each able to commit,
/// with an index that matches HEAD.
fn pair(files: &[(&str, &str)]) -> (Scenario, Scenario, Scenario) {
    let remote = Scenario::new();
    remote.identified().accepting_pushes();
    remote.commit(files);
    let one = Scenario::clone_of(&remote);
    let two = Scenario::clone_of(&remote);
    one.identified();
    two.identified();
    (remote, one, two)
}

/// Commits `files` in `clone`, writes them and stages them, as a put does.
fn commit_here(clone: &Scenario, files: &[(&str, &str)]) -> ObjectId {
    let before: Vec<String> = clone.head().unwrap().files.into_keys().collect();
    let id = clone.commit(files);
    for path in before {
        if !files.iter().any(|(kept, _)| *kept == path) {
            clone.write(&path, Item::Deleted);
        }
    }
    for (path, text) in files {
        clone.write(path, Item::File(text));
    }
    clone.track(id);
    id
}

/// Syncs `clone` guarded on `main`, answering `answer` before each push,
/// and gives the result with every outgoing range the guard was asked.
fn guarded(clone: &Scenario, answer: BeforePush) -> (Result<Synced, SyncError>, Vec<Pushing>) {
    let mut asked = Vec::new();
    let mut before_push = |outgoing: &Pushing| {
        asked.push(*outgoing);
        answer
    };
    let mut guard = Guard {
        branch: "main".to_owned(),
        before_push: &mut before_push,
    };
    let synced = Repository::open(clone.workdir()).unwrap().sync_guarded(
        true,
        &mut guard,
        Deadline::after(GIT_DEADLINE),
        Duration::ZERO,
    );
    (synced, asked)
}

fn remote_head(remote: &Scenario) -> ObjectId {
    remote.head().unwrap().id
}

#[test]
fn a_guarded_sync_pushes_what_its_guard_admits_and_holds_what_it_does_not() {
    let (remote, one, _) = pair(&[("a.json", "0")]);
    let fetched = remote_head(&remote);
    let new = commit_here(&one, &[("a.json", "1")]);
    let (held, asked) = guarded(&one, BeforePush::Hold);
    let error = held.unwrap_err();
    assert_eq!(error.code(), Code::SyncHeld, "{error}");
    assert_eq!(
        asked,
        [Pushing {
            fetched: Some(fetched),
            new
        }]
    );
    assert_eq!(remote_head(&remote), fetched);
    let (pushed, _) = guarded(&one, BeforePush::Push);
    assert!(pushed.unwrap().pushed);
    assert_eq!(remote_head(&remote), new);
}

#[test]
fn a_guarded_sync_asks_before_each_push_after_a_rejected_one() {
    let (remote, one, two) = pair(&[("a.json", "0")]);
    commit_here(&one, &[("a.json", "0"), ("b.json", "1")]);
    commit_here(&two, &[("a.json", "0"), ("c.json", "2")]);
    let mut rounds = 0;
    let mut before_push = |outgoing: &Pushing| {
        rounds += 1;
        // The other clone pushes while this one waits on its guard, once.
        if rounds == 1 {
            let (_, _) = guarded(&two, BeforePush::Push);
        }
        let _ = outgoing;
        BeforePush::Push
    };
    let mut guard = Guard {
        branch: "main".to_owned(),
        before_push: &mut before_push,
    };
    let synced = Repository::open(one.workdir())
        .unwrap()
        .sync_guarded(
            true,
            &mut guard,
            Deadline::after(GIT_DEADLINE),
            Duration::ZERO,
        )
        .unwrap();
    assert!(synced.pushed);
    assert_eq!(synced.attempts, 2);
    assert_eq!(rounds, 2);
    let files = remote.head().unwrap().files;
    assert!(files.contains_key("b.json") && files.contains_key("c.json"));
}

#[test]
fn a_guarded_sync_asks_before_it_starts_a_remote_branch_naming_no_fetched_commit() {
    let (remote, one, _) = pair(&[("a.json", "0")]);
    let tip = remote_head(&one);
    // The clone follows a branch the remote does not have.
    one.write(
        ".git/config",
        Item::Appended("[branch \"main\"]\n\tmerge = refs/heads/fresh\n"),
    );
    one.write(".git/refs/remotes/origin/main", Item::Deleted);
    let (held, asked) = guarded(&one, BeforePush::Hold);
    assert_eq!(held.unwrap_err().code(), Code::SyncHeld);
    assert_eq!(
        asked,
        [Pushing {
            fetched: None,
            new: tip
        }]
    );
    let _ = remote;
}

#[test]
fn a_guarded_sync_refuses_another_branch_before_fetching_and_its_twin_on_main_syncs() {
    let (remote, one, _) = pair(&[("a.json", "0")]);
    let tip = remote.commit(&[("a.json", "1")]);
    let head = remote_head(&one);
    one.write(".git/HEAD", Item::File("ref: refs/heads/other\n"));
    one.write(".git/refs/heads/other", Item::File(&format!("{head}\n")));
    let (refused, asked) = guarded(&one, BeforePush::Push);
    let error = refused.unwrap_err();
    assert_eq!(error.code(), Code::SyncOtherBranch, "{error}");
    assert_eq!(asked, Vec::<Pushing>::new());
    let tracking = std::fs::read_to_string(one.workdir().join(".git/refs/remotes/origin/main"));
    assert_ne!(tracking.unwrap().trim(), tip.to_string());
    one.write(".git/HEAD", Item::File("ref: refs/heads/main\n"));
    let (synced, _) = guarded(&one, BeforePush::Push);
    assert_eq!(synced.unwrap().new, tip);
}

#[test]
fn a_guarded_sync_refuses_a_staged_change_and_leaves_it_and_its_twin_syncs() {
    let (remote, one, _) = pair(&[("a.json", "0")]);
    let tip = remote.commit(&[("a.json", "0"), ("b.json", "1")]);
    let before = one.head().unwrap().id;
    let staged = one.commit(&[("a.json", "9")]);
    one.track(staged);
    one.write(".git/refs/heads/main", Item::File(&format!("{before}\n")));
    let index = one.index();
    let (refused, _) = guarded(&one, BeforePush::Push);
    let error = refused.unwrap_err();
    assert_eq!(error.code(), Code::SyncStaged, "{error}");
    assert!(error.to_string().contains("a.json"), "{error}");
    assert_eq!(one.index(), index);
    assert_eq!(one.head().unwrap().id, before);
    one.track(before);
    let (synced, _) = guarded(&one, BeforePush::Push);
    assert_eq!(synced.unwrap().new, tip);
}

#[test]
fn a_guarded_sync_refuses_a_merge_in_progress_before_fetching() {
    let (_remote, one, _) = pair(&[("a.json", "0")]);
    let head = remote_head(&one);
    one.write(".git/MERGE_HEAD", Item::File(&format!("{head}\n")));
    let (refused, _) = guarded(&one, BeforePush::Push);
    assert_eq!(refused.unwrap_err().code(), Code::SyncMerging);
    one.write(".git/MERGE_HEAD", Item::Deleted);
    assert!(guarded(&one, BeforePush::Push).0.is_ok());
}

#[test]
fn a_guarded_sync_refuses_a_conflict_moving_nothing_and_its_twin_without_one_merges() {
    let (remote, one, two) = pair(&[("a.json", "0")]);
    commit_here(&one, &[("a.json", "1")]);
    guarded(&one, BeforePush::Push).0.unwrap();
    let pushed = remote_head(&remote);
    let mine = commit_here(&two, &[("a.json", "2")]);
    let tracking = || {
        std::fs::read_to_string(two.workdir().join(".git/refs/remotes/origin/main"))
            .unwrap()
            .trim()
            .to_owned()
    };
    let tracked = tracking();
    let (refused, asked) = guarded(&two, BeforePush::Push);
    let error = refused.unwrap_err();
    assert_eq!(error.code(), Code::SyncConflicted, "{error}");
    assert!(error.to_string().contains("a.json"), "{error}");
    assert_eq!(asked, Vec::<Pushing>::new());
    assert_eq!(two.head().unwrap().id, mine);
    assert_eq!(
        std::fs::read_to_string(two.workdir().join("a.json")).unwrap(),
        "2"
    );
    assert_eq!(two.index()["a.json"], two.blob("2"));
    assert!(
        !(two.head().unwrap().files.keys()).any(|path| path.starts_with("conflicts/")),
        "no conflict record is written"
    );
    assert_eq!(tracking(), tracked, "the tracking ref is put back");
    assert_eq!(remote_head(&remote), pushed);
    // The twin: the same clones editing different files merge and push.
    let (remote, one, two) = pair(&[("a.json", "0"), ("b.json", "0")]);
    commit_here(&one, &[("a.json", "1"), ("b.json", "0")]);
    guarded(&one, BeforePush::Push).0.unwrap();
    commit_here(&two, &[("a.json", "0"), ("b.json", "2")]);
    let synced = guarded(&two, BeforePush::Push).0.unwrap();
    assert!(synced.merged && synced.pushed);
    assert_eq!(remote_head(&remote), synced.new);
}

#[test]
fn a_guarded_sync_refuses_a_pin_neither_side_descends_from_and_its_twin_in_a_line_merges() {
    let mounted = Scenario::new();
    let base = mounted.commit_at(&[("f", "0")], &[], 1, "base");
    let left = mounted.commit_at(&[("f", "1")], &[base], 2, "left");
    let right = mounted.commit_at(&[("f", "2")], &[base], 3, "right");
    let after = mounted.commit_at(&[("f", "3")], &[left], 4, "after");
    let pin = |commit| board_scenario::seed::mount_of(&mounted.path(), commit);
    let sides = |local: ObjectId, remote: ObjectId| {
        let (remote_repo, one, two) = pair(&[("mount.json", &pin(base))]);
        commit_here(&one, &[("mount.json", &pin(remote))]);
        guarded(&one, BeforePush::Push).0.unwrap();
        let mine = commit_here(&two, &[("mount.json", &pin(local))]);
        let synced = guarded(&two, BeforePush::Push).0;
        (synced, mine, two, remote_repo)
    };
    let (refused, mine, two, _) = sides(left, right);
    let error = refused.unwrap_err();
    assert_eq!(error.code(), Code::SyncConflicted, "{error}");
    assert!(error.to_string().contains("mount.json"), "{error}");
    assert_eq!(two.head().unwrap().id, mine);
    let (synced, _, _, remote) = sides(left, after);
    let synced = synced.unwrap();
    assert!(synced.pushed);
    assert_eq!(remote_head(&remote), synced.new);
}
