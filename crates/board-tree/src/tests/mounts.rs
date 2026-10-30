//! Mounts: pointers to another local repository at one commit
//! (docs/architecture/paths.md, Pointers: links and mounts;
//! docs/architecture.md, Testing).

use super::*;

fn mount(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

fn link(target: &str) -> String {
    format!(r#"{{"$type": "board/mount/v1", "path": "{target}"}}"#)
}

/// The board, physical path and object of a Location: its identity and what
/// it holds.
fn identity(location: &Location) -> (&Board, &str, Object) {
    (&location.board, location.path.as_str(), location.object)
}

use board_model::Board;

#[test]
fn a_mount_resolves_into_the_mounted_repository_at_its_commit() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    inner.commit(&[("x.json", "2")]);
    let outer = Scenario::new();
    let to_inner = mount(&inner.path(), pinned);
    let commit = outer.commit(&[("m.json", &to_inner)]);
    let location = resolve(&outer, commit, "/m/x").unwrap();
    assert_eq!(location.board.repository, inner.open().path());
    assert_eq!(location.board.source, Source::Commit(pinned));
    assert_eq!(location.path, "x.json");
    assert_eq!(location.object, Object::Blob(inner.blob("1")));
    assert_eq!(location.pointers.len(), 1);
    assert_eq!(location.pointers[0].path, "m.json");
    assert_eq!(location.pointers[0].board.repository, outer.open().path());
}

#[test]
fn a_mount_at_the_end_resolves_to_the_mounted_root() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    let outer = Scenario::new();
    let to_inner = mount(&inner.path(), pinned);
    let commit = outer.commit(&[("m.json", &to_inner)]);
    let location = resolve(&outer, commit, "/m").unwrap();
    assert_eq!(
        (location.path.as_str(), location.object),
        ("", Object::Directory)
    );
    assert_eq!(location.board.source, Source::Commit(pinned));
}

#[test]
fn a_link_can_lead_into_a_mount() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    let outer = Scenario::new();
    let (to_inner, to_x) = (mount(&inner.path(), pinned), link("/m/x"));
    let commit = outer.commit(&[("m.json", &to_inner), ("l.json", &to_x)]);
    let location = resolve(&outer, commit, "/l").unwrap();
    assert_eq!(location.board.source, Source::Commit(pinned));
    assert_eq!(location.path, "x.json");
    assert_eq!(location.pointers.len(), 2);
}

#[test]
fn an_absolute_path_inside_a_mounted_board_is_read_against_that_board() {
    let inner = Scenario::new();
    let to_own_x = link("/x");
    let pinned = inner.commit(&[("x.json", "inner"), ("a.json", &to_own_x)]);
    let outer = Scenario::new();
    let to_inner = mount(&inner.path(), pinned);
    let commit = outer.commit(&[("m.json", &to_inner), ("x.json", "outer")]);
    let location = resolve(&outer, commit, "/m/a").unwrap();
    assert_eq!(location.object, Object::Blob(inner.blob("inner")));
    assert_eq!(location.board.repository, inner.open().path());
}

#[test]
fn mounts_nest() {
    let inner = Scenario::new();
    let inner_commit = inner.commit(&[("x.json", "deep")]);
    let middle = Scenario::new();
    let to_inner = mount(&inner.path(), inner_commit);
    let middle_commit = middle.commit(&[("n.json", &to_inner)]);
    let outer = Scenario::new();
    let to_middle = mount(&middle.path(), middle_commit);
    let commit = outer.commit(&[("m.json", &to_middle)]);
    let location = resolve(&outer, commit, "/m/n/x").unwrap();
    assert_eq!(location.object, Object::Blob(inner.blob("deep")));
    assert_eq!(location.pointers.len(), 2);
}

#[test]
fn one_board_mounted_twice_is_one_identity() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    let outer = Scenario::new();
    let to_inner = mount(&inner.path(), pinned);
    let commit = outer.commit(&[("a.json", &to_inner), ("b.json", &to_inner)]);
    let first = resolve(&outer, commit, "/a/x").unwrap();
    let second = resolve(&outer, commit, "/b/x").unwrap();
    assert_eq!(identity(&first), identity(&second));
    assert_ne!(first.pointers, second.pointers);
}

