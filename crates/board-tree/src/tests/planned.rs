//! The write check: the code the walk would give a file once it is written
//! (docs/architecture/paths/writes.md). Commits can hold names that differ
//! only in case on any file system, so these run over commits.

use std::collections::BTreeMap;

use super::*;
use crate::walk::{Beneath, Planned, planned_code};

const LINK: &str = r#"{"$type": "board/mount/v1", "path": "/a"}"#;
const INVALID: &str = r#"{"$type": "board/mount/v1"}"#;

/// What the write check says of writing `file` into `commit`.
fn planned(scenario: &Scenario, commit: ObjectId, file: &str, is_pointer: bool) -> Option<Planned> {
    let repository = scenario.open();
    let mut tree = repository.commit_tree(commit).unwrap();
    planned_code(&mut tree, file, is_pointer).unwrap()
}

/// The code of an existing `directory`, or of one the write creates when
/// `created`, beside `beside`.
fn directory(code: Code, directory: &str, created: bool, beside: Option<&str>) -> Planned {
    Planned::Directory(Beneath {
        code,
        directory: directory.to_owned(),
        created,
        beside: beside.map(str::to_owned),
    })
}

#[test]
fn a_write_beneath_a_directory_that_collides_in_case_is_refused_naming_both_and_its_twin_is_admitted()
 {
    let scenario = Scenario::new();
    let alone = scenario.commit(&[("Work/sub/a.json", "{}")]);
    assert_eq!(planned(&scenario, alone, "Work/sub/new.json", false), None);
    let both = scenario.commit(&[("Work/sub/a.json", "{}"), ("work/b.json", "{}")]);
    assert_eq!(
        planned(&scenario, both, "Work/sub/new.json", false),
        Some(directory(
            Code::PathCaseCollision,
            "Work",
            false,
            Some("work")
        )),
        "the directory that collides, not the one beneath it"
    );
    assert_eq!(
        planned(&scenario, alone, "work/new.json", false),
        Some(directory(
            Code::PathCaseCollision,
            "work",
            true,
            Some("Work")
        )),
        "a directory the write creates"
    );
    assert_eq!(
        planned(&scenario, alone, "Work/sub/a.json", true),
        None,
        "a pointer with nothing beside it"
    );
}

#[test]
fn a_write_names_the_highest_directory_whose_code_it_inherits_and_its_twin_is_admitted() {
    let scenario = Scenario::new();
    let mut files = vec![("Work/A/x.json", "{}"), ("Work/a/y.json", "{}")];
    let inner = scenario.commit(&files);
    assert_eq!(
        planned(&scenario, inner, "Work/A/new.json", false),
        Some(directory(
            Code::PathCaseCollision,
            "Work/A",
            false,
            Some("Work/a")
        ))
    );
    files.push(("work/z.json", "{}"));
    let outer = scenario.commit(&files);
    assert_eq!(
        planned(&scenario, outer, "Work/A/new.json", false),
        Some(directory(
            Code::PathCaseCollision,
            "Work",
            false,
            Some("work")
        )),
        "Work/A collides too, beneath Work"
    );
    let apart = scenario.commit(&[
        ("Work/A/x.json", "{}"),
        ("Work/b/y.json", "{}"),
        ("other/z.json", "{}"),
    ]);
    assert_eq!(planned(&scenario, apart, "Work/A/new.json", false), None);
}

#[test]
fn a_write_beside_or_beneath_a_pointer_and_its_directory_is_shadowed_and_its_twin_is_admitted() {
    let scenario = Scenario::new();
    let record = scenario.commit(&[("x.json", "{}"), ("x/c.json", "{}")]);
    assert_eq!(planned(&scenario, record, "x/new.json", false), None);
    assert_eq!(planned(&scenario, record, "x.json", false), None);
    assert_eq!(
        planned(&scenario, record, "x.json", true),
        Some(Planned::File(Code::PathShadowed)),
        "a pointer written beside the directory"
    );
    let pointer = scenario.commit(&[("x.json", INVALID), ("x/c.json", "{}")]);
    assert_eq!(
        planned(&scenario, pointer, "x/new.json", false),
        Some(directory(Code::PathShadowed, "x", false, None)),
        "beneath a directory an invalid pointer shadows"
    );
    let alone = scenario.commit(&[("x.json", INVALID)]);
    assert_eq!(
        planned(&scenario, alone, "x/new.json", false),
        Some(directory(Code::PathShadowed, "x", true, None)),
        "into a directory the write creates beside the pointer"
    );
    let plain = scenario.commit(&[("x.json", "{}")]);
    assert_eq!(planned(&scenario, plain, "x/new.json", false), None);
}

#[test]
fn a_write_into_a_directory_with_a_malformed_name_is_refused_naming_it() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("a\tb/x.json", "{}"), ("ab/x.json", "{}")]);
    assert_eq!(
        planned(&scenario, commit, "a\tb/y.json", false),
        Some(directory(Code::PathMalformed, "a\tb", false, None))
    );
    assert_eq!(planned(&scenario, commit, "ab/y.json", false), None);
}

