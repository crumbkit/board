//! Links: pointers to a path in the same board, followed with POSIX symlink
//! semantics (docs/architecture/paths.md, Pointers: links and mounts).

use super::*;
use crate::MAX_HOPS;
use crate::address::Pointers;
use crate::symlink::Chains;
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
fn a_dangling_link_is_named_by_its_display_path_with_the_target_it_points_to() {
    let scenario = Scenario::new();
    let to_y = link("/work/y");
    let to_links = link("/links");
    let commit = scenario.commit(&[
        ("links/x.json", &to_y),
        ("l.json", &to_links),
        ("work/z.json", "{}"),
    ]);
    let message = |text: &str| {
        scenario
            .repositories()
            .resolve(&commit.source(), &path(text))
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        message("/links/x"),
        "the link at /links/x points to /work/y, which is missing"
    );
    assert_eq!(
        message("/l/x/more"),
        "the link at /links/x points to /work/y, which is missing"
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
fn an_invalid_pointer_beside_a_directory_is_shadowed_as_the_walk_reports_it() {
    let scenario = Scenario::new();
    let invalid = r#"{"$type": "board/mount/v1", "path": "/a", "repo": "r"}"#;
    let record = scenario.commit(&[("x.json", "{}"), ("x/c.json", "{}")]);
    assert_eq!(resolve(&scenario, record, "/x").unwrap().path, "x.json");
    assert_eq!(resolve(&scenario, record, "/x/c").unwrap().path, "x/c.json");
    let pointer = scenario.commit(&[("x.json", invalid), ("x/c.json", "{}")]);
    for refused in ["/x", "/x/c"] {
        assert_eq!(
            resolve(&scenario, pointer, refused).unwrap_err(),
            Code::PathShadowed,
            "{refused}"
        );
    }
    assert_eq!(
        walk(&scenario, pointer),
        [
            degraded("x.json", Some("/x"), Code::PathShadowed),
            degraded("x/c.json", Some("/x/c"), Code::PathShadowed),
        ]
    );
    let alone = scenario.commit(&[("x.json", invalid)]);
    assert_eq!(
        resolve(&scenario, alone, "/x").unwrap_err(),
        Code::MountInvalid
    );
}

#[test]
fn a_record_of_pointer_type_too_large_to_read_does_not_shadow_its_directory() {
    let scenario = Scenario::new();
    let large = format!(
        r#"{{"$type": "board/mount/v1", "path": "/a", "pad": "{}"}}"#,
        "a".repeat(board_model::MAX_RECORD_BYTES)
    );
    let commit = scenario.commit(&[("x.json", &large), ("x/c.json", "{}")]);
    assert_eq!(resolve(&scenario, commit, "/x/c").unwrap().path, "x/c.json");
    assert!(walk(&scenario, commit).contains(&ok("x/c.json", "/x/c")));
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

    fn repository(&self) -> &std::path::Path {
        self.inner.repository()
    }

    fn source(&self) -> Source {
        self.inner.source()
    }

    fn root(&self) -> T::Directory {
        self.inner.root()
    }

    fn list(
        &mut self,
        directory: &T::Directory,
    ) -> Result<std::rc::Rc<[RawEntry<T::Directory>]>, crate::GitError> {
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
    let repositories = scenario.repositories();
    let repository = repositories.root();
    for hops in [0, 1, 5, 40] {
        let commit = chain(&scenario, hops);
        let start = if hops == 0 { "/end" } else { "/l0" };
        let mut tree = Counting {
            inner: repository.commit_tree(commit).unwrap(),
            reads: 0,
        };
        let location = repositories
            .resolve_tree(
                &mut tree,
                &path(start),
                true,
                (&mut Pointers::default(), &mut Chains::default()),
            )
            .unwrap();
        assert_eq!(location.pointers.len(), hops);
        assert_eq!(tree.reads, hops + 1, "{hops} hops");
    }
}

#[test]
fn resolving_from_every_link_of_a_chain_parses_each_pointer_once() {
    let scenario = Scenario::new();
    let repositories = scenario.repositories();
    let repository = repositories.root();
    for hops in [1, 5, 39] {
        let commit = chain(&scenario, hops);
        let mut tree = Counting {
            inner: repository.commit_tree(commit).unwrap(),
            reads: 0,
        };
        let mut pointers = Pointers::default();
        for start in 0..hops {
            let location = repositories
                .resolve_tree(
                    &mut tree,
                    &path(&format!("/l{start}")),
                    true,
                    (&mut pointers, &mut Chains::default()),
                )
                .unwrap();
            assert_eq!(location.pointers.len(), hops - start);
        }
        // Each pointer and the record at the end, not one read per hop of
        // every resolution, and the timing log's count says the same.
        assert_eq!(tree.reads, hops + 1, "{hops} hops");
        assert_eq!(pointers.read, tree.reads, "{hops} hops");
    }
}

#[test]
fn many_references_to_one_record_read_it_once_in_one_run() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("projects/hub.json", r#"{"$type": "lr/project/v1"}"#)]);
    let repositories = scenario.repositories();
    let repository = repositories.root();
    let mut tree = Counting {
        inner: repository.commit_tree(commit).unwrap(),
        reads: 0,
    };
    let mut pointers = Pointers::default();
    for _ in 0..1_000 {
        repositories
            .resolve_tree(
                &mut tree,
                &path("/projects/hub"),
                true,
                (&mut pointers, &mut Chains::default()),
            )
            .unwrap();
    }
    assert_eq!(tree.reads, 1);
}

#[test]
fn resolving_many_paths_in_one_commit_decodes_each_directory_once() {
    let scenario = Scenario::new();
    let names: Vec<String> = (0..20).map(|n| format!("a/b/r{n}.json")).collect();
    let files: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), "{}")).collect();
    let commit = scenario.commit(&files);
    let repositories = scenario.repositories();
    let repository = repositories.root();
    let mut in_commit = repositories.in_commit(repository, commit).unwrap();
    let before = repository.trees_decoded();
    for n in 0..20 {
        in_commit.resolve(&path(&format!("/a/b/r{n}"))).unwrap();
    }
    // The root, a and a/b.
    assert_eq!(repository.trees_decoded() - before, 3);
}