#[test]
fn resolving_and_reading_through_one_mounted_repository_opens_it_once_and_a_second_repository_opens_once_more()
 {
    let (one, two) = (Scenario::new(), Scenario::new());
    let (a, b) = (
        one.commit(&[("x.json", "1")]),
        two.commit(&[("x.json", "2")]),
    );
    let outer = Scenario::new();
    // The second names the first repository through `..`, a path that only
    // canonicalising matches, and the third by its `.git` directory.
    let one_path = std::path::PathBuf::from(one.path());
    let through_parent = format!(
        "{}/../{}",
        one.path(),
        one_path.file_name().unwrap().to_str().unwrap()
    );
    let commit = outer.commit(&[
        ("m.json", &mount(&one.path(), a)),
        ("n.json", &mount(&through_parent, a)),
        ("g.json", &mount(&format!("{}/.git", one.path()), a)),
        ("o.json", &mount(&two.path(), b)),
    ]);
    let repositories = outer.repositories();
    for asked in ["/m/x", "/n/x", "/g/x", "/m/x", "/g/x"] {
        let location = repositories
            .resolve(&commit.source(), &path(asked))
            .unwrap();
        let read = repositories.read(&mut repositories.root().reader(), &location, one.blob("1"));
        let read = read.unwrap();
        assert_eq!(read.unwrap().bytes, b"1", "{asked}");
    }
    assert_eq!(repositories.opened(), 1);
    repositories
        .resolve(&commit.source(), &path("/o/x"))
        .unwrap();
    assert_eq!(repositories.opened(), 2);
}

#[test]
fn a_linked_worktree_named_by_its_directory_and_by_its_git_file_is_opened_once() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    // A linked worktree as `git worktree add --detach` lays it out: its
    // `.git` is a file naming a git directory inside the main one.
    let linked = format!("{}/linked", inner.path());
    let git_dir = format!("{}/.git/worktrees/linked", inner.path());
    inner
        .write(
            ".git/worktrees/linked/HEAD",
            Item::File(&format!("{pinned}\n")),
        )
        .write(".git/worktrees/linked/commondir", Item::File("../..\n"))
        .write(
            ".git/worktrees/linked/gitdir",
            Item::File(&format!("{linked}/.git\n")),
        )
        .write("linked/.git", Item::File(&format!("gitdir: {git_dir}\n")));
    let outer = Scenario::new();
    let commit = outer.commit(&[
        ("d.json", &mount(&linked, pinned)),
        ("f.json", &mount(&format!("{linked}/.git"), pinned)),
    ]);
    let repositories = outer.repositories();
    for asked in ["/d/x", "/f/x", "/d/x", "/f/x"] {
        repositories
            .resolve(&commit.source(), &path(asked))
            .unwrap();
    }
    assert_eq!(repositories.opened(), 1);
}

#[test]
fn a_mount_of_the_boards_own_repository_opens_nothing_and_another_opens_one() {
    let outer = Scenario::new();
    let pinned = outer.commit(&[("x.json", "1")]);
    let inner = Scenario::new();
    let other = inner.commit(&[("x.json", "2")]);
    let commit = outer.commit(&[
        ("self.json", &mount(".", pinned)),
        ("other.json", &mount(&inner.path(), other)),
    ]);
    let repositories = outer.repositories();
    let location = repositories
        .resolve(&commit.source(), &path("/self/x"))
        .unwrap();
    assert_eq!(location.board.repository, outer.open().path());
    assert_eq!(repositories.opened(), 0);
    repositories
        .resolve(&commit.source(), &path("/other/x"))
        .unwrap();
    assert_eq!(repositories.opened(), 1);
}

#[test]
fn a_mount_of_a_repository_that_cannot_be_opened_is_repo_missing() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    let outer = Scenario::new();
    let present = mount(&inner.path(), pinned);
    let commit = outer.commit(&[("m.json", &present)]);
    assert!(resolve(&outer, commit, "/m/x").is_ok());
    let absent = mount(&format!("{}/nowhere", inner.path()), pinned);
    let commit = outer.commit(&[("m.json", &absent)]);
    assert_eq!(
        resolve(&outer, commit, "/m/x").unwrap_err(),
        Code::MountRepoMissing
    );
}

#[test]
fn a_home_relative_mount_needs_a_home() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    let inner_path = std::path::PathBuf::from(inner.path());
    let name = inner_path.file_name().unwrap().to_str().unwrap();
    let outer = Scenario::new();
    let to_inner = mount(&format!("~/{name}"), pinned);
    let commit = outer.commit(&[("m.json", &to_inner)]);
    let home = inner_path.parent().unwrap().to_path_buf();
    let with_home = Repositories::open(outer.workdir(), Some(home)).unwrap();
    assert!(with_home.resolve(&commit.source(), &path("/m/x")).is_ok());
    assert_eq!(
        resolve(&outer, commit, "/m/x").unwrap_err(),
        Code::MountRepoMissing
    );
}

