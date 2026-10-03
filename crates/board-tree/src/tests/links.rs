//! Links: pointers to a path in the same board, followed with POSIX symlink
//! semantics (docs/architecture.md, Pointers: links and mounts).

use super::*;
use crate::MAX_HOPS;
use crate::tree::{RawEntry, Tree};

fn link(target: &str) -> String {
    format!(r#"{{"$type": "board/mount/v1", "path": "{target}"}}"#)
}

/// The physical paths of the pointers a resolution followed.
fn followed(location: &Location) -> Vec<&str> {
    location
        .pointers
        .iter()
        .map(|pointer| pointer.path.as_str())
        .collect()
}

#[test]
fn a_link_resolves_to_its_target_and_records_itself() {
    let scenario = Scenario::new();
    let to_y = link("/work/y");
    let commit = scenario.commit(&[("x.json", &to_y), ("work/y.json", "{}")]);
    let direct = resolve(&scenario, commit, "/work/y").unwrap();
    assert!(direct.pointers.is_empty());
    let through = resolve(&scenario, commit, "/x").unwrap();
    assert_eq!(through.path, "work/y.json");
    assert_eq!(through.object, direct.object);
    assert_eq!(followed(&through), ["x.json"]);
}

#[test]
fn resolution_continues_through_a_link_to_a_directory() {
    let scenario = Scenario::new();
    let to_work = link("/work");
    let commit = scenario.commit(&[("l.json", &to_work), ("work/y.json", "{}")]);
    assert_eq!(
        resolve(&scenario, commit, "/l/y").unwrap().path,
        "work/y.json"
    );
    let end = resolve(&scenario, commit, "/l").unwrap();
    assert_eq!((end.path.as_str(), end.object), ("work", Object::Directory));
}

#[test]
fn a_link_whose_target_is_missing_is_dangling() {
    let scenario = Scenario::new();
    let to_y = link("/work/y");
    let present = scenario.commit(&[("x.json", &to_y), ("work/y.json", "{}")]);
    assert!(resolve(&scenario, present, "/x").is_ok());
    let absent = scenario.commit(&[("x.json", &to_y), ("work/z.json", "{}")]);
    assert_eq!(
        resolve(&scenario, absent, "/x").unwrap_err(),
        Code::RefDangling
    );
}

#[test]
fn a_miss_after_a_good_link_is_not_found_not_dangling() {
    let scenario = Scenario::new();
    let to_work = link("/work");
    let commit = scenario.commit(&[("l.json", &to_work), ("work/y.json", "{}")]);
    pair(
        &scenario,
        commit,
        ("/l/y", "work/y.json"),
        "/l/z",
        Code::PathNotFound,
    );
}

#[test]
fn a_record_that_is_not_a_pointer_cannot_be_passed_through() {
    let scenario = Scenario::new();
    let to_work = link("/work");
    let commit = scenario.commit(&[
        ("l.json", &to_work),
        ("r.json", r#"{"$type": "lr/task/v1"}"#),
        ("work/y.json", "{}"),
    ]);
    pair(
        &scenario,
        commit,
        ("/l/y", "work/y.json"),
        "/r/y",
        Code::PathNotFound,
    );
}

#[test]
fn links_that_return_to_a_path_already_followed_are_a_cycle() {
    let scenario = Scenario::new();
    let (to_a, to_b, to_c) = (link("/a"), link("/b"), link("/c"));
    let chain = scenario.commit(&[("a.json", &to_b), ("b.json", &to_c), ("c.json", "{}")]);
    assert_eq!(resolve(&scenario, chain, "/a").unwrap().path, "c.json");
    let cycle = scenario.commit(&[("a.json", &to_b), ("b.json", &to_a)]);
    assert_eq!(
        resolve(&scenario, cycle, "/a").unwrap_err(),
        Code::ResolveCycle
    );
    let own = scenario.commit(&[("a.json", &to_a)]);
    assert_eq!(
        resolve(&scenario, own, "/a").unwrap_err(),
        Code::ResolveCycle
    );
}

/// A chain of `hops` links, `l0` to `l1` and on, the last leading to `end`.
fn chain(scenario: &Scenario, hops: usize) -> ObjectId {
    let texts: Vec<(String, String)> = (0..hops)
        .map(|hop| {
            let target = if hop + 1 == hops {
                "/end".to_owned()
            } else {
                format!("/l{}", hop + 1)
            };
            (format!("l{hop}.json"), link(&target))
        })
        .chain([("end.json".to_owned(), "{}".to_owned())])
        .collect();
    let files: Vec<(&str, &str)> = texts
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    scenario.commit(&files)
}

#[test]
fn a_resolution_follows_at_most_forty_pointers() {
    assert_eq!(MAX_HOPS, 40);
    let scenario = Scenario::new();
    for hops in [39, 40] {
        let commit = chain(&scenario, hops);
        let location = resolve(&scenario, commit, "/l0").unwrap();
        assert_eq!(
            (location.path.as_str(), location.pointers.len()),
            ("end.json", hops)
        );
    }
    let commit = chain(&scenario, 41);
    assert_eq!(
        resolve(&scenario, commit, "/l0").unwrap_err(),
        Code::ResolveTooManyHops
    );
}

#[test]
fn an_invalid_pointer_refuses_resolution_through_it() {
    let scenario = Scenario::new();
    let valid = link("/y");
    let commit = scenario.commit(&[("x.json", &valid), ("y.json", "{}")]);
    assert!(resolve(&scenario, commit, "/x").is_ok());
    let invalid = r#"{"$type": "board/mount/v1", "path": "/y", "extra": true}"#;
    let commit = scenario.commit(&[("x.json", invalid), ("y.json", "{}")]);
    assert_eq!(
        resolve(&scenario, commit, "/x").unwrap_err(),
        Code::MountInvalid
    );
}

#[test]
fn a_pointer_beside_a_directory_is_shadowed() {
    let scenario = Scenario::new();
    let to_y = link("/y");
    let record = scenario.commit(&[("x.json", "{}"), ("x/z.json", "{}"), ("y.json", "{}")]);
    assert_eq!(resolve(&scenario, record, "/x/z").unwrap().path, "x/z.json");
    let pointer = scenario.commit(&[("x.json", &to_y), ("x/z.json", "{}"), ("y.json", "{}")]);
    assert_eq!(
        resolve(&scenario, pointer, "/x").unwrap_err(),
        Code::PathShadowed
    );
    assert_eq!(
        resolve(&scenario, pointer, "/x/z").unwrap_err(),
        Code::PathShadowed
    );
    assert_eq!(
        walk(&scenario, pointer),
        [
            degraded("x.json", Some("/x"), Code::PathShadowed),
            degraded("x/z.json", Some("/x/z"), Code::PathShadowed),
            ok("y.json", "/y"),
        ]
    );
}

#[test]
fn a_link_in_the_working_tree_is_followed() {
    let scenario = Scenario::new();
    let to_y = link("/work/y");
    scenario
        .write("x.json", Item::File(&to_y))
        .write("work/y.json", Item::File("{}"));
    let location = resolve(&scenario, WorkingTree, "/x").unwrap();
    assert_eq!(location.path, "work/y.json");
    assert_eq!(followed(&location), ["x.json"]);
}

/// A tree that counts the blobs it reads.
struct Counting<T> {
    inner: T,
    reads: usize,
}

impl<T: Tree> Tree for Counting<T> {
    type Directory = T::Directory;

    fn source(&self) -> Source {
        self.inner.source()
    }

    fn root(&self) -> T::Directory {
        self.inner.root()
    }

    fn list(
        &mut self,
        directory: &T::Directory,
    ) -> Result<Vec<RawEntry<T::Directory>>, crate::GitError> {
        self.inner.list(directory)
    }

    fn read(
        &mut self,
        directory: &T::Directory,
        entry: &RawEntry<T::Directory>,
    ) -> Result<Option<Vec<u8>>, crate::GitError> {
        self.reads += 1;
        self.inner.read(directory, entry)
    }
}

#[test]
fn resolving_through_n_pointers_reads_n_pointers_and_the_record_it_reaches() {
    let scenario = Scenario::new();
    let repository = scenario.open();
    for hops in [0, 1, 5, 40] {
        let commit = chain(&scenario, hops);
        let start = if hops == 0 { "/end" } else { "/l0" };
        let mut tree = Counting {
            inner: repository.commit_tree(commit).unwrap(),
            reads: 0,
        };
        let location = repository.resolve_in(&mut tree, &path(start)).unwrap();
        assert_eq!(location.pointers.len(), hops);
        assert_eq!(tree.reads, hops + 1, "{hops} hops");
    }
}

#[test]
fn resolving_a_directory_path_reads_no_blob() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("a/b/c.json", "{}"), ("a/b.json", "{}"), ("a/x.json", "{}")]);
    let repository = scenario.open();
    let mut tree = Counting {
        inner: repository.commit_tree(commit).unwrap(),
        reads: 0,
    };
    repository.resolve_in(&mut tree, &path("/a")).unwrap();
    assert_eq!(tree.reads, 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn a_link_resolves_as_its_target_does(files in files(), pick in any::<prop::sample::Index>()) {
        prop_assume!(files.iter().all(|file| !files.iter().any(|other| other.starts_with(&format!("{file}/")))));
        let scenario = Scenario::new();
        let plain: Vec<(&str, &str)> = files.iter().map(|file| (file.as_str(), "{}")).collect();
        let commit = scenario.commit(&plain);
        let entries = scenario.open().walk(&commit.source()).unwrap();
        let targets: Vec<&LogicalPath> = entries
            .iter()
            .filter(|entry| entry.status == Status::Ok)
            .filter_map(|entry| entry.logical.as_ref())
            .collect();
        prop_assume!(!targets.is_empty());
        let target = targets[pick.index(targets.len())];
        let to_target = link(target.as_str());
        let mut linked = plain.clone();
        linked.push(("zz-link.json", &to_target));
        let commit = scenario.commit(&linked);
        let repository = scenario.open();
        let direct = repository.resolve(&commit.source(), target).unwrap();
        let through = repository.resolve(&commit.source(), &path("/zz-link")).unwrap();
        prop_assert_eq!((&through.board, &through.path, through.object), (&direct.board, &direct.path, direct.object));
        prop_assert_eq!(followed(&through), ["zz-link.json"]);
    }
}

