//! Git symlinks, read as the file they name inside their tree
//! (docs/architecture/paths/symlinks.md).

use super::*;
use board_model::MAX_SYMLINKS;

use crate::symlink::{Chains, End, Refused};
use crate::tree::Tree;

const TARGET: &str = r#"{"title": "x"}"#;

/// The walked entry at the physical `path` in `at`.
fn entry(scenario: &Scenario, at: impl At, path: &str) -> Entry {
    let entries = scenario.open().walk(&at.source()).unwrap();
    entries
        .into_iter()
        .find(|entry| entry.path == path)
        .unwrap_or_else(|| panic!("no entry {path}"))
}

/// Commits `work/x.json` and the symlinks `links`, and writes the same
/// into the working tree, so each test reads both.
fn both(scenario: &Scenario, links: &[(&str, &str)]) -> ObjectId {
    let mut items: Vec<(&[u8], Item<'_>)> = vec![(b"work/x.json", Item::File(TARGET))];
    items.extend(
        links
            .iter()
            .map(|(path, target)| (path.as_bytes(), Item::Symlink(target))),
    );
    let commit = scenario.commit_items(&items);
    scenario
        .track(commit)
        .write("work/x.json", Item::File(TARGET));
    for (path, target) in links {
        scenario.write(path, Item::Symlink(target));
    }
    commit
}

/// Asserts, in the commit and in the working tree, that the symlink at
/// `path` reads as `work/x.json`, at the logical path `logical`.
fn reads_as_target(scenario: &Scenario, commit: ObjectId, path: &str, logical: &str) {
    let target = scenario.blob(TARGET);
    for source in [Source::Commit(commit), Source::WorkingTree] {
        let entries = scenario.open().walk(&source).unwrap();
        let link = entries.iter().find(|entry| entry.path == path).unwrap();
        assert_eq!(
            (
                link.logical.as_ref().map(ToString::to_string),
                link.blob,
                link.status
            ),
            (Some(logical.to_owned()), Some(target), Status::Ok),
            "{path} in {source:?}"
        );
        assert_eq!(link.size, Some(TARGET.len() as u64), "{path} in {source:?}");
        let read = scenario
            .open()
            .reader()
            .read(&source, path, target)
            .unwrap()
            .unwrap();
        assert_eq!(read.bytes, TARGET.as_bytes(), "{path} in {source:?}");
        let location = scenario
            .repositories()
            .resolve(&source, &self::path(logical))
            .unwrap();
        assert_eq!(
            location.object,
            Object::Blob(target),
            "{path} in {source:?}"
        );
    }
}

/// Asserts, in the commit and in the working tree, that the symlink at
/// `path` is degraded with `code`, and that resolving it is refused with
/// `code` and a message naming its target `text`.
fn refused(scenario: &Scenario, commit: ObjectId, path: &str, code: Code, text: &str) {
    for source in [Source::Commit(commit), Source::WorkingTree] {
        let entries = scenario.open().walk(&source).unwrap();
        let link = entries.iter().find(|entry| entry.path == path).unwrap();
        assert_eq!(
            (link.blob, link.status),
            (None, Status::Degraded(code)),
            "{path} in {source:?}"
        );
        assert_eq!(
            link.link.as_ref().map(|link| link.target.as_str()),
            Some(text),
            "{path} in {source:?}"
        );
        let logical = link.logical.clone().unwrap();
        let error = scenario
            .repositories()
            .resolve(&source, &logical)
            .unwrap_err();
        assert_eq!(error.code(), code, "{path} in {source:?}");
        assert_eq!(error.fields().target.as_deref(), Some(text));
        let which = match code {
            Code::SymlinkOutside => "is absolute or leaves the repository",
            Code::SymlinkLoop => "loops or passes through more than 32 symlinks",
            _ => "names nothing",
        };
        let predicate = format!("is a symlink to {text:?}, which {which}");
        let target = link.link.as_ref().unwrap().target.clone();
        assert_eq!(format!("is {}", Refused { code, target }), predicate);
        let message = error.to_string();
        assert!(message.contains(&predicate), "{message}");
        assert!(!message.contains("nothing is at"), "{message}");
    }
}

#[test]
fn a_symlink_reads_as_the_file_it_names_at_its_own_path_without_json() {
    let scenario = Scenario::new();
    let commit = both(&scenario, &[("s.json", "work/x.json")]);
    reads_as_target(&scenario, commit, "s.json", "/s");
    let link = entry(&scenario, commit, "s.json").link.unwrap();
    assert_eq!(
        (link.blob, link.target.as_str()),
        (scenario.blob("work/x.json"), "work/x.json")
    );
}

#[test]
fn a_symlink_is_followed_from_its_own_directory_and_one_leaving_the_repository_is_refused() {
    let scenario = Scenario::new();
    let commit = both(
        &scenario,
        &[
            ("deep/in/s.json", "../../work/x.json"),
            ("deep/out.json", "../../work/x.json"),
            ("deep/dot.json", "./../deep/../work/./x.json"),
        ],
    );
    reads_as_target(&scenario, commit, "deep/in/s.json", "/deep/in/s");
    reads_as_target(&scenario, commit, "deep/dot.json", "/deep/dot");
    refused(
        &scenario,
        commit,
        "deep/out.json",
        Code::SymlinkOutside,
        "../../work/x.json",
    );
}

#[test]
fn an_absolute_symlink_is_refused_even_when_it_names_a_file_in_the_repository_and_a_relative_twin_is_followed()
 {
    let scenario = Scenario::new();
    let absolute = scenario.workdir().join("work/x.json");
    let absolute = absolute.to_str().unwrap();
    let commit = both(
        &scenario,
        &[
            ("absolute.json", absolute),
            ("relative.json", "work/x.json"),
        ],
    );
    refused(
        &scenario,
        commit,
        "absolute.json",
        Code::SymlinkOutside,
        absolute,
    );
    reads_as_target(&scenario, commit, "relative.json", "/relative");
}

#[test]
fn a_symlink_to_nothing_or_through_a_file_dangles_and_its_twin_to_a_file_is_followed() {
    let scenario = Scenario::new();
    let commit = both(
        &scenario,
        &[
            ("none.json", "work/none.json"),
            ("through.json", "work/x.json/y.json"),
            ("there.json", "work/x.json"),
        ],
    );
    refused(
        &scenario,
        commit,
        "none.json",
        Code::SymlinkDangling,
        "work/none.json",
    );
    refused(
        &scenario,
        commit,
        "through.json",
        Code::SymlinkDangling,
        "work/x.json/y.json",
    );
    reads_as_target(&scenario, commit, "there.json", "/there");
}

#[test]
fn a_chain_of_symlinks_is_followed_to_its_file_and_a_cycle_is_refused() {
    let scenario = Scenario::new();
    let commit = both(
        &scenario,
        &[
            ("a.json", "b.json"),
            ("b.json", "a.json"),
            ("c.json", "chain/d.json"),
            ("chain/d.json", "../e.json"),
            ("e.json", "work/x.json"),
            ("self.json", "self.json"),
        ],
    );
    refused(&scenario, commit, "a.json", Code::SymlinkLoop, "b.json");
    refused(
        &scenario,
        commit,
        "self.json",
        Code::SymlinkLoop,
        "self.json",
    );
    reads_as_target(&scenario, commit, "c.json", "/c");
    reads_as_target(&scenario, commit, "chain/d.json", "/chain/d");
}

#[test]
fn a_chain_of_as_many_symlinks_as_macos_allows_is_followed_and_one_more_is_refused() {
    let scenario = Scenario::new();
    let names: Vec<String> = (0..=MAX_SYMLINKS).map(|n| format!("l{n}.json")).collect();
    let mut links: Vec<(&str, &str)> = names
        .windows(2)
        .map(|pair| (pair[0].as_str(), pair[1].as_str()))
        .collect();
    links.push((&names[MAX_SYMLINKS], "work/x.json"));
    let commit = both(&scenario, &links);
    // l1 is MAX_SYMLINKS links from the file; l0 is one more.
    reads_as_target(&scenario, commit, "l1.json", "/l1");
    refused(&scenario, commit, "l0.json", Code::SymlinkLoop, "l1.json");
}

#[test]
fn a_symlink_through_a_symlinked_directory_is_followed() {
    let scenario = Scenario::new();
    let commit = both(&scenario, &[("dir", "work"), ("s.json", "dir/x.json")]);
    reads_as_target(&scenario, commit, "s.json", "/s");
}

#[test]
fn a_symlink_to_a_directory_is_a_file_holding_its_target_path_and_is_not_followed() {
    let scenario = Scenario::new();
    let commit = both(&scenario, &[("s", "work")]);
    for source in [Source::Commit(commit), Source::WorkingTree] {
        let entries = scenario.open().walk(&source).unwrap();
        let link = entries.iter().find(|entry| entry.path == "s").unwrap();
        assert_eq!(
            (link.blob, link.status),
            (Some(scenario.blob("work")), Status::Ok),
            "{source:?}"
        );
        let location = scenario
            .repositories()
            .resolve(&source, &path("/s"))
            .unwrap();
        assert_eq!(location.object, Object::Blob(scenario.blob("work")));
        let through = scenario.repositories().resolve(&source, &path("/s/x"));
        assert_eq!(through.unwrap_err().code(), Code::PathNotFound);
    }
}

#[test]
fn a_walk_of_a_subtree_follows_a_symlink_to_a_file_outside_it() {
    let scenario = Scenario::new();
    let commit = both(&scenario, &[("deep/s.json", "../work/x.json")]);
    for source in [Source::Commit(commit), Source::WorkingTree] {
        let entries = scenario.open().walk_under(&source, "deep").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            (entries[0].blob, entries[0].status),
            (Some(scenario.blob(TARGET)), Status::Ok),
            "{source:?}"
        );
    }
}