#[test]
fn an_invalid_pointer_reached_twice_is_refused_the_same_way_both_times() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[
        (
            "bad.json",
            r#"{"$type": "board/mount/v1", "path": "/x", "extra": 1}"#,
        ),
        ("good.json", &link("/x")),
        ("x.json", "{}"),
    ]);
    let repositories = scenario.repositories();
    let repository = repositories.root();
    let mut tree = Counting {
        inner: repository.commit_tree(commit).unwrap(),
        reads: 0,
    };
    let mut pointers = Pointers::default();
    let mut refused = || {
        repositories
            .resolve_tree(
                &mut tree,
                &path("/bad"),
                true,
                (&mut pointers, &mut Chains::default()),
            )
            .unwrap_err()
    };
    let (first, second) = (refused(), refused());
    assert_eq!(first.code(), Code::MountInvalid);
    assert_eq!(
        (second.code(), second.to_string()),
        (first.code(), first.to_string())
    );
    let mut admitted = || {
        repositories
            .resolve_tree(
                &mut tree,
                &path("/good"),
                true,
                (&mut pointers, &mut Chains::default()),
            )
            .unwrap()
    };
    assert_eq!(admitted(), admitted());
    // The invalid pointer once, then the link and its target once each.
    assert_eq!(tree.reads, 3);
}

