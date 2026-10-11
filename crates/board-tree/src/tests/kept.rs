//! Handing the files a walk reads in full to the walking thread while the
//! other threads hash, with a bounded number held between them.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::*;
use crate::GitError;
use crate::worktree::KEPT_AHEAD;

/// How a walk's `keep` failed in a test. These walks read no file that
/// fails, so the walk's own error is a broken fixture.
#[derive(Debug)]
enum Failed {
    Kept,
}

impl From<GitError> for Failed {
    fn from(error: GitError) -> Self {
        panic!("the walk fails: {error}")
    }
}

/// A board of four times as many JSON files as a walk holds ahead of
/// `keep`, opened to hash on four threads, so the hashing threads fill the
/// channel and wait on it.
fn many_files() -> (Scenario, usize) {
    let scenario = Scenario::new();
    let files = 4 * KEPT_AHEAD;
    for n in 0..files {
        scenario.write(&format!("r{n}.json"), Item::File("{}"));
    }
    (scenario, files)
}

fn on_four_threads(scenario: &Scenario) -> Repository {
    let repository = scenario.open();
    repository.set_threads(NonZeroUsize::new(4).unwrap());
    repository
}

/// Runs `walk` on another thread and returns how it ended; it fails if the
/// walk has not ended within ten seconds.
fn ended<R: Send + 'static>(walk: impl FnOnce() -> R + Send + 'static) -> thread::Result<R> {
    let (done, finished) = mpsc::channel::<()>();
    let walker = thread::spawn(move || {
        let _done = done;
        walk()
    });
    assert_eq!(
        finished.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected),
        "the walk ends"
    );
    walker.join()
}

#[test]
fn a_walk_keeps_every_file_and_holds_no_more_unkept_than_its_bound_and_one_per_thread() {
    let (scenario, files) = many_files();
    let (kept, most) = ended(move || {
        let held = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let mut kept = Vec::new();
        on_four_threads(&scenario)
            .walk_working_tree(
                &mut |_, _| None,
                &|path, _| {
                    let now = held.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    path.to_owned()
                },
                &mut |path| {
                    held.fetch_sub(1, Ordering::SeqCst);
                    kept.push(path);
                    Ok::<(), GitError>(())
                },
            )
            .unwrap();
        (kept, most.into_inner())
    })
    .unwrap();
    assert_eq!(kept.len(), files, "every file is kept once");
    assert!(
        most <= KEPT_AHEAD + 4 + 1,
        "{most} files were prepared and not yet kept at once"
    );
}

#[test]
fn a_keep_that_fails_ends_the_walk_with_its_error_and_each_thread_stops_at_its_next_prepared_file()
{
    let (scenario, files) = many_files();
    let ended = ended(move || {
        let repository = on_four_threads(&scenario);
        let prepared = AtomicUsize::new(0);
        let walked = repository.walk_working_tree(
            &mut |_, _| None,
            &|_, _| {
                prepared.fetch_add(1, Ordering::SeqCst);
            },
            &mut |()| Err(Failed::Kept),
        );
        (walked, prepared.into_inner())
    });
    let (walked, prepared) = ended.unwrap();
    assert!(matches!(walked, Err(Failed::Kept)), "{walked:?}");
    // Each thread prepares at most one file after the channel closes: the
    // one it was hashing.
    assert!(
        prepared <= 1 + KEPT_AHEAD + 4,
        "{prepared} of {files} files were read after keep failed"
    );
}

#[test]
fn a_keep_that_panics_ends_the_walk_with_its_panic() {
    let (scenario, _) = many_files();
    let ended = ended(move || {
        on_four_threads(&scenario).walk_working_tree(
            &mut |_, _| None,
            &|_, _| {},
            &mut |()| -> Result<(), Failed> { panic!("keep panics") },
        )
    });
    let payload = ended.unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"keep panics"));
}

#[test]
fn a_thread_that_panics_while_preparing_ends_the_walk_with_its_panic_while_the_others_hash() {
    let (scenario, _) = many_files();
    let ended = ended(move || {
        on_four_threads(&scenario).walk_working_tree(
            &mut |_, _| None,
            &|path, _| assert_ne!(path, "r7.json", "preparing panics"),
            &mut |()| Ok::<(), Failed>(()),
        )
    });
    let payload = ended.unwrap_err();
    let message = payload.downcast_ref::<String>().unwrap();
    assert!(message.contains("preparing panics"), "{message}");
}

#[test]
fn a_keep_that_fails_on_a_record_read_while_listing_is_not_called_again() {
    // `a.json` beside `a/` is read while the walk lists, before the other
    // files are hashed on the threads.
    let (scenario, _) = many_files();
    scenario
        .write("a.json", Item::File("{}"))
        .write("a/b.json", Item::File("{}"));
    let ended = ended(move || {
        let mut calls = Vec::new();
        let walked = on_four_threads(&scenario).walk_working_tree(
            &mut |_, _| None,
            &|path, _| path.to_owned(),
            &mut |path| {
                calls.push(path);
                Err(Failed::Kept)
            },
        );
        (walked, calls)
    });
    let (walked, calls) = ended.unwrap();
    assert!(matches!(walked, Err(Failed::Kept)), "{walked:?}");
    assert_eq!(calls, ["a.json"]);
}

#[test]
fn a_keep_that_takes_a_record_read_while_listing_is_called_for_every_file() {
    let (scenario, files) = many_files();
    scenario
        .write("a.json", Item::File("{}"))
        .write("a/b.json", Item::File("{}"));
    let calls = ended(move || {
        let mut calls = Vec::new();
        on_four_threads(&scenario)
            .walk_working_tree(&mut |_, _| None, &|path, _| path.to_owned(), &mut |path| {
                calls.push(path);
                Ok::<(), Failed>(())
            })
            .unwrap();
        calls
    })
    .unwrap();
    assert_eq!(calls[0], "a.json");
    assert_eq!(calls.len(), files + 2);
}