#[test]
fn a_symlinked_record_and_a_symlink_beside_its_directory_are_addressed_as_files() {
    let scenario = Scenario::new();
    let commit = both(
        &scenario,
        &[("r.json", "work/x.json"), ("work.json", "work/x.json")],
    );
    reads_as_target(&scenario, commit, "r.json", "/r");
    reads_as_target(&scenario, commit, "work.json", "/work");
    let children = entry(&scenario, commit, "work/x.json");
    assert_eq!(
        children.logical,
        Some(path("/work/x")),
        "a symlink is a record beside its directory"
    );
}

#[test]
fn a_commits_stored_trees_walk_its_symlinks_as_the_commit_does() {
    let scenario = Scenario::new();
    let commit = both(
        &scenario,
        &[
            ("s.json", "work/x.json"),
            ("out.json", "../x.json"),
            ("d/s.json", "../s.json"),
            ("dir", "work"),
        ],
    );
    let repository = scenario.open();
    let walked = walk_stored(&repository, commit);
    assert_eq!(walked, repository.walk(&commit.source()).unwrap());
}

#[test]
fn a_stored_tree_whose_names_are_not_all_utf8_walks_as_the_commit_does() {
    let scenario = Scenario::new();
    let commit = scenario.commit_items(&[
        (b"x.json", Item::File("{ not json")),
        (b"x/y.md", Item::File("Y.\n")),
        (b"\xff.md", Item::File("FF.\n")),
        (b"\xfe.md", Item::File("FE.\n")),
        (b"x/s.json", Item::Symlink("../x.json")),
    ]);
    let repository = scenario.open();
    let walked = repository.walk(&commit.source()).unwrap();
    assert_eq!(walked.len(), 5);
    assert_eq!(walk_stored(&repository, commit), walked);
}