#[test]
fn resolving_a_directory_path_reads_no_blob() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("a/b/c.json", "{}"), ("a/b.json", "{}"), ("a/x.json", "{}")]);
    let repositories = scenario.repositories();
    let repository = repositories.root();
    let mut tree = Counting {
        inner: repository.commit_tree(commit).unwrap(),
        reads: 0,
    };
    repositories
        .resolve_tree(
            &mut tree,
            &path("/a"),
            true,
            (&mut Pointers::default(), &mut Chains::default()),
        )
        .unwrap();
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
        let repository = scenario.repositories();
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
    repository: &Repositories,
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
    fn resolving_a_display_path_returns_the_same_location(board in linked_board()) {
        let texts: Vec<(String, String)> = board
            .iter()
            .map(|(name, target)| (name.clone(), target.as_deref().map_or_else(|| "{}".to_owned(), link)))
            .collect();
        let files: Vec<(&str, &str)> = texts.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
        let scenario = Scenario::new();
        let commit = scenario.commit(&files);
        let repository = scenario.repositories();
        for name in 0..6 {
            for text in [format!("/n{name}"), format!("/n{name}/y")] {
                if let Ok(location) = repository.resolve(&commit.source(), &path(&text)) {
                    let display = location.display_path().unwrap();
                    let again = repository.resolve(&commit.source(), &display).unwrap();
                    prop_assert_eq!(
                        (&again.board, &again.path, again.object),
                        (&location.board, &location.path, location.object),
                        "{} displayed as {}", text, display
                    );
                    prop_assert!(again.pointers.is_empty(), "{} displayed as {}", text, display);
                }
            }
        }
    }

    #[test]
    fn resolution_always_ends_with_a_location_or_a_resolution_code(board in linked_board()) {
        let texts: Vec<(String, String)> = board
            .iter()
            .map(|(name, target)| (name.clone(), target.as_deref().map_or_else(|| "{}".to_owned(), link)))
            .collect();
        let files: Vec<(&str, &str)> = texts.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
        let scenario = Scenario::new();
        let commit = scenario.commit(&files);
        let repository = scenario.repositories();
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
        let (left, right) = (first.repositories(), second.repositories());
        for name in 0..6 {
            for text in [format!("/n{name}"), format!("/n{name}/y")] {
                prop_assert_eq!(outcome(&left, one, &text), outcome(&right, two, &text), "{}", text);
            }
        }
    }

    #[test]
    fn a_path_that_names_nothing_names_its_deepest_ancestor_that_resolves(board in linked_board()) {
        let mut texts: Vec<(String, String)> = board
            .iter()
            .map(|(name, target)| (name.clone(), target.as_deref().map_or_else(|| "{}".to_owned(), link)))
            .collect();
        texts.push(("d/e.json".to_owned(), "{}".to_owned()));
        let files: Vec<(&str, &str)> = texts.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
        let scenario = Scenario::new();
        let commit = scenario.commit(&files);
        let repository = scenario.repositories();
        let resolves = |path: &LogicalPath| repository.resolve(&commit.source(), path).is_ok();
        let texts = (0..6)
            .flat_map(|name| [format!("/n{name}/y/z"), format!("/d/n{name}/y")])
            .chain(["/d/e/x".to_owned()]);
        for text in texts {
            let path = path(&text);
            if let Err(crate::ResolveError::NotFound { ancestor, .. }) = repository.resolve(&commit.source(), &path) {
                let depth = ancestor.segments().count();
                prop_assert_eq!(&path.prefix(depth), &ancestor, "{}", text);
                prop_assert!(resolves(&ancestor), "{} names {}", text, ancestor);
                prop_assert!(!resolves(&path.prefix(depth + 1)), "{} names {}", text, ancestor);
            }
        }
    }
}

#[test]
fn a_record_beside_its_directory_that_cannot_be_read_degrades_both() {
    let scenario = Scenario::new();
    let readable = scenario.commit(&[("x.json", "{}"), ("x/z.json", "{}")]);
    assert_eq!(
        walk(&scenario, readable),
        [ok("x.json", "/x"), ok("x/z.json", "/x/z")]
    );
    let missing = scenario.commit_items(&[
        (b"x.json", Item::MissingBlob),
        (b"x/z.json", Item::File("{}")),
    ]);
    assert_eq!(
        walk(&scenario, missing),
        [
            degraded("x.json", Some("/x"), Code::RepoUnreadable),
            degraded("x/z.json", Some("/x/z"), Code::RepoUnreadable),
        ]
    );
    assert_eq!(
        resolve(&scenario, missing, "/x/z").unwrap_err(),
        Code::RepoUnreadable
    );
}

#[test]
fn a_file_that_is_not_json_is_never_followed_as_a_pointer() {
    let scenario = Scenario::new();
    let to_a = link("/a");
    let commit = scenario.commit(&[("a.json", "{}"), ("l.md", &to_a), ("l.json", &to_a)]);
    assert_eq!(resolve(&scenario, commit, "/l").unwrap().path, "a.json");
    assert_eq!(resolve(&scenario, commit, "/l.md").unwrap().path, "l.md");
}
