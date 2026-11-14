//! Syncing a clone that has commits of its own: the merge a sync makes of
//! them with the remote's, the record of each conflict, and how two moved
//! mount pins resolve (docs/architecture/writes.md, Sync).

use std::time::Duration;

use board_model::{Conflict, ConflictKind, ObjectId, Side};

use super::*;
use crate::{Deadline, GIT_DEADLINE, PinBasis, Synced};

/// Commits `files` in `clone` and writes them to its working tree, as a put
/// would have.
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
    id
}

fn sync(clone: &Scenario, push: bool) -> Synced {
    clone
        .open()
        .sync(push, Deadline::after(GIT_DEADLINE), Duration::ZERO)
        .unwrap()
}

/// A remote and two clones of it, all able to commit.
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

fn records(clone: &Scenario) -> Vec<Conflict> {
    clone
        .head()
        .unwrap()
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("conflicts/"))
        .map(|(_, bytes)| Conflict::from_value(&serde_json::from_slice(bytes).unwrap()).unwrap())
        .collect()
}

fn text(clone: &Scenario, path: &str) -> String {
    String::from_utf8(clone.head().unwrap().files[path].clone()).unwrap()
}

#[test]
fn two_clones_that_edit_one_file_merge_to_the_remote_version_and_record_one_conflict() {
    let (_remote, one, two) = pair(&[("a.json", "0")]);
    let first = commit_here(&one, &[("a.json", "1")]);
    let second = commit_here(&two, &[("a.json", "2")]);
    assert!(!sync(&one, true).merged);
    let synced = sync(&two, true);
    assert!(synced.merged);
    assert_eq!(synced.conflicts.len(), 1);
    assert_eq!(synced.conflicts[0].path, "a.json");
    assert_eq!(synced.conflicts[0].kind, ConflictKind::BothChanged);
    assert!(synced.pushed);
    let head = two.head().unwrap();
    assert_eq!(head.parents, [second, first]);
    assert_eq!(text(&two, "a.json"), "1");
    assert_eq!(
        std::fs::read_to_string(two.workdir().join("a.json")).unwrap(),
        "1"
    );
    let history: Vec<ObjectId> = two.history().into_iter().map(|(id, _)| id).collect();
    assert!(history.contains(&first) && history.contains(&second));
    let found = records(&two);
    assert_eq!(found.len(), 1);
    let record = &found[0];
    assert_eq!(record.path, "a.json");
    assert_eq!(record.kind, ConflictKind::BothChanged);
    assert_eq!(record.kept, Side::Remote);
    assert_eq!(record.base, Some(two.blob("0")));
    assert_eq!(record.local.blob, Some(two.blob("2")));
    assert_eq!(record.local.commit, second);
    assert_eq!(record.remote.blob, Some(two.blob("1")));
    assert_eq!(record.remote.commit, first);
    assert_eq!(record.remote.author, "scenario <scenario@example.com>");
    assert_eq!(
        synced.conflicts[0].record,
        format!("conflicts/{}", record_name(&two))
    );
    // The first clone fast-forwards to the merge, which holds the same record.
    let again = sync(&one, false);
    assert!(!again.merged);
    assert_eq!(one.head().unwrap().id, head.id);
    assert_eq!(records(&one).len(), 1);
}

fn record_name(clone: &Scenario) -> String {
    clone
        .head()
        .unwrap()
        .files
        .keys()
        .find_map(|path| path.strip_prefix("conflicts/"))
        .unwrap()
        .to_owned()
}

#[test]
fn a_merge_without_conflicts_keeps_the_local_commit_ids_and_writes_no_record() {
    let (_remote, one, two) = pair(&[("a.json", "0")]);
    let theirs = commit_here(&one, &[("a.json", "0"), ("b.json", "1")]);
    sync(&one, true);
    let ours = commit_here(&two, &[("a.json", "5")]);
    let synced = sync(&two, false);
    assert!(synced.merged);
    assert_eq!(synced.conflicts, Vec::new());
    let head = two.head().unwrap();
    assert_eq!(head.parents, [ours, theirs]);
    assert_eq!(head.files["a.json"], b"5");
    assert_eq!(head.files["b.json"], b"1");
    assert_eq!(records(&two).len(), 0);
    assert_eq!(
        std::fs::read_to_string(two.workdir().join("b.json")).unwrap(),
        "1"
    );
    assert_eq!(two.index().len(), 2);
}