#[test]
fn finding_a_path_does_not_follow_a_symlink_and_following_one_finds_its_file() {
    let scenario = Scenario::new();
    let commit = both(&scenario, &[("s.json", "work/x.json")]);
    let repository = scenario.open();
    let blob = scenario.blob(TARGET);
    let committed = &mut repository.commit_tree(commit).unwrap();
    let working = &mut repository.working_tree(None).unwrap().on_demand();
    for path in ["work/x.json", "s.json/y.json", "work", "missing"] {
        assert_eq!(ends(committed, path), None, "{path}");
        assert_eq!(ends(working, path), None, "{path}");
    }
    let file = Some((b"workx.json".to_vec(), blob));
    assert_eq!(ends(committed, "s.json"), file);
    assert_eq!(ends(working, "s.json"), file);
}

/// The names of the file the symlink at `path` leads to, joined, and its
/// blob, or `None` when no symlink is there or it leads to no file.
fn ends<T: Tree>(tree: &mut T, path: &str) -> Option<(Vec<u8>, ObjectId)> {
    let chains = &mut Chains::default();
    let at = chains.find(tree, path.as_bytes()).unwrap();
    match chains.follow(tree, at).unwrap() {
        Some((End::File { at, blob, .. }, _)) => Some((chains.names(at).concat(), blob)),
        _ => None,
    }
}

#[test]
fn a_symlink_to_a_submodule_dangles_and_its_twin_to_a_file_is_followed() {
    let scenario = Scenario::new();
    let commit = scenario.commit_items(&[
        (b"work/x.json", Item::File(TARGET)),
        (b"linked", Item::Submodule),
        (b"sub.json", Item::Symlink("linked")),
        (b"in.json", Item::Symlink("linked/x.json")),
        (b"ok.json", Item::Symlink("work/x.json")),
    ]);
    scenario
        .track(commit)
        .write("work/x.json", Item::File(TARGET))
        .write("linked/x", Item::File(""))
        .write("linked/x", Item::Deleted)
        .write("sub.json", Item::Symlink("linked"))
        .write("in.json", Item::Symlink("linked/x.json"))
        .write("ok.json", Item::Symlink("work/x.json"));
    for (path, text) in [("sub.json", "linked"), ("in.json", "linked/x.json")] {
        refused(&scenario, commit, path, Code::SymlinkDangling, text);
    }
    reads_as_target(&scenario, commit, "ok.json", "/ok");
}