/// A directory whose logical path is `bytes` long: fifteen segments of the
/// longest a segment may be, then one that makes up the rest.
fn deep(bytes: usize) -> String {
    let long = "d".repeat(board_model::MAX_SEGMENT_BYTES);
    let mut segments = vec![long; 15];
    let used: usize = segments.iter().map(|segment| segment.len() + 1).sum();
    segments.push("e".repeat(bytes - used - 1));
    segments.join("/")
}

#[test]
fn a_write_below_the_root_whose_logical_path_is_too_long_is_malformed_and_its_twin_is_admitted() {
    let max = board_model::MAX_PATH_BYTES;
    let scenario = Scenario::new();
    // `/x` and `/n` take two bytes beneath the directory, `/n/x` four.
    for (base, file, refused) in [
        (deep(max - 2), "x.json", None),
        (deep(max - 1), "x.json", Some("file")),
        (deep(max - 4), "n/x.json", None),
        (deep(max - 3), "n/x.json", Some("file")),
        (deep(max - 1), "n/x.json", Some("n")),
    ] {
        let commit = scenario.commit(&[(&format!("{base}/a.json"), "{}")]);
        let path = format!("{base}/{file}");
        let expected = refused.map(|at| {
            if at == "file" {
                Planned::File(Code::PathMalformed)
            } else {
                directory(Code::PathMalformed, &format!("{base}/n"), true, None)
            }
        });
        assert_eq!(
            planned(&scenario, commit, &path, false),
            expected,
            "{}",
            path.len()
        );
    }
}

/// Up to eight files, each one to three segments drawn from a few names, so
/// that records, directories, names differing in case and malformed names
/// often meet at one path; each file a record, a link, or an invalid
/// pointer.
fn files() -> impl Strategy<Value = BTreeMap<String, &'static str>> {
    let content = prop::sample::select(vec!["{}", LINK, INVALID]);
    prop::collection::btree_map(paths(), content, 0..8)
}

fn paths() -> impl Strategy<Value = String> {
    let segment = prop::sample::select(vec!["a", "A", "b", "a.json", "A.json", "a\tb"]);
    prop::collection::vec(segment, 1..4).prop_map(|segments| segments.join("/"))
}

/// Whether one of `files` is a directory on the way to another, which no
/// tree can hold.
fn clash(files: &BTreeMap<String, &str>) -> bool {
    files.keys().any(|path| {
        files
            .keys()
            .any(|other| other.starts_with(&format!("{path}/")))
    })
}

proptest! {
    #[test]
    fn the_write_check_predicts_the_code_the_walk_gives_the_written_file(
        files in files(),
        target in paths(),
        is_pointer in any::<bool>(),
    ) {
        let mut after = files.clone();
        after.insert(target.clone(), if is_pointer { LINK } else { "{}" });
        prop_assume!(!clash(&after));
        let scenario = Scenario::new();
        let commit = |files: &BTreeMap<String, &str>| {
            let pairs: Vec<(&str, &str)> =
                files.iter().map(|(path, content)| (path.as_str(), *content)).collect();
            scenario.commit(&pairs)
        };
        let planned = planned(&scenario, commit(&files), &target, is_pointer);
        let written = scenario
            .open()
            .walk(&commit(&after).source())
            .unwrap()
            .into_iter()
            .find(|entry| entry.path == target)
            .unwrap();
        let code = match &planned {
            None => None,
            Some(Planned::File(code)) => Some(*code),
            Some(Planned::Directory(Beneath { code, directory, created, .. })) => {
                let prefix = format!("{directory}/");
                prop_assert!(target.starts_with(&prefix), "{}", directory);
                let exists = files.keys().any(|path| path.starts_with(&prefix));
                prop_assert_eq!(*created, !exists, "{}", directory);
                Some(*code)
            }
        };
        prop_assert_eq!(written.status, code.map_or(Status::Ok, Status::Degraded));
    }
}

#[test]
fn a_directory_with_its_own_code_says_what_is_wrong_with_it_instead_of_the_code() {
    let beneath = |code, created, beside: Option<&str>| {
        Beneath {
            code,
            directory: "d".to_owned(),
            created,
            beside: beside.map(str::to_owned),
        }
        .to_string()
    };
    let cases = [
        (
            beneath(Code::PathShadowed, false, None),
            "the directory d above it shares its name with another entry beside it",
        ),
        (
            beneath(Code::PathShadowed, true, None),
            "the directory d it would create would share its name with another entry beside it",
        ),
        (
            beneath(Code::PathCaseCollision, false, Some("D")),
            "the directory d above it differs only in case from D",
        ),
        (
            beneath(Code::PathCaseCollision, true, Some("D")),
            "the directory d it would create would differ only in case from D",
        ),
        (
            beneath(Code::PathMalformed, false, None),
            "the directory d above it has no valid logical path",
        ),
        (
            beneath(Code::PathMalformed, true, None),
            "the directory d it would create would have no valid logical path",
        ),
        (
            beneath(Code::RepoUnreadable, false, None),
            "the directory d above it cannot be read, or an entry that may share its name cannot be read",
        ),
        (
            beneath(Code::RepoUnreadable, true, None),
            "the directory d it would create would be beside an entry that may share its name and cannot be read",
        ),
        (
            beneath(Code::RepoSubmodule, false, None),
            "the directory d above it is a submodule",
        ),
    ];
    for (said, expected) in cases {
        assert_eq!(said, expected);
    }
}
