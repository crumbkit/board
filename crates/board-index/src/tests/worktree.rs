//! The racy-clean rule, and the working tree a refresh stores.

use board_model::Stat;
use board_tree::{Entry, Status};
use proptest::prelude::*;

use super::*;
use crate::store::trusted;

const SECOND: i64 = 1_000_000_000;

/// When the second holding `ns` began.
fn second_of(ns: i64) -> i64 {
    ns - ns.rem_euclid(SECOND)
}

fn stat_with(size: u64, modified_ns: i64, changed_ns: i64, inode: u64, mode: u32) -> Stat {
    Stat {
        size,
        modified_ns,
        changed_ns,
        inode,
        mode,
    }
}

fn stat() -> impl Strategy<Value = Stat> {
    (
        any::<u64>(),
        -4 * SECOND..4 * SECOND,
        any::<i64>(),
        any::<u64>(),
        any::<u32>(),
    )
        .prop_map(|(size, modified, changed, inode, mode)| {
            stat_with(size, modified, changed, inode, mode)
        })
}

/// `stat` with field `field` (0 to 4) changed.
fn changed(stat: Stat, field: u8) -> Stat {
    let mut other = stat;
    match field {
        0 => other.size ^= 1,
        1 => other.modified_ns ^= 1,
        2 => other.changed_ns ^= 1,
        3 => other.inode ^= 1,
        _ => other.mode ^= 1,
    }
    other
}

proptest! {
    #[test]
    fn a_file_modified_in_a_second_before_the_refresh_began_is_trusted(
        refreshed in -(1_i64 << 62)..(1_i64 << 62),
        earlier in 1..(1_i64 << 40),
        seen in stat(),
    ) {
        let seen = Stat { modified_ns: second_of(refreshed) - earlier, ..seen };
        prop_assert!(trusted(Some(&seen), &seen, Some(refreshed)));
    }

    #[test]
    fn a_file_modified_in_or_after_the_second_the_refresh_began_is_not_trusted(
        refreshed in -(1_i64 << 62)..(1_i64 << 62),
        later in 0..(1_i64 << 40),
        seen in stat(),
    ) {
        let seen = Stat { modified_ns: second_of(refreshed) + later, ..seen };
        prop_assert!(!trusted(Some(&seen), &seen, Some(refreshed)));
    }

    #[test]
    fn a_file_whose_stat_changed_is_not_trusted_and_its_twin_whose_stat_did_not_is(
        seen in stat(),
        field in 0_u8..5,
    ) {
        let refreshed = Some(5 * SECOND);
        prop_assert!(!trusted(Some(&changed(seen, field)), &seen, refreshed));
        prop_assert!(trusted(Some(&seen), &seen, refreshed));
    }

    #[test]
    fn a_file_with_no_stored_stat_or_no_refresh_is_not_trusted_and_its_twin_is(seen in stat()) {
        prop_assert!(!trusted(None, &seen, Some(5 * SECOND)));
        prop_assert!(!trusted(Some(&seen), &seen, None));
        prop_assert!(trusted(Some(&seen), &seen, Some(5 * SECOND)));
    }
}

/// Whether a file modified at `modified_ns` is trusted after a refresh that
/// began half a second into its tenth second.
fn trusted_at(modified_ns: i64) -> bool {
    let seen = stat_with(1, modified_ns, 0, 1, 0o100_644);
    trusted(Some(&seen), &seen, Some(10 * SECOND + SECOND / 2))
}

#[test]
fn a_file_modified_in_the_second_the_refresh_began_is_not_trusted() {
    assert!(!trusted_at(10 * SECOND));
    assert!(!trusted_at(10 * SECOND + SECOND / 2));
}

#[test]
fn its_twin_modified_in_the_second_before_is_trusted() {
    assert!(trusted_at(10 * SECOND - 1));
    assert!(trusted_at(9 * SECOND));
}

#[test]
fn the_extreme_times_compare_without_overflow() {
    let first = stat_with(0, i64::MIN, 0, 0, 0);
    let last = stat_with(0, i64::MAX, 0, 0, 0);
    assert!(trusted(Some(&first), &first, Some(i64::MAX)));
    assert!(!trusted(Some(&last), &last, Some(i64::MIN)));
}

/// A file at `path` holding `blob(byte)`, last modified at `modified_ns`.
fn file(path: &str, byte: u8, modified_ns: i64) -> Entry {
    Entry {
        path: path.into(),
        logical: None,
        blob: Some(blob(byte)),
        status: Status::Ok,
        stat: Some(stat_with(1, modified_ns, 0, 1, 0o100_644)),
        size: Some(1),
        link: None,
    }
}

/// Syncs `entries` into a new index, then asks a second walk, which sees
/// `entry`, what it need not hash. Returns the blob it knew and how many
/// files it would hash.
fn known_after(entries: &[Entry], entry: &Entry) -> (Option<ObjectId>, usize) {
    let writer = Index::open(None, Path::new(ROOT)).unwrap().write().unwrap();
    let worktree = writer.worktree().unwrap();
    writer.sync_worktree(worktree, entries).unwrap();
    let mut worktree = writer.worktree().unwrap();
    let known = worktree.known(&entry.path, entry.stat.as_ref().unwrap());
    (known, writer.sync_worktree(worktree, entries).unwrap())
}