#[test]
fn a_symlink_to_a_file_that_cannot_be_read_is_unreadable_and_its_twin_to_a_file_is_followed() {
    let scenario = Scenario::new();
    let commit = scenario.commit_items(&[
        (b"work/x.json", Item::File(TARGET)),
        (b"ok.json", Item::Symlink("work/x.json")),
    ]);
    scenario
        .track(commit)
        .write("work/x.json", Item::File(TARGET))
        .write("gone.json", Item::Unreadable("{}"))
        .write("s.json", Item::Symlink("gone.json"))
        .write("ok.json", Item::Symlink("work/x.json"));
    let link = entry(&scenario, WorkingTree, "s.json");
    assert_eq!(
        (link.blob, link.status),
        (None, Status::Degraded(Code::RepoUnreadable))
    );
    reads_as_target(&scenario, commit, "ok.json", "/ok");
}

/// The walked status of each of `paths` in `commit`, and the code that
/// resolving it is refused with, if it is.
fn in_commit(scenario: &Scenario, commit: ObjectId, paths: &[&str]) -> Vec<(Status, Option<Code>)> {
    let entries = scenario.open().walk(&commit.source()).unwrap();
    paths
        .iter()
        .map(|path| {
            let entry = entries.iter().find(|entry| entry.path == *path).unwrap();
            let logical = entry.logical.clone().unwrap();
            let resolved = scenario.repositories().resolve(&commit.source(), &logical);
            (entry.status, resolved.err().map(|error| error.code()))
        })
        .collect()
}

/// The walk of the HEAD commit `commit` from the trees an index stores.
fn walk_stored(repository: &Repository, commit: ObjectId) -> Vec<Entry> {
    let root = repository.head_tree().unwrap().unwrap();
    let stored = repository.trees(root, &mut |_| false).unwrap();
    // The index gives a tree's rows in the order of their names.
    let mut rows = |id| {
        let tree = stored.iter().find(|tree: &&crate::TreeRows| tree.id == id);
        let mut rows = tree?.rows.clone();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        Some(rows)
    };
    crate::walk_stored(repository, commit, root, &mut rows).unwrap()
}

#[test]
fn a_symlink_text_that_is_empty_too_long_not_utf8_or_a_lossy_spelling_dangles_and_its_twins_are_followed()
 {
    let scenario = Scenario::new();
    // `.` and empty names are skipped, so each text names work/x.json.
    let longest = format!(".{}work/x.json", "/".repeat(4084));
    // Its first 4,096 bytes would name work/x.json.
    let longer = format!(".{}work/x.jsonX", "/".repeat(4084));
    assert_eq!((longest.len(), longer.len()), (4096, 4097));
    let commit = scenario.commit_items(&[
        (b"work/x.json", Item::File(TARGET)),
        // A file whose name is not UTF-8, which a walk shows lossily.
        (b"work/x\xff.json", Item::File(TARGET)),
        (b"empty.json", Item::Symlink("")),
        (b"longer.json", Item::Symlink(&longer)),
        (b"bytes.json", Item::LinkBytes(b"work/x\xff.json")),
        // The lossy spelling of the name that is not UTF-8, and a file
        // whose name holds U+FFFD itself.
        (b"lossy.json", Item::Symlink("work/x\u{FFFD}.json")),
        (b"work/y\xef\xbf\xbd.json", Item::File(TARGET)),
        // The same for a directory.
        (b"bad\xfe/f.json", Item::File(TARGET)),
        (b"lossy-dir.json", Item::Symlink("bad\u{FFFD}/f.json")),
        (b"real\xef\xbf\xbd/f.json", Item::File(TARGET)),
        // Names that differ only in their bytes that are not UTF-8.
        (b"twins/\xff.json", Item::File("{}")),
        (b"twins/\xef\xbf\xbd.json", Item::File(TARGET)),
        (b"longest.json", Item::Symlink(&longest)),
        (b"ok.json", Item::Symlink("work/x.json")),
        (b"replacement.json", Item::Symlink("work/y\u{FFFD}.json")),
        (b"real-dir.json", Item::Symlink("real\u{FFFD}/f.json")),
        (b"pair.json", Item::Symlink("twins/\u{FFFD}.json")),
    ]);
    let dangling = (
        Status::Degraded(Code::SymlinkDangling),
        Some(Code::SymlinkDangling),
    );
    let refused = [
        "empty.json",
        "longer.json",
        "bytes.json",
        "lossy.json",
        "lossy-dir.json",
    ];
    assert_eq!(in_commit(&scenario, commit, &refused), vec![dangling; 5]);
    let followed = [
        "longest.json",
        "ok.json",
        "replacement.json",
        "real-dir.json",
        "pair.json",
    ];
    assert_eq!(
        in_commit(&scenario, commit, &followed),
        vec![(Status::Ok, None); 5]
    );
    let repository = scenario.open();
    let walked = repository.walk(&commit.source()).unwrap();
    let pair = walked
        .iter()
        .find(|entry| entry.path == "pair.json")
        .unwrap();
    assert_eq!(pair.blob, Some(scenario.blob(TARGET)));
    // An index's rows hold the exact names, and its walk follows them as
    // the commit's does.
    assert_eq!(walk_stored(&repository, commit), walked);
}

