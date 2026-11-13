use board_model::{ObjectId, TYPES_DIRECTORY};
use gix::object::tree::EntryKind;

use super::*;
use crate::git::Repository;

struct Fixture {
    _directory: tempfile::TempDir,
    repository: Repository,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        gix::init(directory.path()).unwrap();
        let repository = Repository::open(directory.path()).unwrap();
        Self {
            _directory: directory,
            repository,
        }
    }

    /// A tree of the given files, each a plain blob.
    fn tree(&self, files: &[(&str, &str)]) -> gix::ObjectId {
        let git = self.repository.git();
        let mut editor = git.edit_tree(git.empty_tree().id).unwrap();
        for (path, content) in files {
            let blob = git.write_blob(content.as_bytes()).unwrap().detach();
            editor.upsert(*path, EntryKind::Blob, blob).unwrap();
        }
        editor.write().unwrap().detach()
    }

    fn blob(&self, content: &str) -> ObjectId {
        let id = self
            .repository
            .git()
            .write_blob(content.as_bytes())
            .unwrap()
            .detach();
        model_id(&id)
    }

    fn merge(
        &self,
        base: &[(&str, &str)],
        local: &[(&str, &str)],
        remote: &[(&str, &str)],
    ) -> (Merged, Vec<(String, String)>) {
        self.merge_with(base, local, remote, &|_, _| false)
    }

    fn merge_with(
        &self,
        base: &[(&str, &str)],
        local: &[(&str, &str)],
        remote: &[(&str, &str)],
        descends: &dyn Fn(ObjectId, ObjectId) -> bool,
    ) -> (Merged, Vec<(String, String)>) {
        let merged = merge_trees(
            &self.repository,
            Some(self.tree(base)),
            self.tree(local),
            self.tree(remote),
            descends,
        )
        .unwrap();
        let files = self.files(merged.tree);
        (merged, files)
    }

    /// Every file beneath `tree` with its content, in path order.
    fn files(&self, tree: gix::ObjectId) -> Vec<(String, String)> {
        let git = self.repository.git();
        let mut found = Vec::new();
        let mut pending = vec![(String::new(), tree)];
        while let Some((prefix, id)) = pending.pop() {
            let tree = git.find_tree(id).unwrap();
            for entry in tree.decode().unwrap().entries {
                let path = format!("{prefix}{}", entry.filename);
                if entry.mode.is_tree() {
                    pending.push((format!("{path}/"), entry.oid.to_owned()));
                } else {
                    let blob = git.find_blob(entry.oid).unwrap();
                    found.push((path, String::from_utf8(blob.data.clone()).unwrap()));
                }
            }
        }
        found.sort();
        found
    }
}

fn file(path: &str, content: &str) -> (String, String) {
    (path.to_owned(), content.to_owned())
}

const OLD: &str = "1111111111111111111111111111111111111111";
const LOCAL: &str = "2222222222222222222222222222222222222222";
const REMOTE: &str = "3333333333333333333333333333333333333333";

fn pin(commit: &str) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "../other", "follow": "main", "commit": "{commit}"}}"#
    )
}

fn id(hex: &str) -> ObjectId {
    ObjectId::from_hex(hex).unwrap()
}

#[test]
fn a_change_on_one_side_is_taken_without_a_conflict() {
    let fixture = Fixture::new();
    let base = [("a.json", "1"), ("b.json", "1")];
    let local = [("a.json", "2"), ("b.json", "1")];
    let remote = [("a.json", "1"), ("b.json", "3"), ("c.json", "new")];
    let (merged, files) = fixture.merge(&base, &local, &remote);
    assert_eq!(
        files,
        [
            file("a.json", "2"),
            file("b.json", "3"),
            file("c.json", "new")
        ]
    );
    assert_eq!(merged.conflicts, Vec::new());
    assert_eq!(merged.pins, Vec::new());
}

#[test]
fn a_deletion_on_one_side_is_taken_when_the_other_left_the_file_alone() {
    let fixture = Fixture::new();
    let base = [("a.json", "1"), ("b.json", "1")];
    let (merged, files) = fixture.merge(&base, &[("b.json", "1")], &base);
    assert_eq!(files, [file("b.json", "1")]);
    assert_eq!(merged.conflicts, Vec::new());
}

#[test]
fn the_same_change_on_both_sides_is_taken_without_a_conflict() {
    let fixture = Fixture::new();
    let base = [("a.json", "1"), ("gone.json", "1")];
    let both = [("a.json", "2"), ("added.json", "x")];
    let (merged, files) = fixture.merge(&base, &both, &both);
    assert_eq!(files, [file("a.json", "2"), file("added.json", "x")]);
    assert_eq!(merged.conflicts, Vec::new());
}