#[test]
fn a_mount_of_a_commit_the_repository_lacks_is_commit_missing() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    let outer = Scenario::new();
    let present = mount(&inner.path(), pinned);
    let commit = outer.commit(&[("m.json", &present)]);
    assert!(resolve(&outer, commit, "/m/x").is_ok());
    let absent = mount(&inner.path(), ObjectId::Sha1([9; 20]));
    let commit = outer.commit(&[("m.json", &absent)]);
    assert_eq!(
        resolve(&outer, commit, "/m/x").unwrap_err(),
        Code::MountCommitMissing
    );
}

#[test]
fn a_mount_whose_commit_is_missing_is_named_by_the_mount_and_not_the_path_asked() {
    let inner = Scenario::new();
    inner.commit(&[("x.json", "1")]);
    let outer = Scenario::new();
    let absent = mount(&inner.path(), ObjectId::Sha1([9; 20]));
    let commit = outer.commit(&[("vendor/m.json", &absent), ("l.json", &link("/vendor/m"))]);
    for asked in ["/vendor/m/x", "/l/x"] {
        let error = outer
            .repositories()
            .resolve(&commit.source(), &path(asked))
            .unwrap_err();
        assert_eq!(error.code(), Code::MountCommitMissing);
        assert_eq!(error.fields().path, Some(path("/vendor/m")), "{asked}");
        assert_eq!(
            error.to_string(),
            format!(
                "/vendor/m mounts commit {}, which its repository does not hold",
                ObjectId::Sha1([9; 20])
            )
        );
    }
}

#[test]
fn a_remote_mount_is_refused() {
    let outer = Scenario::new();
    let remote = mount("https://example.com/board.git", ObjectId::Sha1([9; 20]));
    let commit = outer.commit(&[("m.json", &remote)]);
    assert_eq!(
        resolve(&outer, commit, "/m/x").unwrap_err(),
        Code::MountRemoteUnsupported
    );
}

#[test]
fn a_link_dangles_when_its_mount_moves_to_a_commit_without_the_target() {
    let inner = Scenario::new();
    let with_x = inner.commit(&[("x.json", "1")]);
    let without_x = inner.commit(&[("y.json", "1")]);
    let outer = Scenario::new();
    let to_x = link("/m/x");
    let (before, after) = (
        mount(&inner.path(), with_x),
        mount(&inner.path(), without_x),
    );
    let first = outer.commit(&[("m.json", &before), ("l.json", &to_x)]);
    assert_eq!(resolve(&outer, first, "/l").unwrap().path, "x.json");
    let second = outer.commit(&[("m.json", &after), ("l.json", &to_x)]);
    assert_eq!(
        resolve(&outer, second, "/l").unwrap_err(),
        Code::RefDangling
    );
}

#[test]
fn a_dangling_link_inside_a_mount_is_named_through_its_mount_point() {
    let inner = Scenario::new();
    let to_y = link("/y");
    let pinned = inner.commit(&[("links/x.json", &to_y), ("z.json", "{}")]);
    let outer = Scenario::new();
    let to_inner = mount(&inner.path(), pinned);
    let commit = outer.commit(&[("m.json", &to_inner)]);
    let error = outer
        .repositories()
        .resolve(&commit.source(), &path("/m/links/x"))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "the link at /m/links/x points to /y, which is missing"
    );
}

#[test]
fn an_uncommitted_root_reads_committed_mounted_content() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "committed")]);
    inner.write("x.json", Item::File("edited"));
    let outer = Scenario::new();
    let to_inner = mount(&inner.path(), pinned);
    outer
        .write("m.json", Item::File(&to_inner))
        .write("y.json", Item::File("uncommitted"));
    let mounted = resolve(&outer, WorkingTree, "/m/x").unwrap();
    assert_eq!(mounted.object, Object::Blob(inner.blob("committed")));
    assert_eq!(mounted.board.source, Source::Commit(pinned));
    let root = resolve(&outer, WorkingTree, "/y").unwrap();
    assert_eq!(root.board.source, Source::WorkingTree);
    let repositories = outer.repositories();
    let mut view = repositories.snapshot().unwrap();
    for (text, bytes) in [("/m/x", "committed"), ("/y", "uncommitted")] {
        let location = view.resolve(&path(text)).unwrap();
        let Object::Blob(blob) = location.object else {
            panic!("{text} is a directory");
        };
        let read = view.read(&location.board, &location.path, blob);
        assert_eq!(read.unwrap().unwrap().bytes, bytes.as_bytes(), "{text}");
    }
}