#[test]
fn a_long_symlink_text_is_shown_by_its_start_and_length_and_a_short_one_whole() {
    let scenario = Scenario::new();
    let long = "a".repeat(20_000);
    let huge = "a".repeat(board_model::MAX_RECORD_BYTES + 1);
    let near = format!("{}.json", "b".repeat(200));
    let limit = "c".repeat(4096);
    let commit = scenario.commit_items(&[
        (b"long.json", Item::Symlink(&long)),
        (b"huge.json", Item::Symlink(&huge)),
        (b"near.json", Item::Symlink(&near)),
        (b"limit.json", Item::Symlink(&limit)),
        (b"short.json", Item::Symlink("gone.json")),
    ]);
    let walked = scenario.open().walk(&commit.source()).unwrap();
    let b = "b".repeat(100);
    // A text over the limit is not loaded, however much longer it is.
    for (path, shown) in [
        ("long.json", "… (over 4096 bytes)".to_owned()),
        ("huge.json", "… (over 4096 bytes)".to_owned()),
        ("near.json", format!("{b}… (205 bytes)")),
        ("limit.json", format!("{}… (4096 bytes)", "c".repeat(100))),
        ("short.json", "gone.json".to_owned()),
    ] {
        let entry = walked.iter().find(|entry| entry.path == path).unwrap();
        assert_eq!(entry.link.as_ref().unwrap().target, shown, "{path}");
        let error = scenario
            .repositories()
            .resolve(&commit.source(), &entry.logical.clone().unwrap())
            .unwrap_err();
        assert_eq!(error.fields().target, Some(shown.clone()), "{path}");
        assert_eq!(error.code(), Code::SymlinkDangling, "{path}");
        assert!(error.to_string().len() < 200, "{error}");
    }
}

#[test]
fn an_invisible_or_control_character_in_a_symlink_text_is_shown_escaped_and_a_plain_text_as_it_is()
{
    let scenario = Scenario::new();
    let forged = "nope\nerror: forged line\u{1b}[31mRED\\";
    let reordered = "gone\u{202e}nosj.\u{200b}x\u{2028}y\u{2029}z";
    let commit = scenario.commit_items(&[
        (b"forged.json", Item::Symlink(forged)),
        (b"reordered.json", Item::Symlink(reordered)),
        (b"plain.json", Item::Symlink("gone.json")),
    ]);
    let walked = scenario.open().walk(&commit.source()).unwrap();
    for (path, shown) in [
        ("forged.json", r"nope\nerror: forged line\u{1b}[31mRED\\"),
        (
            "reordered.json",
            r"gone\u{202e}nosj.\u{200b}x\u{2028}y\u{2029}z",
        ),
        ("plain.json", "gone.json"),
    ] {
        let entry = walked.iter().find(|entry| entry.path == path).unwrap();
        assert_eq!(entry.link.as_ref().unwrap().target, shown, "{path}");
        let error = scenario
            .repositories()
            .resolve(&commit.source(), &entry.logical.clone().unwrap())
            .unwrap_err();
        assert_eq!(error.fields().target.as_deref(), Some(shown), "{path}");
        let printed = format!("{error}\n{}", error.code().hint(&error.fields()));
        assert!(printed.contains(shown), "{printed}");
        assert_eq!(printed.lines().count(), 2, "{printed}");
        assert!(
            !printed.contains(['\u{1b}', '\u{202e}', '\u{200b}', '\u{2028}', '\u{2029}']),
            "{printed}"
        );
    }
}