#[test]
fn a_file_both_sides_edited_differently_takes_the_remote_and_records_a_conflict() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge(
        &[("a.json", "1")],
        &[("a.json", "local")],
        &[("a.json", "remote")],
    );
    assert_eq!(files, [file("a.json", "remote")]);
    assert_eq!(
        merged.conflicts,
        [Conflict {
            path: "a.json".to_owned(),
            kind: ConflictKind::BothChanged,
            base: Some(fixture.blob("1")),
            local: Some(fixture.blob("local")),
            remote: Some(fixture.blob("remote")),
        }]
    );
}

#[test]
fn a_file_both_sides_added_differently_takes_the_remote_and_records_a_conflict() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge(&[], &[("a.json", "local")], &[("a.json", "remote")]);
    assert_eq!(files, [file("a.json", "remote")]);
    assert_eq!(merged.conflicts.len(), 1);
    assert_eq!(merged.conflicts[0].kind, ConflictKind::BothChanged);
    assert_eq!(merged.conflicts[0].base, None);
}

#[test]
fn a_conflict_deep_in_a_directory_names_the_whole_path_and_siblings_still_merge() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge(
        &[("d/e/a.json", "1"), ("d/b.json", "1")],
        &[("d/e/a.json", "local"), ("d/b.json", "1")],
        &[("d/e/a.json", "remote"), ("d/b.json", "2")],
    );
    assert_eq!(files, [file("d/b.json", "2"), file("d/e/a.json", "remote")]);
    assert_eq!(merged.conflicts.len(), 1);
    assert_eq!(merged.conflicts[0].path, "d/e/a.json");
}

#[test]
fn an_edit_wins_over_a_delete_from_the_local_side_and_records_a_conflict() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge(&[("a.json", "1")], &[("a.json", "edited")], &[]);
    assert_eq!(files, [file("a.json", "edited")]);
    assert_eq!(
        merged.conflicts,
        [Conflict {
            path: "a.json".to_owned(),
            kind: ConflictKind::EditDelete,
            base: Some(fixture.blob("1")),
            local: Some(fixture.blob("edited")),
            remote: None,
        }]
    );
}

#[test]
fn an_edit_wins_over_a_delete_from_the_remote_side_and_records_a_conflict() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge(&[("a.json", "1")], &[], &[("a.json", "edited")]);
    assert_eq!(files, [file("a.json", "edited")]);
    assert_eq!(merged.conflicts.len(), 1);
    assert_eq!(merged.conflicts[0].kind, ConflictKind::EditDelete);
    assert_eq!(merged.conflicts[0].local, None);
    assert_eq!(merged.conflicts[0].remote, Some(fixture.blob("edited")));
}

#[test]
fn a_file_against_a_directory_takes_the_remote_and_records_a_conflict() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge(&[], &[("a", "file")], &[("a/b.json", "inside")]);
    assert_eq!(files, [file("a/b.json", "inside")]);
    assert_eq!(
        merged.conflicts,
        [Conflict {
            path: "a".to_owned(),
            kind: ConflictKind::FileDirectory,
            base: None,
            local: Some(fixture.blob("file")),
            remote: None,
        }]
    );
}

#[test]
fn a_directory_against_a_file_takes_the_remote_file() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge(&[], &[("a/b.json", "inside")], &[("a", "file")]);
    assert_eq!(files, [file("a", "file")]);
    assert_eq!(merged.conflicts.len(), 1);
    assert_eq!(merged.conflicts[0].kind, ConflictKind::FileDirectory);
    assert_eq!(merged.conflicts[0].remote, Some(fixture.blob("file")));
}

#[test]
fn two_sides_adding_different_files_to_one_directory_keep_both_without_a_conflict() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge(&[], &[("d/a.json", "1")], &[("d/b.json", "2")]);
    assert_eq!(files, [file("d/a.json", "1"), file("d/b.json", "2")]);
    assert_eq!(merged.conflicts, Vec::new());
}

#[test]
fn a_pin_the_remote_moved_forward_of_the_local_one_takes_the_remote_pin() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge_with(
        &[("mount.json", &pin(OLD))],
        &[("mount.json", &pin(LOCAL))],
        &[("mount.json", &pin(REMOTE))],
        &|candidate, of| candidate == id(REMOTE) && of == id(LOCAL),
    );
    assert_eq!(files, [file("mount.json", &pin(REMOTE))]);
    assert_eq!(merged.conflicts, Vec::new());
    assert_eq!(
        merged.pins,
        [PinResolution {
            path: "mount.json".to_owned(),
            local: id(LOCAL),
            remote: id(REMOTE),
            taken: PinSide::Remote,
        }]
    );
}