#[test]
fn a_board_that_mounts_its_own_parent_ends_where_the_pins_run_out() {
    let outer = Scenario::new();
    let inner = Scenario::new();
    let inner_first = inner.commit(&[("x.json", "1")]);
    let to_inner_first = mount(&inner.path(), inner_first);
    let outer_first = outer.commit(&[("m.json", &to_inner_first)]);
    let to_outer = mount(&outer.path(), outer_first);
    let inner_second = inner.commit(&[("p.json", &to_outer)]);
    let to_inner_second = mount(&inner.path(), inner_second);
    let outer_second = outer.commit(&[("m.json", &to_inner_second)]);
    let location = resolve(&outer, outer_second, "/m/p/m/x").unwrap();
    assert_eq!(location.board.source, Source::Commit(inner_first));
    assert_eq!(location.pointers.len(), 3);
    let missing = outer
        .repositories()
        .resolve(&outer_second.source(), &path("/m/p/m/p/q"))
        .unwrap_err();
    assert_eq!(missing.code(), Code::PathNotFound);
    assert_eq!(missing.fields().ancestor, Some(path("/m/p/m")));
}

#[test]
fn a_cycle_through_a_mounted_board_is_refused() {
    let inner = Scenario::new();
    let (to_b, to_a) = (link("/b"), link("/a"));
    let pinned = inner.commit(&[("a.json", &to_b), ("b.json", &to_a), ("c.json", "{}")]);
    let outer = Scenario::new();
    let to_inner = mount(&inner.path(), pinned);
    let commit = outer.commit(&[("m.json", &to_inner)]);
    assert!(resolve(&outer, commit, "/m/c").is_ok());
    assert_eq!(
        resolve(&outer, commit, "/m/a").unwrap_err(),
        Code::ResolveCycle
    );
}

#[test]
fn a_display_path_keeps_mount_points_and_replaces_links() {
    let inner = Scenario::new();
    let inner_commit = inner.commit(&[("x.json", "deep")]);
    let middle = Scenario::new();
    let to_inner = mount(&inner.path(), inner_commit);
    let to_x = link("/n/x");
    let middle_commit = middle.commit(&[("n.json", &to_inner), ("l.json", &to_x)]);
    let outer = Scenario::new();
    let to_middle = mount(&middle.path(), middle_commit);
    let to_deep = link("/m/l");
    let commit = outer.commit(&[("m.json", &to_middle), ("k.json", &to_deep)]);
    for (asked, display) in [
        ("/m/n/x", "/m/n/x"),
        ("/m/l", "/m/n/x"),
        ("/k", "/m/n/x"),
        ("/m", "/m"),
        ("/m/n", "/m/n"),
    ] {
        let location = resolve(&outer, commit, asked).unwrap();
        assert_eq!(
            location.display_path().unwrap().as_str(),
            display,
            "{asked}"
        );
        let again = resolve(&outer, commit, display).unwrap();
        assert_eq!(identity(&again), identity(&location), "{asked}");
    }
}

#[test]
fn a_write_through_a_mount_is_refused_naming_the_mount_and_a_write_beside_it_is_admitted() {
    let inner = Scenario::new();
    let inner_commit = inner.commit(&[("x.json", "1")]);
    let middle = Scenario::new();
    let to_inner = mount(&inner.path(), inner_commit);
    let middle_commit = middle.commit(&[("n.json", &to_inner)]);
    let outer = Scenario::new();
    outer
        .write("m.json", Item::File(&mount(&middle.path(), middle_commit)))
        .write("l.json", Item::File(&link("/m")));
    let repository = outer.repositories();
    for asked in ["/m/new", "/m/n/x", "/l/new", "/l/n/new"] {
        let error = repository.target(&path(asked), false).unwrap_err();
        assert_eq!(error.code(), Code::PutThroughMount, "{asked}");
        assert_eq!(
            error.fields(),
            board_model::Fields::at(&path("/m")),
            "{asked}"
        );
    }
    let admitted = repository.target(&path("/new"), false).unwrap();
    assert_eq!(admitted.path, "new.json");
    // A put may write through one mount, so the error says where it leads.
    let inside = |asked: &str| match repository.target(&path(asked), false) {
        Err(WriteError::ThroughMount { why, inside, .. }) => (*why, inside.map(|inside| *inside)),
        other => panic!("{asked} gave {other:?}"),
    };
    let one = Some(Inside {
        repository: middle.open().path().to_path_buf(),
        commit: middle_commit,
        file: "new.json".to_owned(),
        path: path("/new"),
    });
    assert_eq!(inside("/m/new"), (Through::Mounted, one.clone()));
    assert_eq!(inside("/l/new"), (Through::Mounted, one));
    assert_eq!(inside("/m/n/x"), (Through::Nested, None));
    assert_eq!(inside("/l/n/new"), (Through::Nested, None));
}