#[test]
fn a_clone_with_nothing_new_fast_forwards_to_the_remote() {
    let (_remote, one, two) = pair(&[("a.json", "0")]);
    let tip = commit_here(&one, &[("a.json", "1")]);
    sync(&one, true);
    let synced = sync(&two, false);
    assert!(!synced.merged);
    assert_eq!(synced.new, tip);
    assert_eq!(two.head().unwrap().id, tip);
    assert_eq!(text(&two, "a.json"), "1");
}

#[test]
fn a_remote_with_nothing_new_leaves_the_head_alone() {
    let (_remote, _one, two) = pair(&[("a.json", "0")]);
    let ours = commit_here(&two, &[("a.json", "5")]);
    let synced = sync(&two, true);
    assert!(!synced.merged);
    assert_eq!(synced.new, ours);
    assert!(synced.pushed);
    assert_eq!(two.head().unwrap().id, ours);
}

#[test]
fn a_file_one_side_edited_and_the_other_deleted_keeps_the_edit_and_records_it() {
    let (_remote, one, two) = pair(&[("a.json", "0"), ("b.json", "0")]);
    commit_here(&one, &[("b.json", "0")]);
    sync(&one, true);
    commit_here(&two, &[("a.json", "9"), ("b.json", "0")]);
    let synced = sync(&two, false);
    assert_eq!(synced.conflicts.len(), 1);
    assert_eq!(synced.conflicts[0].kind, ConflictKind::EditDelete);
    assert_eq!(text(&two, "a.json"), "9");
    let found = records(&two);
    assert_eq!(found[0].kept, Side::Local);
    assert_eq!(found[0].remote.blob, None);
}

/// A mounted repository with three commits in a line, and a pointer file for
/// each as a mount pin of it.
struct Mounted {
    repo: Scenario,
    commits: [ObjectId; 3],
}

impl Mounted {
    fn new() -> Self {
        let repo = Scenario::new();
        let commits = [
            repo.commit(&[("f", "0")]),
            repo.commit(&[("f", "1")]),
            repo.commit(&[("f", "2")]),
        ];
        Self { repo, commits }
    }

    fn pin(&self, commit: ObjectId) -> String {
        board_scenario::seed::mount_of(&self.repo.path(), commit)
    }
}

/// Syncs a clone whose mount pin moved to `local` against a remote whose
/// moved to `remote`, from `base`, and gives the pin the clone ends with.
fn pins(base: &str, local: &str, remote: &str) -> (Synced, String) {
    let (_remote, one, two) = pair(&[("mount.json", base)]);
    commit_here(&one, &[("mount.json", remote)]);
    sync(&one, true);
    commit_here(&two, &[("mount.json", local)]);
    let synced = sync(&two, false);
    let pin = text(&two, "mount.json");
    (synced, pin)
}

#[test]
fn two_moved_pins_take_the_one_that_descends_from_the_other_without_a_record() {
    let mounted = Mounted::new();
    let [old, middle, new] = mounted.commits;
    let (synced, pin) = pins(&mounted.pin(old), &mounted.pin(middle), &mounted.pin(new));
    assert_eq!(pin, mounted.pin(new));
    assert_eq!(synced.conflicts, Vec::new());
    assert_eq!(synced.pins.len(), 1);
    assert_eq!(synced.pins[0].basis, PinBasis::Descendant);
    let (synced, pin) = pins(&mounted.pin(old), &mounted.pin(new), &mounted.pin(middle));
    assert_eq!(pin, mounted.pin(new));
    assert_eq!(synced.conflicts, Vec::new());
}