/// A board of up to six names, each a record or a link to one of the names,
/// possibly continuing into a child: links to links, cycles and dangling
/// targets all occur.
fn linked_board() -> impl Strategy<Value = Vec<(String, Option<String>)>> {
    let target = (0..6usize, prop::option::of(Just("y"))).prop_map(|(name, child)| match child {
        Some(child) => format!("/n{name}/{child}"),
        None => format!("/n{name}"),
    });
    prop::collection::vec(prop::option::of(target), 1..6).prop_map(|targets| {
        targets
            .into_iter()
            .enumerate()
            .map(|(name, target)| (format!("n{name}.json"), target))
            .collect()
    })
}

/// The outcome of resolving `text`, without the repository's own path, so
/// that two repositories holding the same tree can be compared.
fn outcome(
    repository: &Repository,
    commit: ObjectId,
    text: &str,
) -> Result<(String, Object, Vec<String>), Code> {
    repository
        .resolve(&commit.source(), &path(text))
        .map(|location| {
            let pointers = location
                .pointers
                .iter()
                .map(|pointer| pointer.path.clone())
                .collect();
            (location.path, location.object, pointers)
        })
        .map_err(|error| error.code())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn resolution_always_ends_with_a_location_or_a_resolution_code(board in linked_board()) {
        let texts: Vec<(String, String)> = board
            .iter()
            .map(|(name, target)| (name.clone(), target.as_deref().map_or_else(|| "{}".to_owned(), link)))
            .collect();
        let files: Vec<(&str, &str)> = texts.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
        let scenario = Scenario::new();
        let commit = scenario.commit(&files);
        let repository = scenario.open();
        for name in 0..6 {
            for text in [format!("/n{name}"), format!("/n{name}/y")] {
                match outcome(&repository, commit, &text) {
                    Ok((_, _, pointers)) => prop_assert!(pointers.len() <= MAX_HOPS),
                    Err(code) => prop_assert!(
                        [Code::PathNotFound, Code::RefDangling, Code::ResolveCycle, Code::ResolveTooManyHops]
                            .contains(&code),
                        "{text}: {code}"
                    ),
                }
            }
        }
    }

    #[test]
    fn a_result_depends_only_on_the_tree_and_the_path(board in linked_board()) {
        let texts: Vec<(String, String)> = board
            .iter()
            .map(|(name, target)| (name.clone(), target.as_deref().map_or_else(|| "{}".to_owned(), link)))
            .collect();
        let files: Vec<(&str, &str)> = texts.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
        let (first, second) = (Scenario::new(), Scenario::new());
        // The second repository has other history before the same tree.
        second.commit(&[("unrelated.json", "{}")]);
        let (one, two) = (first.commit(&files), second.commit(&files));
        let (left, right) = (first.open(), second.open());
        for name in 0..6 {
            for text in [format!("/n{name}"), format!("/n{name}/y")] {
                prop_assert_eq!(outcome(&left, one, &text), outcome(&right, two, &text), "{}", text);
            }
        }
    }
}