#[test]
fn a_commit_descends_from_an_ancestor_found_within_the_limit_and_not_beyond_it() {
    let scenario = Scenario::new();
    let first = scenario.commit(&[("x.json", "1")]);
    scenario.commit(&[("x.json", "2")]);
    let third = scenario.commit(&[("x.json", "3")]);
    let repository = scenario.open();
    assert!(repository.descends(third, first, 2).unwrap());
    assert!(!repository.descends(third, first, 1).unwrap());
    assert!(!repository.descends(first, third, ANCESTRY).unwrap());
}

#[test]
fn pinning_a_link_is_refused_naming_the_link_and_pinning_a_mount_is_admitted() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    let outer = Scenario::new();
    outer
        .write("m.json", Item::File(&mount(&inner.path(), pinned)))
        .write("l.json", Item::File(&link("/m")));
    let error = outer.repositories().pin(&path("/l")).unwrap_err();
    assert_eq!(error.code(), Code::MountInvalid);
    assert_eq!(error.fields(), board_model::Fields::at(&path("/l")));
    assert_eq!(outer.repositories().pin(&path("/m")).unwrap().new, pinned);
}

#[test]
fn pinning_a_symlink_to_a_mount_is_refused_naming_the_symlink_and_pinning_its_mount_is_admitted() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("x.json", "1")]);
    let outer = Scenario::new();
    outer
        .write("m.json", Item::File(&mount(&inner.path(), pinned)))
        .write("s.json", Item::Symlink("m.json"));
    let error = outer.repositories().pin(&path("/s")).unwrap_err();
    assert_eq!(error.code(), Code::PutSymlink);
    assert_eq!(error.fields(), board_model::Fields::at(&path("/s")));
    assert_eq!(outer.repositories().pin(&path("/m")).unwrap().new, pinned);
}

#[test]
fn resolving_into_a_mount_reads_the_size_of_each_symlink_listed_and_of_no_file_while_a_walk_reads_every_size()
 {
    let inner = Scenario::new();
    let names: Vec<String> = (0..50).map(|n| format!("d/r{n}.json")).collect();
    let mut items: Vec<(&[u8], Item<'_>)> = names
        .iter()
        .map(|name| (name.as_bytes(), Item::File("{}")))
        .collect();
    // Over 4,096 bytes, so it dangles without its text being read.
    let long = format!("{}r0.json", "./".repeat(2100));
    items.push((b"d/s.json", Item::Symlink("r0.json")));
    items.push((b"d/long.json", Item::Symlink(&long)));
    let pinned = inner.commit_items(&items);
    let outer = Scenario::new();
    let commit = outer.commit(&[("m.json", &mount(&inner.path(), pinned))]);
    let repositories = outer.repositories();
    let mounted = repositories
        .repository(&Board {
            repository: inner.open().path().to_path_buf(),
            source: Source::Commit(pinned),
        })
        .unwrap();
    let mut in_commit = repositories.in_commit(repositories.root(), commit).unwrap();
    for n in 0..50 {
        in_commit.resolve(&path(&format!("/m/d/r{n}"))).unwrap();
    }
    let symlink = in_commit.resolve(&path("/m/d/s")).unwrap();
    assert_eq!(symlink.object, Object::Blob(inner.blob("{}")));
    let refused = in_commit.resolve(&path("/m/d/long")).unwrap_err();
    assert_eq!(refused.code(), Code::SymlinkDangling);
    // Each resolution decodes the mounted root and `d` together, and only
    // the two symlinks in `d` have a size read: two for each pair decoded.
    let decoded = mounted.trees_decoded();
    assert_eq!(mounted.sizes_read(), decoded);
    assert_eq!(repositories.root().sizes_read(), 0);
    // A walk gives every file its size: the 50 files and both symlinks.
    let walked = mounted.walk(&Source::Commit(pinned)).unwrap();
    let sized = walked.iter().filter(|entry| entry.size.is_some()).count();
    assert_eq!(sized, 51, "every file but the dangling symlink");
    assert_eq!(mounted.sizes_read() - decoded, 52);
}