#[test]
fn a_path_through_a_symlink_that_leads_to_no_file_names_the_symlink_and_the_symlink_itself_names_its_text()
 {
    let scenario = Scenario::new();
    let commit = scenario.commit_items(&[(b"ddir", Item::Symlink("nothing"))]);
    for (path, message) in [
        (
            "/ddir/x",
            r#"/ddir/x passes through /ddir, a symlink to "nothing", which names nothing"#,
        ),
        (
            "/ddir",
            r#"/ddir is a symlink to "nothing", which names nothing"#,
        ),
    ] {
        let error = scenario
            .repositories()
            .resolve(&commit.source(), &self::path(path))
            .unwrap_err();
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn a_chain_of_long_texts_fetches_each_stored_tree_once() {
    let scenario = Scenario::new();
    // Each text goes in and out of `d` as often as fits, then names the
    // next symlink, so following one looks up `d` hundreds of times.
    let text = |next: &str| format!("{}{next}", "d/../".repeat((4096 - next.len()) / 5));
    let names: Vec<String> = (0..MAX_SYMLINKS).map(|n| format!("l{n}.json")).collect();
    let mut texts: Vec<String> = names.windows(2).map(|pair| text(&pair[1])).collect();
    texts.push(text("work/x.json"));
    let mut items: Vec<(&[u8], Item<'_>)> = vec![
        (b"work/x.json", Item::File(TARGET)),
        (b"d/f.md", Item::File("")),
    ];
    items.extend(
        names
            .iter()
            .zip(&texts)
            .map(|(name, text)| (name.as_bytes(), Item::Symlink(text))),
    );
    let commit = scenario.commit_items(&items);
    let repository = scenario.open();
    let root = repository.head_tree().unwrap().unwrap();
    let stored = repository.trees(root, &mut |_| false).unwrap();
    let mut fetched = 0;
    let mut rows = |id| {
        fetched += 1;
        stored
            .iter()
            .find(|tree: &&crate::TreeRows| tree.id == id)
            .map(|tree| tree.rows.clone())
    };
    let walked = crate::walk_stored(&repository, commit, root, &mut rows).unwrap();
    assert_eq!(fetched, stored.len(), "the root, d and work, each once");
    let target = scenario.blob(TARGET);
    for name in &names {
        let link = walked.iter().find(|entry| entry.path == *name).unwrap();
        assert_eq!(
            (link.status, link.blob),
            (Status::Ok, Some(target)),
            "{name}"
        );
    }
    assert_eq!(walked, repository.walk(&commit.source()).unwrap());
}

/// A commit of `f.json` and the symlinks `links`, each with its text.
fn linked(scenario: &Scenario, links: &[(String, String)]) -> ObjectId {
    let mut items: Vec<(&[u8], Item<'_>)> = vec![(b"f.json", Item::File(TARGET))];
    items.extend(
        links
            .iter()
            .map(|(path, text)| (path.as_bytes(), Item::Symlink(text))),
    );
    scenario.commit_items(&items)
}

/// Where the symlink at `path` leads, followed with `chains`.
fn follow(chains: &mut Chains<ObjectId>, tree: &mut crate::git::CommitTree<'_>, path: &str) -> End {
    let at = chains.find(tree, path.as_bytes()).unwrap();
    chains.follow(tree, at).unwrap().unwrap().0
}

#[test]
fn symlinks_that_share_a_chain_of_long_texts_follow_it_once_whether_it_ends_at_a_file_or_loops() {
    // Each text goes in and out of `d` as often as fits, then names the next
    // symlink: `c0` to `c30` end at a file, and `k0` to `k30` loop. Each of
    // the `l` symlinks names `c0` or `k0`.
    let text = |next: &str| format!("{}{next}", "d/../".repeat((4096 - next.len()) / 5));
    let mut links = vec![("d/g.md".to_owned(), "../f.json".to_owned())];
    for hop in 0..31 {
        let next = if hop == 30 {
            "f.json".to_owned()
        } else {
            format!("c{}", hop + 1)
        };
        links.push((format!("c{hop}"), text(&next)));
        links.push((format!("k{hop}"), text(&format!("k{}", (hop + 1) % 31))));
    }
    for link in 0..200 {
        links.push((format!("l{link}"), "c0".to_owned()));
        links.push((format!("m{link}"), "k0".to_owned()));
    }
    let scenario = Scenario::new();
    let commit = linked(&scenario, &links);
    let repository = scenario.open();
    let tree = &mut repository.commit_tree(commit).unwrap();
    let blob = scenario.blob(TARGET);
    for (start, many) in [("c0", 'l'), ("k0", 'm')] {
        let chains = &mut Chains::default();
        let ends = follow(chains, tree, start);
        let once = chains.steps;
        assert!(once > 31 * 800, "{start}: {once} steps");
        for link in 0..200 {
            assert_eq!(follow(chains, tree, &format!("{many}{link}")), ends);
        }
        // Finding each symlink is one step, and its text one more.
        assert_eq!(chains.steps, once + 400, "{start}");
        match ends {
            End::File { blob: found, .. } => assert_eq!((start, found), ("c0", blob)),
            ends => assert_eq!((start, ends), ("k0", End::Refused(Code::SymlinkLoop))),
        }
    }
}

#[test]
fn resolving_many_paths_through_one_chain_follows_it_once_in_one_commit() {
    // Each `p` symlink and the chain from `c0` pass through 32 symlinks.
    let chain = MAX_SYMLINKS - 1;
    let mut links: Vec<(String, String)> = (0..chain)
        .map(|hop| {
            let next = if hop + 1 == chain {
                "f.json".to_owned()
            } else {
                format!("c{}.json", hop + 1)
            };
            (format!("c{hop}.json"), next)
        })
        .collect();
    links.extend((0..100).map(|n| (format!("p{n}.json"), "c0.json".to_owned())));
    let scenario = Scenario::new();
    let commit = linked(&scenario, &links);
    let repositories = scenario.repositories();
    let mut in_commit = repositories.in_commit(repositories.root(), commit).unwrap();
    let blob = Object::Blob(scenario.blob(TARGET));
    for n in 0..100 {
        let location = in_commit.resolve(&path(&format!("/p{n}"))).unwrap();
        assert_eq!(location.object, blob);
    }
    // One step for each text of the chain, and one for each `p` text.
    assert_eq!(in_commit.chains.steps, chain + 100);
}

#[test]
fn a_chain_met_too_deep_to_finish_is_followed_when_met_with_room_and_one_through_a_file_dangles() {
    let mut links: Vec<(String, String)> = (0..MAX_SYMLINKS)
        .map(|hop| {
            let next = if hop + 1 == MAX_SYMLINKS {
                "f.json".to_owned()
            } else {
                format!("c{}", hop + 1)
            };
            (format!("c{hop}"), next)
        })
        .collect();
    for (link, text) in [
        ("s", "f.json"),
        ("a", "c0"),
        ("d", "c0"),
        ("b", "s/x"),
        ("e", "s"),
        ("g", "c0"),
    ] {
        links.push((link.to_owned(), text.to_owned()));
    }
    let scenario = Scenario::new();
    let commit = linked(&scenario, &links);
    let repository = scenario.open();
    let tree = &mut repository.commit_tree(commit).unwrap();
    let chains = &mut Chains::default();
    let mut follow = |chains: &mut Chains<_>, path| match follow(chains, tree, path) {
        End::File { .. } => Ok(()),
        End::Refused(code) => Err(code),
        End::Directory(_) => panic!("{path} leads to a directory"),
    };
    // `a` adds one symlink to the 32 of the chain from `c0`, both before and
    // after `c0` is remembered to reach the file in 32. `g` meets `c0` with
    // no more room than `a` did, so the chain is not followed again.
    assert_eq!(follow(chains, "a"), Err(Code::SymlinkLoop));
    let before = chains.steps;
    assert_eq!(follow(chains, "g"), Err(Code::SymlinkLoop));
    assert_eq!(chains.steps - before, 2, "the names g and c0");
    assert_eq!(follow(chains, "c0"), Ok(()));
    assert_eq!(follow(chains, "d"), Err(Code::SymlinkLoop));
    assert_eq!(follow(chains, "b"), Err(Code::SymlinkDangling));
    assert_eq!(follow(chains, "e"), Ok(()));
}

#[test]
fn reading_many_symlinks_in_one_snapshot_lists_each_directory_once_as_reading_their_files_does() {
    let scenario = Scenario::new();
    for n in 0..50 {
        let target = format!("../files/f{n}.json");
        scenario
            .write(&format!("files/f{n}.json"), Item::File(TARGET))
            .write(&format!("links/l{n}.json"), Item::Symlink(&target));
    }
    let repositories = scenario.repositories();
    let mut view = repositories.snapshot().unwrap();
    for n in 0..50 {
        for text in [format!("/links/l{n}"), format!("/files/f{n}")] {
            let location = view.resolve(&path(&text)).unwrap();
            let Object::Blob(blob) = location.object else {
                panic!("{text} is a directory");
            };
            let read = view.read(&location.board, &location.path, blob);
            assert_eq!(read.unwrap().unwrap().bytes, TARGET.as_bytes(), "{text}");
        }
    }
    let listed = repositories.root().directories_listed();
    assert_eq!(listed, 3, "the root, links and files");
}

#[test]
fn a_symlink_refused_for_each_reason_says_why_in_its_message() {
    let refused = |code| {
        Refused {
            code,
            target: "t".to_owned(),
        }
        .to_string()
    };
    let loops = format!("loops or passes through more than {MAX_SYMLINKS} symlinks");
    for (code, which) in [
        (Code::SymlinkOutside, "is absolute or leaves the repository"),
        (Code::SymlinkLoop, loops.as_str()),
        (Code::SymlinkDangling, "names nothing"),
        (Code::RepoUnreadable, "leads to a file that cannot be read"),
    ] {
        assert_eq!(refused(code), format!("a symlink to \"t\", which {which}"));
    }
}

#[test]
fn a_symlink_retargeted_after_a_snapshot_read_it_is_refused_by_a_new_follow_and_not_followed_again()
{
    let scenario = Scenario::new();
    scenario
        .write("files/f1.json", Item::File(TARGET))
        .write("files/f2.json", Item::File("{}"))
        .write("links/l1.json", Item::Symlink("../files/f1.json"))
        .write("links/l2.json", Item::Symlink("../files/f1.json"));
    let repository = scenario.open();
    let tree = &mut repository.working_tree(None).unwrap().on_demand();
    let mut follow = |chains: &mut Chains<std::path::PathBuf>, path: &str| {
        let at = chains.find(tree, path.as_bytes()).unwrap();
        chains.follow(tree, at).unwrap().unwrap().0
    };
    let first = &mut Chains::default();
    assert!(matches!(follow(first, "links/l2.json"), End::File { .. }));
    for link in ["links/l1.json", "links/l2.json"] {
        scenario
            .write(link, Item::Deleted)
            .write(link, Item::Symlink("../files/f2.json"));
    }
    // The snapshot holds l2's old text, which no longer matches the file
    // system, so a new follow cannot read it. l1, met for the first time,
    // is read as it now is.
    let second = &mut Chains::default();
    assert_eq!(
        follow(second, "links/l2.json"),
        End::Refused(Code::RepoUnreadable)
    );
    assert!(matches!(follow(second, "links/l1.json"), End::File { .. }));
}

#[test]
fn a_symlink_retargeted_during_a_read_reads_as_the_snapshot_found_it_or_as_changed() {
    let scenario = Scenario::new();
    scenario
        .write("files/f1.json", Item::File(TARGET))
        .write("files/f2.json", Item::File("{}"))
        .write("links/l1.json", Item::Symlink("../files/f1.json"))
        .write("links/l2.json", Item::Symlink("../files/f1.json"));
    let repository = scenario.open();
    let reader = &mut repository.reader();
    let walked = scenario.blob(TARGET);
    let mut read = |path| reader.read(&Source::WorkingTree, path, walked);
    assert_eq!(
        read("links/l2.json").unwrap().unwrap().bytes,
        TARGET.as_bytes()
    );
    for link in ["links/l1.json", "links/l2.json"] {
        scenario
            .write(link, Item::Deleted)
            .write(link, Item::Symlink("../files/f2.json"));
    }
    // l2 is followed as the reader's snapshot first followed it; l1, which
    // it had not followed, leads to a file that no longer holds the blob
    // the walk found.
    assert_eq!(
        read("links/l2.json").unwrap().unwrap().bytes,
        TARGET.as_bytes()
    );
    let changed = read("links/l1.json").unwrap_err();
    assert!(
        matches!(changed, crate::GitError::Changed { .. }),
        "{changed}"
    );
    assert_eq!(changed.code(), Code::RepoUnreadable);
}

#[test]
fn reading_many_symlinks_into_one_chain_follows_the_chain_once_in_a_resolver_and_a_reader() {
    // Each `l` symlink names `c0`, the first of a chain of 31 that ends at
    // `f.json`.
    let steps = |links: usize| {
        let scenario = Scenario::new();
        scenario.write("f.json", Item::File(TARGET));
        for hop in 0..31 {
            let next = if hop == 30 {
                "f.json".to_owned()
            } else {
                format!("c{}", hop + 1)
            };
            scenario.write(&format!("c{hop}"), Item::Symlink(&next));
        }
        for link in 0..links {
            scenario.write(&format!("l{link}.json"), Item::Symlink("c0"));
        }
        let walked = scenario.blob(TARGET);
        let repositories = scenario.repositories();
        let (view, reader) = (
            &mut repositories.snapshot().unwrap(),
            &mut repositories.root().reader(),
        );
        for link in 0..links {
            let location = view.resolve(&path(&format!("/l{link}"))).unwrap();
            let read = view.read(&location.board, &location.path, walked).unwrap();
            assert_eq!(read.unwrap().bytes, TARGET.as_bytes());
            let read = reader
                .read(&Source::WorkingTree, &location.path, walked)
                .unwrap();
            assert_eq!(read.unwrap().bytes, TARGET.as_bytes());
        }
        [view.chains.steps, reader.chains.steps]
    };
    let (few, many) = (steps(5), steps(25));
    // Each further symlink costs its own name and the name `c0`.
    assert_eq!([many[0] - few[0], many[1] - few[1]], [40, 40]);
}