#[test]
fn a_stored_file_modified_before_the_refresh_is_known_without_hashing() {
    let old = file("a.md", 1, 0);
    assert_eq!(
        known_after(std::slice::from_ref(&old), &old),
        (Some(blob(1)), 0)
    );
}

/// A file at `path` the walk could not read, last modified at `modified_ns`.
fn unreadable(path: &str, modified_ns: i64) -> Entry {
    Entry {
        blob: None,
        status: Status::Degraded(Code::RepoUnreadable),
        size: None,
        ..file(path, 0, modified_ns)
    }
}

/// A cold rebuild would try the file again, so a warm walk does too. It is
/// not counted as hashed, so a walk that still cannot read it writes nothing.
#[test]
fn a_stored_unreadable_file_modified_before_the_refresh_is_read_again_but_not_counted() {
    let old = unreadable("a.md", 0);
    assert_eq!(known_after(std::slice::from_ref(&old), &old), (None, 0));
}

#[test]
fn a_stored_unreadable_file_modified_after_the_refresh_began_is_read_again() {
    let ahead = unreadable("a.md", i64::MAX);
    assert_eq!(known_after(std::slice::from_ref(&ahead), &ahead), (None, 1));
}

#[test]
fn a_stored_file_modified_after_the_refresh_began_is_hashed() {
    let ahead = file("a.md", 1, i64::MAX);
    assert_eq!(known_after(std::slice::from_ref(&ahead), &ahead), (None, 1));
}

#[test]
fn a_file_the_refresh_did_not_store_is_hashed() {
    let old = file("a.md", 1, 0);
    assert_eq!(known_after(&[], &old), (None, 1));
}

/// The refresh time the index holds, as stored.
fn refreshed(connection: &rusqlite::Connection) -> String {
    connection
        .query_row(
            "SELECT value FROM main.state WHERE key = 'refreshed_ns'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

/// What a second sync did to an index.
struct Synced {
    wrote: bool,
    /// The paths and blobs the index then holds.
    rows: Vec<(String, String)>,
    /// The refresh time it then holds; the first sync's is set to "0".
    refreshed: String,
}

/// Syncs `first` into a new index and sets the refresh time it records to
/// "0", then syncs `second` in a new transaction after a walk that hashed
/// `hashed` files.
fn synced_twice(first: &[Entry], second: &[Entry], hashed: usize) -> Synced {
    let cache = tempfile::tempdir().unwrap();
    let open = || Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let writer = open().write().unwrap();
    let worktree = writer.worktree().unwrap();
    writer.sync_worktree(worktree, first).unwrap();
    writer
        .connection()
        .execute(
            "UPDATE main.state SET value = '0' WHERE key = 'refreshed_ns'",
            [],
        )
        .unwrap();
    writer.decide(Vec::new()).unwrap();
    let writer = open().deferred().unwrap();
    let mut worktree = writer.worktree().unwrap();
    for _ in 0..hashed {
        worktree.known("unstored.md", &stat_with(0, 0, 0, 0, 0));
    }
    writer.sync_worktree(worktree, second).unwrap();
    let wrote = writer.wrote().unwrap();
    let view = writer.decide(Vec::new()).unwrap().view().unwrap();
    let rows = view
        .connection()
        .prepare("SELECT path, blob FROM main.worktree ORDER BY path")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    Synced {
        wrote,
        rows,
        refreshed: refreshed(view.connection()),
    }
}

#[test]
fn a_sync_that_hashed_and_changed_nothing_writes_nothing() {
    let entries = [file("a.md", 1, 0)];
    let synced = synced_twice(&entries, &entries, 0);
    assert!(!synced.wrote);
    assert_eq!(synced.rows, [("a.md".to_owned(), blob(1).to_string())]);
    assert_eq!(synced.refreshed, "0");
}

#[test]
fn a_sync_after_a_walk_that_hashed_a_file_records_when_the_refresh_began() {
    let entries = [file("a.md", 1, 0)];
    let synced = synced_twice(&entries, &entries, 1);
    assert!(synced.wrote);
    assert_ne!(synced.refreshed, "0");
}

#[test]
fn a_sync_stores_a_changed_file_and_forgets_a_deleted_one() {
    let first = [file("a.md", 1, 0), file("b.md", 2, 0)];
    let synced = synced_twice(&first, &[file("a.md", 3, 0)], 0);
    assert!(synced.wrote);
    assert_eq!(synced.rows, [("a.md".to_owned(), blob(3).to_string())]);
    assert_ne!(synced.refreshed, "0");
}

#[test]
fn a_sync_that_only_forgets_a_deleted_file_records_when_the_refresh_began() {
    let first = [file("a.md", 1, 0), file("b.md", 2, 0)];
    let synced = synced_twice(&first, &[file("a.md", 1, 0)], 0);
    assert!(synced.wrote);
    assert_eq!(synced.rows, [("a.md".to_owned(), blob(1).to_string())]);
    assert_ne!(synced.refreshed, "0");
}

#[test]
fn a_sync_records_a_refresh_time_no_later_than_the_walk_that_preceded_it() {
    let writer = Index::open(None, Path::new(ROOT)).unwrap().write().unwrap();
    let worktree = writer.worktree().unwrap();
    let walked = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    writer
        .sync_worktree(worktree, &[file("a.md", 1, 0)])
        .unwrap();
    let recorded: u128 = refreshed(writer.connection()).parse().unwrap();
    assert!(recorded > 0 && recorded <= walked);
}