#[test]
fn a_pin_the_local_side_moved_forward_of_the_remote_one_keeps_the_local_pin() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge_with(
        &[("mount.json", &pin(OLD))],
        &[("mount.json", &pin(LOCAL))],
        &[("mount.json", &pin(REMOTE))],
        &|candidate, of| candidate == id(LOCAL) && of == id(REMOTE),
    );
    assert_eq!(files, [file("mount.json", &pin(LOCAL))]);
    assert_eq!(merged.conflicts, Vec::new());
    assert_eq!(merged.pins[0].taken, PinSide::Local);
}

#[test]
fn two_pins_with_neither_descending_from_the_other_take_the_remote_pin_without_a_conflict() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge(
        &[("mount.json", &pin(OLD))],
        &[("mount.json", &pin(LOCAL))],
        &[("mount.json", &pin(REMOTE))],
    );
    assert_eq!(files, [file("mount.json", &pin(REMOTE))]);
    assert_eq!(merged.conflicts, Vec::new());
    assert_eq!(merged.pins[0].taken, PinSide::Remote);
}

#[test]
fn a_mount_pointer_that_changed_more_than_its_pin_is_an_ordinary_conflict() {
    let fixture = Fixture::new();
    let other = pin(LOCAL).replace("../other", "../elsewhere");
    let (merged, files) = fixture.merge_with(
        &[("mount.json", &pin(OLD))],
        &[("mount.json", &other)],
        &[("mount.json", &pin(REMOTE))],
        &|_, _| true,
    );
    assert_eq!(files, [file("mount.json", &pin(REMOTE))]);
    assert_eq!(merged.pins, Vec::new());
    assert_eq!(merged.conflicts.len(), 1);
    assert_eq!(merged.conflicts[0].kind, ConflictKind::BothChanged);
}

#[test]
fn a_pin_moved_on_one_side_only_is_taken_and_never_asks_about_ancestry() {
    let fixture = Fixture::new();
    let (merged, files) = fixture.merge_with(
        &[("mount.json", &pin(OLD))],
        &[("mount.json", &pin(OLD))],
        &[("mount.json", &pin(REMOTE))],
        &|_, _| panic!("one side moved, so nothing to compare"),
    );
    assert_eq!(files, [file("mount.json", &pin(REMOTE))]);
    assert_eq!(merged.pins, Vec::new());
    assert_eq!(merged.conflicts, Vec::new());
}

#[test]
fn two_schemas_for_one_type_take_the_remote_and_are_flagged() {
    let fixture = Fixture::new();
    let path = format!("{TYPES_DIRECTORY}acme/thing/v1.schema.json");
    let (merged, files) = fixture.merge(
        &[],
        &[(&path, "{\"local\":1}")],
        &[(&path, "{\"remote\":1}")],
    );
    assert_eq!(files, [file(&path, "{\"remote\":1}")]);
    assert_eq!(
        merged.schemas,
        [SchemaClash {
            path: path.clone(),
            local: fixture.blob("{\"local\":1}"),
            remote: fixture.blob("{\"remote\":1}"),
        }]
    );
    assert_eq!(merged.conflicts.len(), 1);
    assert_eq!(merged.conflicts[0].path, path);
}

#[test]
fn two_ordinary_files_differing_are_never_flagged_as_schemas() {
    let fixture = Fixture::new();
    let (merged, _) = fixture.merge(&[], &[("a.json", "1")], &[("a.json", "2")]);
    assert_eq!(merged.schemas, Vec::new());
}

#[test]
fn a_merge_writes_no_ref_and_leaves_both_inputs_unchanged() {
    let fixture = Fixture::new();
    let (local, remote) = (
        fixture.tree(&[("a.json", "1")]),
        fixture.tree(&[("a.json", "2")]),
    );
    let merged = merge_trees(&fixture.repository, None, local, remote, &|_, _| false).unwrap();
    assert_ne!(merged.tree, local);
    assert_eq!(fixture.files(local), [file("a.json", "1")]);
    assert!(fixture.repository.head().unwrap().is_none());
}

#[test]
fn a_merge_that_leaves_nothing_is_the_empty_tree() {
    let fixture = Fixture::new();
    let base = [("a.json", "1")];
    let (merged, files) = fixture.merge(&base, &[], &[]);
    assert_eq!(files, Vec::new());
    assert_eq!(merged.tree, fixture.repository.git().empty_tree().id);
}