#[test]
fn two_moved_pins_where_a_commit_is_missing_keep_the_local_pin() {
    let mounted = Mounted::new();
    let [old, middle, _] = mounted.commits;
    let missing = ObjectId::from_hex(&"ab".repeat(20)).unwrap();
    let (synced, pin) = pins(
        &mounted.pin(old),
        &mounted.pin(middle),
        &mounted.pin(missing),
    );
    assert_eq!(pin, mounted.pin(middle));
    assert_eq!(synced.pins[0].basis, PinBasis::Unknown);
    assert_eq!(synced.conflicts, Vec::new());
}

#[test]
fn local_records_that_fail_a_schema_the_remote_changed_are_reported() {
    let schema = |max: u32| {
        format!(r#"{{"type":"object","properties":{{"n":{{"type":"integer","maximum":{max}}}}}}}"#)
    };
    let record = |n: u32| format!(r#"{{"$type":"t/x/v1","n":{n}}}"#);
    let (_remote, one, two) = pair(&[
        (".board/types/t/x/v1.schema.json", &schema(100)),
        ("r.json", &record(1)),
    ]);
    commit_here(
        &one,
        &[
            (".board/types/t/x/v1.schema.json", &schema(5)),
            ("r.json", &record(1)),
        ],
    );
    sync(&one, true);
    commit_here(
        &two,
        &[
            (".board/types/t/x/v1.schema.json", &schema(200)),
            ("r.json", &record(50)),
        ],
    );
    let synced = sync(&two, false);
    assert_eq!(synced.failing, ["r.json"]);
}

#[test]
fn a_text_file_both_sides_changed_in_different_places_keeps_both_changes_without_a_record() {
    let (_remote, one, two) = pair(&[("daily.md", "a\nb\nc\nd\ne\nf\ng\n")]);
    commit_here(&one, &[("daily.md", "a\nb\nc\nd\ne\nf\ng\nphone\n")]);
    sync(&one, true);
    commit_here(&two, &[("daily.md", "agent\na\nb\nc\nd\ne\nf\ng\n")]);
    let synced = sync(&two, false);
    assert!(synced.merged);
    assert_eq!(synced.conflicts, Vec::new());
    assert_eq!(records(&two).len(), 0);
    let merged = "agent\na\nb\nc\nd\ne\nf\ng\nphone\n";
    assert_eq!(text(&two, "daily.md"), merged);
    assert_eq!(
        std::fs::read_to_string(two.workdir().join("daily.md")).unwrap(),
        merged
    );
}

#[test]
fn a_text_file_both_sides_changed_on_one_line_takes_the_remote_and_records_it() {
    let (_remote, one, two) = pair(&[("daily.md", "a\nb\nc\n")]);
    commit_here(&one, &[("daily.md", "a\nphone\nc\n")]);
    sync(&one, true);
    commit_here(&two, &[("daily.md", "a\nagent\nc\n")]);
    let synced = sync(&two, false);
    assert_eq!(synced.conflicts.len(), 1);
    assert_eq!(text(&two, "daily.md"), "a\nphone\nc\n");
    assert_eq!(records(&two).len(), 1);
}

fn record_files(clone: &Scenario) -> std::collections::BTreeMap<String, String> {
    (clone.head().unwrap().files.into_iter())
        .filter(|(path, _)| path.starts_with("conflicts/"))
        .map(|(path, bytes)| (path, String::from_utf8(bytes).unwrap()))
        .collect()
}

#[test]
fn a_later_conflict_at_an_already_conflicted_path_keeps_the_earlier_record() {
    let (_remote, one, two) = pair(&[("a.json", "0")]);
    commit_here(&one, &[("a.json", "1")]);
    sync(&one, true);
    commit_here(&two, &[("a.json", "2")]);
    sync(&two, true);
    let earlier = record_files(&two);
    assert_eq!(earlier.len(), 1);
    sync(&one, false);
    let (name, record) = earlier.iter().next().unwrap();
    commit_here(&one, &[("a.json", "3"), (name, record)]);
    sync(&one, true);
    commit_here(&two, &[("a.json", "4"), (name, record)]);
    let synced = sync(&two, false);
    assert_eq!(synced.conflicts.len(), 1);
    let found = record_files(&two);
    assert_eq!(found.len(), 2, "{:?}", found.keys());
    for (path, bytes) in &earlier {
        assert_eq!(&found[path], bytes);
    }
    assert!(found.contains_key(synced.conflicts[0].record.as_str()));
}

#[test]
fn two_clones_that_make_the_same_conflict_record_never_conflict_about_it() {
    let remote = Scenario::new();
    remote.identified().accepting_pushes();
    remote.commit(&[("a.json", "0")]);
    let [one, two, three] = [(); 3].map(|()| Scenario::clone_of(&remote));
    for clone in [&one, &two, &three] {
        clone.identified();
    }
    commit_here(&one, &[("a.json", "1")]);
    sync(&one, true);
    commit_here(&two, &[("a.json", "2")]);
    // Another parent gives `three`'s commit another id, with the same file.
    commit_here(&three, &[("a.json", "0"), ("b.json", "1")]);
    commit_here(&three, &[("a.json", "2"), ("b.json", "1")]);
    sync(&two, true);
    let held = record_files(&two);
    assert_eq!(held.len(), 1);
    let synced = sync(&three, true);
    // The clone makes the record the other already pushed: that one stands.
    assert_eq!(record_files(&three), held);
    assert_eq!(synced.conflicts.len(), 1);
    assert!(held.contains_key(synced.conflicts[0].record.as_str()));
    // A fourth merge of the two clones' records finds no conflict in them.
    let again = sync(&two, true);
    assert_eq!(again.conflicts, Vec::new());
    assert_eq!(record_files(&two), held);
}

#[test]
fn merging_the_same_commits_twice_gives_the_same_conflict_record_bytes_and_name() {
    let remote = Scenario::new();
    remote.identified().accepting_pushes();
    remote.commit(&[("a.json", "0")]);
    let [one, two, three] = [(); 3].map(|()| Scenario::clone_of(&remote));
    for clone in [&one, &two, &three] {
        clone.identified();
    }
    commit_here(&one, &[("a.json", "1")]);
    sync(&one, true);
    // `two` and `three` commit the same tree on the same parent, so the
    // commit has one id.
    let ours = commit_here(&two, &[("a.json", "2")]);
    assert_eq!(commit_here(&three, &[("a.json", "2")]), ours);
    sync(&two, false);
    sync(&three, false);
    assert_eq!(record_files(&two), record_files(&three));
    assert_eq!(record_files(&two).len(), 1);
}

#[test]
fn two_appends_at_the_end_of_a_text_file_both_stay_remote_first_and_record_nothing() {
    let (_remote, one, two) = pair(&[("daily.md", "a\nb\n")]);
    commit_here(&one, &[("daily.md", "a\nb\nphone\n")]);
    sync(&one, true);
    commit_here(&two, &[("daily.md", "a\nb\nagent\n")]);
    let synced = sync(&two, true);
    assert!(synced.merged && synced.pushed);
    assert_eq!(synced.conflicts, Vec::new());
    assert_eq!(records(&two).len(), 0);
    assert_eq!(text(&two, "daily.md"), "a\nb\nphone\nagent\n");
    assert_eq!(
        std::fs::read_to_string(two.workdir().join("daily.md")).unwrap(),
        "a\nb\nphone\nagent\n"
    );
}

#[test]
fn the_same_conflict_with_the_sides_swapped_has_one_record_name() {
    // `second` is the clone that merges: its commit is the local side.
    let merged_with = |local: &str, remote: &str| {
        let (_remote, first, second) = pair(&[("a.json", "0")]);
        commit_here(&first, &[("a.json", remote)]);
        sync(&first, true);
        commit_here(&second, &[("a.json", local)]);
        sync(&second, false);
        record_files(&second).into_keys().collect::<Vec<_>>()
    };
    let names = merged_with("1", "2");
    assert_eq!(names.len(), 1);
    assert_eq!(names, merged_with("2", "1"));
}
