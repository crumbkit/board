//! Hashing, reading and listing names in the working tree.

use super::*;
use crate::git::model_id;

#[test]
fn a_stream_longer_or_shorter_than_its_length_has_no_id() {
    let directory = tempfile::tempdir().unwrap();
    gix::init(directory.path()).unwrap();
    let repository = Repository::open(directory.path()).unwrap();
    let expected =
        gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, b"abc").unwrap();
    assert_eq!(
        repository.hash(&mut &b"abc"[..], 3),
        Some(model_id(&expected))
    );
    assert_eq!(repository.hash(&mut &b"abcd"[..], 3), None);
    assert_eq!(repository.hash(&mut &b"ab"[..], 3), None);
}

#[test]
fn a_stream_longer_or_shorter_than_its_length_is_not_read() {
    assert_eq!(read_exactly(&b"abc"[..], 3), Some(b"abc".to_vec()));
    assert_eq!(read_exactly(&b"abcd"[..], 3), None);
    assert_eq!(read_exactly(&b"ab"[..], 3), None);
}

#[test]
fn a_name_is_listed_only_in_its_own_spelling() {
    for (written, listed) in [(".board", true), (".Board", false)] {
        let scenario = board_scenario::Scenario::new();
        scenario.write(&format!("{written}/x.md"), board_scenario::Item::File(""));
        assert_eq!(lists(scenario.workdir(), ".board"), listed, "{written}");
    }
}

#[test]
fn a_name_is_found_in_the_entries_given_even_when_they_are_not_the_listing() {
    let scenario = board_scenario::Scenario::new();
    let empty = board_scenario::Item::File("{}");
    scenario.write("a.json", empty).write("b.json", empty);
    let repository = Repository::open(scenario.workdir()).unwrap();
    let mut tree = repository.working_tree(None).unwrap();
    let root = PathBuf::new();
    let listed = tree.list(&root).unwrap();
    let reversed: Vec<RawEntry<PathBuf>> = listed.iter().rev().cloned().collect();
    for entries in [&listed[..], &reversed[..]] {
        let Named::One(found) = tree
            .named(&root, entries, "a", &mut Pointers::default())
            .unwrap()
        else {
            panic!("a.json is the one entry named a");
        };
        assert_eq!(found.name, b"a.json");
    }
}
